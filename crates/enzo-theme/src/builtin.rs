//! Built-in themes, embedded at compile time.

use crate::model::Theme;

/// All built-in themes as `(id, toml_source)` pairs.
///
/// These are compiled into the binary via `include_str!` so a fresh install
/// ships with a full theme library and never depends on external files.
pub const BUILTIN_THEMES: &[(&str, &str)] = &[
    ("enzo-dark", include_str!("../themes/enzo-dark.toml")),
    ("enzo-light", include_str!("../themes/enzo-light.toml")),
    ("tokyo-night", include_str!("../themes/tokyo-night.toml")),
    ("matrix", include_str!("../themes/matrix.toml")),
    ("gameboy-dmg", include_str!("../themes/gameboy-dmg.toml")),
    ("amber-crt", include_str!("../themes/amber-crt.toml")),
];

/// Return the ids of all built-in themes.
#[must_use]
pub fn builtin_names() -> Vec<&'static str> {
    BUILTIN_THEMES.iter().map(|(id, _)| *id).collect()
}

impl Theme {
    /// Load a built-in theme by id (e.g. `"enzo-dark"`).
    ///
    /// # Errors
    /// Returns an error if no built-in theme has that id, or if (impossibly) the
    /// embedded TOML fails to parse.
    pub fn builtin(id: &str) -> anyhow::Result<Self> {
        let (_, src) = BUILTIN_THEMES
            .iter()
            .find(|(name, _)| *name == id)
            .ok_or_else(|| anyhow::anyhow!("no built-in theme '{id}'"))?;
        Self::from_toml(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_parse() {
        for (id, _) in BUILTIN_THEMES {
            let theme = Theme::builtin(id).unwrap_or_else(|e| panic!("theme {id}: {e}"));
            assert_eq!(theme.meta.id, *id, "id mismatch in {id}");
        }
    }

    #[test]
    fn every_builtin_defines_core_roles() {
        for (id, _) in BUILTIN_THEMES {
            let t = Theme::builtin(id).unwrap();
            for role in ["background", "foreground", "accent", "cursor"] {
                assert!(t.role(role).is_some(), "theme {id} missing role {role}");
            }
        }
    }

    #[test]
    fn every_builtin_role_resolves_to_valid_hex() {
        for (id, _) in BUILTIN_THEMES {
            let t = Theme::builtin(id).unwrap();
            for name in t.roles.keys() {
                let resolved = t
                    .role(name)
                    .unwrap_or_else(|| panic!("theme {id} role {name} did not resolve"));
                crate::model::parse_hex(&resolved)
                    .unwrap_or_else(|e| panic!("theme {id} role {name}: {e}"));
            }
        }
    }

    #[test]
    fn builtin_unknown_errors() {
        assert!(Theme::builtin("does-not-exist").is_err());
    }

    #[test]
    fn builtin_names_nonempty() {
        assert!(builtin_names().contains(&"enzo-dark"));
        assert!(builtin_names().len() >= 6);
    }
}
