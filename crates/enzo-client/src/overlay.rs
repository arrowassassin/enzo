//! ATP overlay state — agent prompt cards and content blocks.
//!
//! Holds the inline approval prompt (design doc §10.6: "approvals are inline
//! blocks you can scroll past; nothing steals focus") and any pushed content
//! blocks. The renderer draws these on top of the active surface; the event
//! loop hit-tests button rects the renderer publishes back here.

/// A unified-diff line classified for colouring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Context line (unchanged).
    Context,
    /// Added line (`+`).
    Add,
    /// Removed line (`-`).
    Remove,
    /// Hunk header (`@@ … @@`).
    Header,
}

/// One parsed line of a diff for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Classification (drives the colour).
    pub kind: DiffLineKind,
    /// The line text (without the leading marker for add/remove/context).
    pub text: String,
}

/// An interactive agent approval prompt awaiting the user's decision.
#[derive(Debug, Clone)]
pub struct PromptCard {
    /// Prompt id (echoed to `prompt.respond`).
    pub id: String,
    /// Title line shown in the card header.
    pub title: String,
    /// Body / context text.
    pub body: String,
    /// File path of the diff, if this is a diff prompt.
    pub diff_path: Option<String>,
    /// Parsed diff lines (empty for text prompts).
    pub diff_lines: Vec<DiffLine>,
    /// Available actions, e.g. `["accept", "reject", "edit"]`.
    pub actions: Vec<String>,
}

impl PromptCard {
    /// Build a card from raw ATP fields, parsing the optional diff `raw` text.
    #[must_use]
    pub fn new(
        id: String,
        title: String,
        body: String,
        diff: Option<serde_json::Value>,
        actions: Vec<String>,
    ) -> Self {
        let (diff_path, diff_lines) = match diff {
            Some(d) => {
                let path = d["path"].as_str().map(str::to_owned);
                let lines = parse_diff(d["raw"].as_str().unwrap_or(""));
                (path, lines)
            }
            None => (None, Vec::new()),
        };
        Self {
            id,
            title,
            body,
            diff_path,
            diff_lines,
            actions,
        }
    }
}

/// Parse raw unified-diff text into classified [`DiffLine`]s.
#[must_use]
pub fn parse_diff(raw: &str) -> Vec<DiffLine> {
    raw.lines()
        .filter(|l| !l.starts_with("--- ") && !l.starts_with("+++ "))
        .map(|line| {
            let (kind, text) = if let Some(rest) = line.strip_prefix('+') {
                (DiffLineKind::Add, rest)
            } else if let Some(rest) = line.strip_prefix('-') {
                (DiffLineKind::Remove, rest)
            } else if line.starts_with("@@") {
                (DiffLineKind::Header, line)
            } else {
                (
                    DiffLineKind::Context,
                    line.strip_prefix(' ').unwrap_or(line),
                )
            };
            DiffLine {
                kind,
                text: text.to_owned(),
            }
        })
        .collect()
}

/// A non-blocking content block pushed by an agent.
#[derive(Debug, Clone)]
pub struct Block {
    /// Block id.
    pub id: String,
    /// Title line.
    pub title: String,
    /// Body text.
    pub body: String,
}

/// All overlay state for the active window.
#[derive(Debug, Default)]
pub struct OverlayState {
    /// The active blocking prompt, if any.
    pub prompt: Option<PromptCard>,
    /// Pushed content blocks (most recent last).
    pub blocks: Vec<Block>,
}

impl OverlayState {
    /// Create empty overlay state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Show a new prompt, replacing any current one.
    pub fn set_prompt(&mut self, card: PromptCard) {
        self.prompt = Some(card);
    }

    /// Clear the active prompt (after the user responds).
    pub fn clear_prompt(&mut self) {
        self.prompt = None;
    }

    /// `true` if a prompt is currently displayed (and steals a/r/e + clicks).
    #[must_use]
    pub fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// Add or replace a content block by id.
    pub fn push_block(&mut self, block: Block) {
        // Keep the block list bounded.
        const MAX_BLOCKS: usize = 8;
        if let Some(existing) = self.blocks.iter_mut().find(|b| b.id == block.id) {
            *existing = block;
        } else {
            self.blocks.push(block);
        }
        if self.blocks.len() > MAX_BLOCKS {
            let overflow = self.blocks.len() - MAX_BLOCKS;
            self.blocks.drain(0..overflow);
        }
    }

    /// Remove a block by id.
    pub fn clear_block(&mut self, id: &str) {
        self.blocks.retain(|b| b.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_diff_classifies_lines() {
        let raw = "@@ -1,2 +1,2 @@\n ctx\n-old\n+new\n";
        let lines = parse_diff(raw);
        assert_eq!(lines[0].kind, DiffLineKind::Header);
        assert_eq!(lines[1].kind, DiffLineKind::Context);
        assert_eq!(lines[1].text, "ctx");
        assert_eq!(lines[2].kind, DiffLineKind::Remove);
        assert_eq!(lines[2].text, "old");
        assert_eq!(lines[3].kind, DiffLineKind::Add);
        assert_eq!(lines[3].text, "new");
    }

    #[test]
    fn parse_diff_skips_file_headers() {
        let raw = "--- a/x.rs\n+++ b/x.rs\n+added\n";
        let lines = parse_diff(raw);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, DiffLineKind::Add);
    }

    #[test]
    fn prompt_card_parses_diff() {
        let card = PromptCard::new(
            "p1".into(),
            "edit x.rs".into(),
            "body".into(),
            Some(json!({ "path": "src/x.rs", "raw": "+a\n-b\n" })),
            vec!["accept".into(), "reject".into()],
        );
        assert_eq!(card.diff_path.as_deref(), Some("src/x.rs"));
        assert_eq!(card.diff_lines.len(), 2);
    }

    #[test]
    fn prompt_card_text_only() {
        let card = PromptCard::new(
            "p2".into(),
            "run cmd".into(),
            "ls -la".into(),
            None,
            vec!["accept".into()],
        );
        assert!(card.diff_path.is_none());
        assert!(card.diff_lines.is_empty());
    }

    #[test]
    fn overlay_prompt_lifecycle() {
        let mut o = OverlayState::new();
        assert!(!o.has_prompt());
        o.set_prompt(PromptCard::new(
            "p".into(),
            "t".into(),
            "b".into(),
            None,
            vec![],
        ));
        assert!(o.has_prompt());
        o.clear_prompt();
        assert!(!o.has_prompt());
    }

    #[test]
    fn block_push_replaces_by_id() {
        let mut o = OverlayState::new();
        o.push_block(Block {
            id: "b1".into(),
            title: "one".into(),
            body: String::new(),
        });
        o.push_block(Block {
            id: "b1".into(),
            title: "two".into(),
            body: String::new(),
        });
        assert_eq!(o.blocks.len(), 1);
        assert_eq!(o.blocks[0].title, "two");
    }

    #[test]
    fn block_clear_removes() {
        let mut o = OverlayState::new();
        o.push_block(Block {
            id: "b1".into(),
            title: "x".into(),
            body: String::new(),
        });
        o.clear_block("b1");
        assert!(o.blocks.is_empty());
    }

    #[test]
    fn blocks_are_bounded() {
        let mut o = OverlayState::new();
        for i in 0..20 {
            o.push_block(Block {
                id: format!("b{i}"),
                title: String::new(),
                body: String::new(),
            });
        }
        assert!(o.blocks.len() <= 8);
    }
}
