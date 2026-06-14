//! Text buffer backed by a `ropey` rope.
//!
//! A rope gives O(log n) edits and line indexing regardless of file size, the
//! foundation the editor widget (design doc §5.2) and the DB SQL editor both
//! build on. The buffer tracks a dirty flag and an associated language id for
//! highlighting and formatter selection.

use ropey::Rope;

use crate::lang::Language;

/// An in-memory text document.
pub struct Buffer {
    rope: Rope,
    /// Detected/assigned language.
    language: Language,
    /// `true` if there are unsaved changes since the last `mark_clean`.
    dirty: bool,
}

impl Buffer {
    /// Create an empty buffer with an unknown language.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            language: Language::PlainText,
            dirty: false,
        }
    }

    /// Create a buffer from initial `text`, inferring the language from `path`.
    #[must_use]
    pub fn from_text(text: &str, path: Option<&str>) -> Self {
        let language = path.map_or(Language::PlainText, Language::from_path);
        Self {
            rope: Rope::from_str(text),
            language,
            dirty: false,
        }
    }

    /// The buffer's full contents as a `String`.
    #[must_use]
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Number of lines (counting a trailing newline's empty final line).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Total length in characters.
    #[must_use]
    pub fn char_count(&self) -> usize {
        self.rope.len_chars()
    }

    /// Get line `idx` (0-based) including its trailing newline, if present.
    #[must_use]
    pub fn line(&self, idx: usize) -> Option<String> {
        if idx < self.rope.len_lines() {
            Some(self.rope.line(idx).to_string())
        } else {
            None
        }
    }

    /// The assigned language.
    #[must_use]
    pub fn language(&self) -> Language {
        self.language
    }

    /// Override the language (e.g. user picks from the status bar).
    pub fn set_language(&mut self, language: Language) {
        self.language = language;
    }

    /// `true` if the buffer has unsaved edits.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the buffer clean (after a successful save).
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Insert `text` at character offset `char_idx`.
    ///
    /// # Errors
    /// Returns an error if `char_idx` is out of bounds.
    pub fn insert(&mut self, char_idx: usize, text: &str) -> anyhow::Result<()> {
        if char_idx > self.rope.len_chars() {
            anyhow::bail!(
                "insert offset {char_idx} out of bounds (len {})",
                self.rope.len_chars()
            );
        }
        self.rope.insert(char_idx, text);
        self.dirty = true;
        Ok(())
    }

    /// Remove the characters in `[start, end)`.
    ///
    /// # Errors
    /// Returns an error if the range is invalid or out of bounds.
    pub fn remove(&mut self, start: usize, end: usize) -> anyhow::Result<()> {
        if start > end || end > self.rope.len_chars() {
            anyhow::bail!(
                "remove range {start}..{end} invalid (len {})",
                self.rope.len_chars()
            );
        }
        self.rope.remove(start..end);
        self.dirty = true;
        Ok(())
    }

    /// Replace the entire contents (e.g. after an external formatter run).
    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.dirty = true;
    }

    /// Convert a (line, column) position to a character offset.
    ///
    /// Columns past the end of a line clamp to the line's length.
    #[must_use]
    pub fn line_col_to_char(&self, line: usize, col: usize) -> Option<usize> {
        if line >= self.rope.len_lines() {
            return None;
        }
        let line_start = self.rope.line_to_char(line);
        // Clamp to the line's content length, excluding any trailing newline so
        // a column past end-of-line lands at the last character, not after `\n`.
        let slice = self.rope.line(line);
        let mut line_len = slice.len_chars();
        if slice.to_string().ends_with('\n') {
            line_len -= 1;
        }
        Some(line_start + col.min(line_len))
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_empty_and_clean() {
        let b = Buffer::new();
        assert_eq!(b.char_count(), 0);
        assert!(!b.is_dirty());
        assert_eq!(b.language(), Language::PlainText);
    }

    #[test]
    fn from_text_infers_language() {
        let b = Buffer::from_text("fn main() {}", Some("src/main.rs"));
        assert_eq!(b.language(), Language::Rust);
        assert!(!b.is_dirty());
    }

    #[test]
    fn insert_sets_dirty_and_content() {
        let mut b = Buffer::new();
        b.insert(0, "hello").unwrap();
        assert_eq!(b.text(), "hello");
        assert!(b.is_dirty());
    }

    #[test]
    fn insert_out_of_bounds_errors() {
        let mut b = Buffer::new();
        assert!(b.insert(5, "x").is_err());
    }

    #[test]
    fn remove_deletes_range() {
        let mut b = Buffer::from_text("hello world", None);
        b.remove(5, 11).unwrap();
        assert_eq!(b.text(), "hello");
    }

    #[test]
    fn remove_invalid_range_errors() {
        let mut b = Buffer::from_text("abc", None);
        assert!(b.remove(2, 1).is_err());
        assert!(b.remove(0, 99).is_err());
    }

    #[test]
    fn line_count_and_line_access() {
        let b = Buffer::from_text("a\nb\nc", None);
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line(0).as_deref(), Some("a\n"));
        assert_eq!(b.line(2).as_deref(), Some("c"));
        assert_eq!(b.line(9), None);
    }

    #[test]
    fn mark_clean_clears_dirty() {
        let mut b = Buffer::new();
        b.insert(0, "x").unwrap();
        assert!(b.is_dirty());
        b.mark_clean();
        assert!(!b.is_dirty());
    }

    #[test]
    fn line_col_conversion() {
        let b = Buffer::from_text("ab\ncde", None);
        assert_eq!(b.line_col_to_char(0, 0), Some(0));
        assert_eq!(b.line_col_to_char(1, 1), Some(4)); // 'd'
        // Column clamps to line length.
        assert_eq!(b.line_col_to_char(0, 99), Some(2));
        assert_eq!(b.line_col_to_char(9, 0), None);
    }

    #[test]
    fn set_text_replaces_and_dirties() {
        let mut b = Buffer::from_text("old", None);
        b.mark_clean();
        b.set_text("new content");
        assert_eq!(b.text(), "new content");
        assert!(b.is_dirty());
    }
}
