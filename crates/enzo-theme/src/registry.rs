//! Theme registry — tracks built-in + user themes and the active selection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::builtin::BUILTIN_THEMES;
use crate::model::Theme;

/// A registry of available themes plus the currently-active one.
///
/// Built-in themes are always present. User themes loaded from a directory
/// override built-ins with the same id, enabling community/custom theming
/// (design doc §9). Reloading the user directory implements hot-reload.
pub struct ThemeRegistry {
    themes: BTreeMap<String, Theme>,
    active: String,
    user_dir: Option<PathBuf>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeRegistry {
    /// Create a registry pre-populated with all built-in themes, active = `enzo-dark`.
    #[must_use]
    pub fn new() -> Self {
        let mut themes = BTreeMap::new();
        for (id, src) in BUILTIN_THEMES {
            if let Ok(theme) = Theme::from_toml(src) {
                themes.insert((*id).to_owned(), theme);
            }
        }
        Self {
            themes,
            active: "enzo-dark".to_owned(),
            user_dir: None,
        }
    }

    /// Point the registry at a user theme directory and load its `*.toml` files.
    ///
    /// User themes override built-ins sharing the same id. Missing directories
    /// are tolerated (returns `Ok` with no themes added).
    ///
    /// # Errors
    /// Returns an error only if a present directory cannot be read.
    pub fn with_user_dir(mut self, dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let dir = dir.into();
        self.user_dir = Some(dir.clone());
        self.load_user_dir(&dir)?;
        Ok(self)
    }

    /// Reload all user themes from the configured user directory (hot-reload).
    ///
    /// Built-in themes are preserved; user themes are re-read from disk.
    ///
    /// # Errors
    /// Returns an error if the user directory is set but cannot be read.
    pub fn reload(&mut self) -> anyhow::Result<()> {
        if let Some(dir) = self.user_dir.clone() {
            self.load_user_dir(&dir)?;
        }
        Ok(())
    }

    fn load_user_dir(&mut self, dir: &Path) -> anyhow::Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir)
            .map_err(|e| anyhow::anyhow!("read theme dir {}: {e}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "toml") {
                match std::fs::read_to_string(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|s| Theme::from_toml(&s))
                {
                    Ok(theme) => {
                        self.themes.insert(theme.meta.id.clone(), theme);
                    }
                    Err(e) => {
                        // A broken user theme must not poison the whole registry.
                        eprintln!("enzo-theme: skipping {}: {e:#}", path.display());
                    }
                }
            }
        }
        Ok(())
    }

    /// List all available theme ids, sorted.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.themes.keys().map(String::as_str).collect()
    }

    /// Get a theme by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.themes.get(id)
    }

    /// The id of the active theme.
    #[must_use]
    pub fn active_id(&self) -> &str {
        &self.active
    }

    /// The active theme.
    #[must_use]
    pub fn active(&self) -> &Theme {
        self.themes
            .get(&self.active)
            .or_else(|| self.themes.values().next())
            .expect("registry always has at least one built-in theme")
    }

    /// Set the active theme by id.
    ///
    /// # Errors
    /// Returns an error if no theme with that id exists.
    pub fn set_active(&mut self, id: &str) -> anyhow::Result<()> {
        if self.themes.contains_key(id) {
            id.clone_into(&mut self.active);
            Ok(())
        } else {
            anyhow::bail!("unknown theme '{id}'")
        }
    }

    /// Register or replace a theme at runtime (e.g. from `theme.import`).
    pub fn insert(&mut self, theme: Theme) {
        self.themes.insert(theme.meta.id.clone(), theme);
    }

    /// A JSON summary of every theme for `theme.list`: id, name, appearance, tags.
    #[must_use]
    pub fn list_json(&self) -> serde_json::Value {
        let items: Vec<serde_json::Value> = self
            .themes
            .values()
            .map(|t| {
                serde_json::json!({
                    "id": t.meta.id,
                    "name": t.meta.name,
                    "appearance": t.meta.appearance,
                    "high_contrast": t.meta.high_contrast,
                    "colorblind_safe": t.meta.colorblind_safe,
                    "tags": t.meta.tags,
                    "active": t.meta.id == self.active,
                })
            })
            .collect();
        serde_json::json!({ "themes": items, "active": self.active })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_has_builtins_and_default_active() {
        let r = ThemeRegistry::new();
        assert!(r.ids().contains(&"enzo-dark"));
        assert_eq!(r.active_id(), "enzo-dark");
        assert_eq!(r.active().meta.id, "enzo-dark");
    }

    #[test]
    fn set_active_switches() {
        let mut r = ThemeRegistry::new();
        r.set_active("matrix").unwrap();
        assert_eq!(r.active_id(), "matrix");
        assert_eq!(r.active().meta.name, "Matrix");
    }

    #[test]
    fn set_active_unknown_errors() {
        let mut r = ThemeRegistry::new();
        assert!(r.set_active("nope").is_err());
        assert_eq!(r.active_id(), "enzo-dark");
    }

    #[test]
    fn insert_overrides_and_is_listed() {
        let mut r = ThemeRegistry::new();
        let custom = Theme::from_toml(
            r##"
            [meta]
            name = "Custom"
            id = "custom"
            [roles]
            background = "#000000"
            "##,
        )
        .unwrap();
        r.insert(custom);
        assert!(r.ids().contains(&"custom"));
        r.set_active("custom").unwrap();
        assert_eq!(r.active().meta.name, "Custom");
    }

    #[test]
    fn list_json_marks_active() {
        let r = ThemeRegistry::new();
        let j = r.list_json();
        assert_eq!(j["active"], "enzo-dark");
        let themes = j["themes"].as_array().unwrap();
        let dark = themes.iter().find(|t| t["id"] == "enzo-dark").unwrap();
        assert_eq!(dark["active"], true);
    }

    #[test]
    fn missing_user_dir_is_tolerated() {
        let r = ThemeRegistry::new()
            .with_user_dir("/tmp/enzo-no-such-theme-dir-zzz")
            .unwrap();
        // Still has built-ins.
        assert!(r.ids().contains(&"enzo-dark"));
    }
}
