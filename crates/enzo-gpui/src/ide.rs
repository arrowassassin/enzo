//! IDE surface — real file tree + `editor.highlight`-backed syntax viewer.
//!
//! Files are read from the local filesystem (rooted at the process cwd) and
//! syntax-highlighted by the daemon's tree-sitter service over ATP. Faithful to
//! `design/mockups/ide.html`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    Context, Entity, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px,
};
use gpui_component::input::{Input, InputState};

use crate::EnzoApp;
use crate::text_input::TextInput;
use crate::theme;
use crate::widgets::{icon, pixel_header, text};

/// Directories never shown in the tree (heavy / noise).
const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".direnv"];

/// Don't read/highlight files larger than this (it's a viewer, not a log tail).
const MAX_FILE_BYTES: u64 = 1_000_000;

/// One visible row in the flattened file tree.
pub struct TreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
}

/// IDE surface state.
pub struct IdeState {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    pub tree: Vec<TreeEntry>,
    pub open_path: Option<PathBuf>,
    pub content: String,
    /// The gpui-component code editor (ropey + tree-sitter + LSP), recreated per
    /// file so the language matches. `None` until a file is opened.
    pub editor: Option<Entity<InputState>>,
    pub language: String,
    pub error: Option<String>,
    /// Git source-control state (from `git.status`/`git.info`).
    pub git_branch: String,
    pub git_entries: Vec<crate::atp::GitEntry>,
}

impl IdeState {
    /// Repository root for git operations (the workspace cwd).
    pub fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
}

impl IdeState {
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut state = Self {
            root: root.clone(),
            expanded: HashSet::new(),
            tree: Vec::new(),
            open_path: None,
            content: String::new(),
            editor: None,
            language: "plaintext".to_owned(),
            error: None,
            git_branch: String::new(),
            git_entries: Vec::new(),
        };
        state.rebuild_tree();
        state
    }

    /// Rebuild the flattened visible tree from `expanded`.
    pub fn rebuild_tree(&mut self) {
        let mut out = Vec::new();
        build_tree(&self.root, 0, &self.expanded, &mut out);
        self.tree = out;
    }

    /// Toggle a directory's expansion.
    pub fn toggle_dir(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_owned());
        }
        self.rebuild_tree();
    }

    /// Read a file's content + detect its language. The caller (which has a
    /// `Window`) builds the editor entity from this.
    pub fn open_file(&mut self, path: &Path) {
        self.language = language_for(path).to_owned();
        self.open_path = Some(path.to_owned());
        // Guard against huge/binary files (avoids a multi-MB read + highlight).
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            self.content.clear();
            self.error = Some("file too large to preview".to_owned());
            return;
        }
        match std::fs::read_to_string(path) {
            Ok(content) => {
                self.content = content;
                self.error = None;
            }
            Err(e) => {
                self.content.clear();
                self.error = Some(e.to_string());
            }
        }
    }
}

/// Walk `dir`, descending into expanded directories, appending visible rows.
fn build_tree(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<TreeEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| {
        let p = e.path();
        (!p.is_dir(), e.file_name().to_string_lossy().to_lowercase())
    });
    for e in entries {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let is_dir = path.is_dir();
        let exp = is_dir && expanded.contains(&path);
        out.push(TreeEntry {
            path: path.clone(),
            name,
            is_dir,
            depth,
            expanded: exp,
        });
        if exp {
            build_tree(&path, depth + 1, expanded, out);
        }
    }
}

/// Language id (matching the daemon's `editor.languages`) for a path.
fn language_for(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx") => "javascript",
        Some("json") => "json",
        _ => "plaintext",
    }
}

// ── Render ──────────────────────────────────────────────────────────────────

/// Explorer file tree + Source Control section.
pub fn sidebar(
    ide: &IdeState,
    commit_input: &Entity<TextInput>,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let mut col = div().flex().flex_col().child(pixel_header("EXPLORER"));
    for entry in &ide.tree {
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        let active = ide.open_path.as_ref() == Some(&entry.path);
        let color = if active { theme::TEAL } else { theme::FG1 };
        let glyph = if is_dir {
            if entry.expanded {
                theme::ICON_CHEVRON_DOWN
            } else {
                theme::ICON_CHEVRON_RIGHT
            }
        } else {
            ""
        };
        let mut row = div()
            .id(SharedString::from(format!("tree-{}", entry.path.display())))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(10.0 + entry.depth as f32 * 14.0))
            .pr(px(10.0))
            .py(px(3.0))
            .text_color(color);
        if is_dir {
            row = row.child(icon(glyph, 11.0, color));
        } else {
            row = row.child(div().w(px(11.0)));
        }
        row = row
            .child(text(&entry.name, 11.0, color))
            .on_click(cx.listener(move |this, _, window, cx| {
                if is_dir {
                    this.ide.toggle_dir(&path);
                } else {
                    this.open_file(&path, window, cx);
                }
                cx.notify();
            }));
        col = col.child(row);
    }
    col.child(source_control(ide, commit_input, cx))
}

/// Git source-control section: branch, changed files (click to stage/unstage),
/// commit message + button.
fn source_control(
    ide: &IdeState,
    commit_input: &Entity<TextInput>,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let header = div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(12.0))
        .pt(px(14.0))
        .pb(px(6.0))
        .child(
            div()
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(theme::PURPLE)
                .child("SOURCE CONTROL"),
        )
        .child(icon(theme::ICON_GIT_BRANCH, 11.0, theme::GREEN_LT))
        .child(text(
            if ide.git_branch.is_empty() {
                "—"
            } else {
                &ide.git_branch
            },
            10.0,
            theme::FG1,
        ));
    let mut col = div().flex().flex_col().child(header);
    if ide.git_entries.is_empty() {
        col = col.child(
            div()
                .pl(px(12.0))
                .child(text("✓ clean", 11.0, theme::FAINT)),
        );
    }
    for e in &ide.git_entries {
        let color = if e.staged {
            theme::GREEN_LT
        } else if e.state == "??" {
            theme::FAINT
        } else {
            theme::AMBER
        };
        let file = e.path.clone();
        let unstage = e.staged;
        col = col.child(
            div()
                .id(SharedString::from(format!("git-{}", e.path)))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(6.0))
                .pl(px(12.0))
                .pr(px(10.0))
                .py(px(2.0))
                .child(
                    div()
                        .w(px(14.0))
                        .text_size(px(9.0))
                        .font_family(theme::FONT_PIXEL)
                        .text_color(color)
                        .child(SharedString::from(if e.staged {
                            "✓".to_owned()
                        } else {
                            e.state.clone()
                        })),
                )
                .child(text(&e.path, 11.0, theme::FG2))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.git_stage(file.clone(), unstage, cx);
                })),
        );
    }
    // Commit message + button.
    col.child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(12.0))
            .pt(px(8.0))
            .child(
                div()
                    .px(px(8.0))
                    .py(px(5.0))
                    .bg(theme::BG_CARD)
                    .border_1()
                    .border_color(theme::BORDER)
                    .rounded(px(4.0))
                    .text_size(px(11.0))
                    .font_family(theme::FONT_MONO)
                    .text_color(theme::FG0)
                    .child(commit_input.clone()),
            )
            .child(
                div()
                    .id("git-commit")
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(px(5.0))
                    .rounded(px(4.0))
                    .bg(theme::GREEN)
                    .text_size(px(8.0))
                    .font_family(theme::FONT_PIXEL)
                    .text_color(theme::GREEN_INK)
                    .child("✓ COMMIT")
                    .on_click(cx.listener(|this, _, _, cx| this.do_git_commit(cx))),
            ),
    )
}

/// Tab bar showing the open file.
pub fn tab_bar(ide: &IdeState) -> impl IntoElement {
    let name = ide
        .open_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map_or_else(
            || "no file".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(12.0))
        .py(px(7.0))
        .bg(theme::BG_BAR)
        .border_b_2()
        .border_color(theme::BORDER)
        .child(
            div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(3.0))
                .bg(theme::BG_SURFACE)
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(theme::TEAL)
                .child(SharedString::from(name)),
        )
}

/// Editor: the gpui-component CodeEditor (ropey + tree-sitter + LSP), or a
/// placeholder / error.
pub fn content(ide: &IdeState) -> impl IntoElement {
    if let Some(err) = &ide.error {
        return div()
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(text(&format!("✗ {err}"), 12.0, theme::RED_LT))
            .into_any_element();
    }
    match &ide.editor {
        Some(editor) => div()
            .size_full()
            .text_size(px(12.5))
            .child(Input::new(editor))
            .into_any_element(),
        None => div()
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(text("⌖ open a file from the explorer", 14.0, theme::FAINT))
            .into_any_element(),
    }
}

/// Status bar: LSP indicator, language, line count.
pub fn status_bar(ide: &IdeState) -> impl IntoElement {
    let cell = |s: String, c: gpui::Rgba| {
        div()
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(c)
            .child(SharedString::from(s))
    };
    let lines = if ide.open_path.is_some() {
        ide.content.split('\n').count()
    } else {
        0
    };
    div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(12.0))
        .py(px(6.0))
        .bg(theme::BG_BAR)
        .border_t_2()
        .border_color(theme::BORDER)
        .child(cell("● tree-sitter".to_owned(), theme::TEAL))
        .child(cell(ide.language.to_uppercase(), theme::FG1))
        .child(
            div()
                .ml_auto()
                .child(cell(format!("{lines} lines · ⌘K"), theme::FAINT)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_detection() {
        assert_eq!(language_for(Path::new("a/b/main.rs")), "rust");
        assert_eq!(language_for(Path::new("x.py")), "python");
        assert_eq!(language_for(Path::new("x.ts")), "javascript");
        assert_eq!(language_for(Path::new("Cargo.toml")), "plaintext");
    }
}
