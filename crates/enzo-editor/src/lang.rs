//! Language registry — maps file extensions to languages, LSP servers, and
//! formatters.
//!
//! Keeps the editor's language knowledge in one place: which tree-sitter grammar
//! to highlight with, which language server to spawn (design doc §5.2:
//! rust-analyzer, tsserver, pyright), and which external formatter to run.

use serde::{Deserialize, Serialize};

/// A supported source language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Rust.
    Rust,
    /// Python.
    Python,
    /// JavaScript / TypeScript.
    JavaScript,
    /// JSON.
    Json,
    /// Unrecognised / plain text (no highlighting).
    PlainText,
}

/// How to format a given language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatter {
    /// Executable name.
    pub command: &'static str,
    /// Arguments; the file path is appended unless `stdin` is true.
    pub args: &'static [&'static str],
    /// `true` if the formatter reads source on stdin and writes to stdout.
    pub stdin: bool,
}

/// The language server command for a language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServer {
    /// Executable name (e.g. `rust-analyzer`).
    pub command: &'static str,
    /// Launch arguments.
    pub args: &'static [&'static str],
    /// The LSP `languageId` string.
    pub language_id: &'static str,
}

impl Language {
    /// Infer the language from a file path's extension.
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        Self::from_extension(&ext)
    }

    /// Infer the language from a bare extension (no leading dot).
    #[must_use]
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Self::Rust,
            "py" | "pyi" => Self::Python,
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Self::JavaScript,
            "json" | "jsonc" => Self::Json,
            _ => Self::PlainText,
        }
    }

    /// A stable id string (used in ATP payloads).
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::Json => "json",
            Self::PlainText => "plaintext",
        }
    }

    /// The display name.
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Json => "JSON",
            Self::PlainText => "Plain Text",
        }
    }

    /// The recommended language server, if one is configured.
    #[must_use]
    pub fn lsp_server(self) -> Option<LspServer> {
        match self {
            Self::Rust => Some(LspServer {
                command: "rust-analyzer",
                args: &[],
                language_id: "rust",
            }),
            Self::Python => Some(LspServer {
                command: "pyright-langserver",
                args: &["--stdio"],
                language_id: "python",
            }),
            Self::JavaScript => Some(LspServer {
                command: "typescript-language-server",
                args: &["--stdio"],
                language_id: "javascript",
            }),
            Self::Json | Self::PlainText => None,
        }
    }

    /// The recommended formatter, if one is configured.
    #[must_use]
    pub fn formatter(self) -> Option<Formatter> {
        match self {
            Self::Rust => Some(Formatter {
                command: "rustfmt",
                args: &["--edition", "2021"],
                stdin: true,
            }),
            Self::Python => Some(Formatter {
                command: "black",
                args: &["-", "-q"],
                stdin: true,
            }),
            Self::JavaScript => Some(Formatter {
                command: "prettier",
                args: &["--stdin-filepath", "file.js"],
                stdin: true,
            }),
            Self::Json => Some(Formatter {
                command: "prettier",
                args: &["--stdin-filepath", "file.json"],
                stdin: true,
            }),
            Self::PlainText => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_detects_languages() {
        assert_eq!(Language::from_path("src/main.rs"), Language::Rust);
        assert_eq!(Language::from_path("app.py"), Language::Python);
        assert_eq!(Language::from_path("index.tsx"), Language::JavaScript);
        assert_eq!(Language::from_path("data.json"), Language::Json);
        assert_eq!(Language::from_path("README"), Language::PlainText);
        assert_eq!(Language::from_path("notes.md"), Language::PlainText);
    }

    #[test]
    fn extension_case_insensitive() {
        assert_eq!(Language::from_path("MAIN.RS"), Language::Rust);
    }

    #[test]
    fn ids_and_names() {
        assert_eq!(Language::Rust.id(), "rust");
        assert_eq!(Language::JavaScript.display_name(), "JavaScript");
    }

    #[test]
    fn rust_has_lsp_and_formatter() {
        assert_eq!(
            Language::Rust.lsp_server().unwrap().command,
            "rust-analyzer"
        );
        assert_eq!(Language::Rust.formatter().unwrap().command, "rustfmt");
    }

    #[test]
    fn plaintext_has_no_services() {
        assert!(Language::PlainText.lsp_server().is_none());
        assert!(Language::PlainText.formatter().is_none());
    }

    #[test]
    fn json_has_formatter_but_no_lsp() {
        assert!(Language::Json.lsp_server().is_none());
        assert_eq!(Language::Json.formatter().unwrap().command, "prettier");
    }
}
