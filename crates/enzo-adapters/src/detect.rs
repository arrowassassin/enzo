//! Prompt-pattern detection for AI CLI output streams.
//!
//! Each AI CLI has its own approval-prompt format. This module provides
//! matchers that scan a rolling line buffer and signal when an approval
//! decision is required.

/// Strip ANSI/VT escape sequences from a string, leaving plain text.
#[must_use]
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    // CSI sequence: read until a letter.
                    for d in chars.by_ref() {
                        if d.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC sequence: read until BEL or ST.
                    for d in chars.by_ref() {
                        if d == '\x07' || d == '\x1b' {
                            break;
                        }
                    }
                }
                Some('(' | ')' | '#') => {
                    chars.next(); // two-char designator
                }
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Description of a detected approval prompt extracted from AI CLI output.
#[derive(Debug, Clone)]
pub struct DetectedPrompt {
    /// Short title describing what the AI CLI wants to do.
    pub title: String,
    /// Full context (the buffered lines surrounding the prompt).
    pub body: String,
    /// Parsed unified diff, if one is present in the context.
    pub diff: Option<ParsedDiff>,
}

/// A parsed unified diff with file path and hunk information.
#[derive(Debug, Clone)]
pub struct ParsedDiff {
    /// Target file path from the `+++` line.
    pub path: String,
    /// Raw diff text including all hunks.
    pub raw: String,
}

/// Rolling line buffer used to detect prompts and extract context.
pub struct PromptDetector {
    /// Recent output lines (ANSI-stripped).
    lines: std::collections::VecDeque<String>,
    /// How many lines of context to retain.
    capacity: usize,
}

impl PromptDetector {
    /// Create a detector that retains `capacity` lines of context.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: std::collections::VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Feed a raw PTY output line (may contain ANSI codes).
    ///
    /// Returns a [`DetectedPrompt`] if the line looks like an approval request.
    pub fn push(&mut self, raw_line: &str) -> Option<DetectedPrompt> {
        let plain = strip_ansi(raw_line);
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(plain.clone());

        if is_approval_line(&plain) {
            Some(self.build_prompt(&plain))
        } else {
            None
        }
    }

    /// Drain all buffered context (call after handling a prompt).
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    fn build_prompt(&self, prompt_line: &str) -> DetectedPrompt {
        let body: String = self.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        let diff = extract_diff(&body);
        let title = extract_title(&body, prompt_line);
        DetectedPrompt { title, body, diff }
    }
}

/// Returns `true` if `line` looks like a yes/no approval request.
fn is_approval_line(line: &str) -> bool {
    let l = line.trim().to_ascii_lowercase();
    l.contains("[y/n]")
        || l.contains("[yes/no]")
        || l.contains("(y/n)")
        || l.ends_with("y or n")
        || l.ends_with("y/n:")
        // Claude Code's tool-use approval phrasing (varies by version).
        || l.contains("do you want to")
        || l.contains("would you like to")
        || l.contains("allow this action")
        || l.contains("proceed?")
        // Numbered yes/no selector lines: "1. yes" / "❯ 1. yes".
        || (l.contains("1.") && l.contains("yes"))
}

/// Extract a human-readable title from the context around a prompt.
fn extract_title(body: &str, prompt_line: &str) -> String {
    // Prefer lines that name the tool / file: look for "Edit(", "Create(",
    // "Bash(", "Write(" patterns (Claude Code tool call syntax).
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("Edit(")
            || t.starts_with("Create(")
            || t.starts_with("Write(")
            || t.starts_with("Bash(")
            || t.starts_with("MultiEdit(")
        {
            return t.to_owned();
        }
        // "claude wants to …" style descriptions
        if t.to_ascii_lowercase().starts_with("claude wants to") {
            return t.to_owned();
        }
    }
    // Fall back to the approval line itself.
    prompt_line.trim().to_owned()
}

/// Extract a unified diff from the context, if one is present.
fn extract_diff(body: &str) -> Option<ParsedDiff> {
    let mut in_diff = false;
    let mut path = String::new();
    let mut raw = String::new();

    for line in body.lines() {
        if line.starts_with("--- ") && !in_diff {
            in_diff = true;
        }
        if line.starts_with("+++ ") && path.is_empty() {
            // "+++ b/src/foo.rs" → "src/foo.rs"
            line.trim_start_matches("+++ ")
                .trim_start_matches("b/")
                .clone_into(&mut path);
        }
        if in_diff {
            raw.push_str(line);
            raw.push('\n');
        }
    }

    if in_diff && !path.is_empty() {
        Some(ParsedDiff { path, raw })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn strip_ansi_passthrough_plain() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn is_approval_line_detects_yn_bracket() {
        assert!(is_approval_line("Do you want to proceed? [y/n]"));
    }

    #[test]
    fn is_approval_line_detects_claude_code_phrases() {
        assert!(is_approval_line("Do you want to create this file?"));
        assert!(is_approval_line("Do you want to run the command?"));
    }

    #[test]
    fn is_approval_line_rejects_normal_output() {
        assert!(!is_approval_line("Compiling enzo v0.1.0"));
        assert!(!is_approval_line("warning: unused import"));
    }

    #[test]
    fn detector_returns_none_for_normal_lines() {
        let mut d = PromptDetector::new(20);
        assert!(d.push("Compiling enzo v0.1.0").is_none());
    }

    #[test]
    fn detector_returns_prompt_on_yn_line() {
        let mut d = PromptDetector::new(20);
        d.push("Edit(path=\"src/foo.rs\")");
        let p = d.push("Do you want to proceed? [y/n]");
        assert!(p.is_some());
        let p = p.unwrap();
        assert!(p.title.contains("Edit("), "title: {}", p.title);
    }

    #[test]
    fn extract_diff_finds_unified_diff() {
        let body = "--- a/src/foo.rs\n+++ b/src/foo.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let diff = extract_diff(body);
        assert!(diff.is_some());
        assert_eq!(diff.unwrap().path, "src/foo.rs");
    }

    #[test]
    fn extract_diff_returns_none_without_diff() {
        let body = "Some random output\nwithout any diff";
        assert!(extract_diff(body).is_none());
    }
}
