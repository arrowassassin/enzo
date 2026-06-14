//! Syntax highlighting via tree-sitter.
//!
//! Produces a flat list of [`HighlightSpan`]s (byte range + capture name) that
//! the GPU renderer maps to theme `syntax.*` colors. Highlight names follow the
//! conventional tree-sitter capture vocabulary (`keyword`, `string`, `function`,
//! …) so they line up with the theme token names in `enzo-theme`.

use anyhow::Context;
use tree_sitter_highlight::{Highlight, HighlightConfiguration, Highlighter};

use crate::lang::Language;

/// The highlight capture names we recognise, in priority order.
///
/// The index into this slice is the [`Highlight`] id tree-sitter reports.
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "keyword",
    "string",
    "number",
    "comment",
    "function",
    "type",
    "variable",
    "constant",
    "operator",
    "punctuation",
    "property",
    "constructor",
];

/// One highlighted span of source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// The capture name (one of [`HIGHLIGHT_NAMES`]).
    pub name: &'static str,
}

/// Build the tree-sitter `HighlightConfiguration` for `language`, if supported.
fn configuration(language: Language) -> anyhow::Result<Option<HighlightConfiguration>> {
    let (ts_language, highlights_query, name): (tree_sitter::Language, &str, &str) = match language
    {
        Language::Rust => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            "rust",
        ),
        Language::Python => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "python",
        ),
        Language::JavaScript => (
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            "javascript",
        ),
        Language::Json => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "json",
        ),
        Language::PlainText => return Ok(None),
    };

    let mut config = HighlightConfiguration::new(ts_language, name, highlights_query, "", "")
        .with_context(|| format!("build highlight config for {name}"))?;
    config.configure(HIGHLIGHT_NAMES);
    Ok(Some(config))
}

/// Highlight `source` for `language`, returning ordered spans.
///
/// Returns an empty `Vec` for [`Language::PlainText`] or if the grammar emits no
/// captures. Spans are non-overlapping and in source order.
///
/// # Errors
/// Returns an error if the grammar configuration or highlight pass fails.
pub fn highlight(language: Language, source: &str) -> anyhow::Result<Vec<HighlightSpan>> {
    let Some(config) = configuration(language)? else {
        return Ok(vec![]);
    };

    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(&config, source.as_bytes(), None, |_| None)
        .context("run highlighter")?;

    let mut spans = Vec::new();
    let mut stack: Vec<Highlight> = Vec::new();
    for event in events {
        use tree_sitter_highlight::HighlightEvent;
        match event.context("highlight event")? {
            HighlightEvent::HighlightStart(h) => stack.push(h),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                if let Some(h) = stack.last()
                    && let Some(&name) = HIGHLIGHT_NAMES.get(h.0)
                {
                    spans.push(HighlightSpan { start, end, name });
                }
            }
        }
    }
    Ok(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_yields_no_spans() {
        let spans = highlight(Language::PlainText, "hello world").unwrap();
        assert!(spans.is_empty());
    }

    #[test]
    fn rust_highlights_keyword() {
        let spans = highlight(Language::Rust, "fn main() {}").unwrap();
        assert!(!spans.is_empty(), "expected some spans");
        // The `fn` keyword should be captured as "keyword".
        let has_keyword = spans.iter().any(|s| s.name == "keyword");
        assert!(has_keyword, "expected a keyword span, got: {spans:?}");
    }

    #[test]
    fn rust_highlights_string() {
        let spans = highlight(Language::Rust, r#"fn f() { let x = "hi"; }"#).unwrap();
        assert!(spans.iter().any(|s| s.name == "string"), "spans: {spans:?}");
    }

    #[test]
    fn python_highlights() {
        let spans = highlight(Language::Python, "def f():\n    return 1").unwrap();
        assert!(!spans.is_empty());
        assert!(spans.iter().any(|s| s.name == "keyword"));
    }

    #[test]
    fn json_highlights() {
        let spans = highlight(Language::Json, r#"{"a": 1, "b": "two"}"#).unwrap();
        assert!(!spans.is_empty(), "json should produce spans");
    }

    #[test]
    fn spans_are_in_source_order() {
        let spans = highlight(Language::Rust, "fn main() { let y = 2; }").unwrap();
        for pair in spans.windows(2) {
            assert!(
                pair[0].start <= pair[1].start,
                "spans out of order: {spans:?}"
            );
        }
    }

    #[test]
    fn highlight_names_align_with_theme_tokens() {
        // These names must exist as syntax tokens in enzo-theme themes.
        for name in ["keyword", "string", "number", "comment", "function", "type"] {
            assert!(HIGHLIGHT_NAMES.contains(&name));
        }
    }
}
