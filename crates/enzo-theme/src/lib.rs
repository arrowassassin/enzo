//! Enzo theme system.
//!
//! A theme is **pure data** (TOML) — hot-reloadable, sandbox-safe, shareable.
//! Layered tokens following the design doc §9: `palette → roles → syntax →
//! fonts → effects`. The effects pipeline (scanlines, phosphor, etc.) applies
//! to chrome/background only; code text always stays crisp.
//!
//! # Example
//! ```
//! use enzo_theme::Theme;
//! let theme = Theme::builtin("enzo-dark").unwrap();
//! assert_eq!(theme.meta.name, "Enzo Dark");
//! // Roles resolve to concrete colors.
//! let bg = theme.role("background").unwrap();
//! assert!(bg.starts_with('#'));
//! ```

mod builtin;
mod model;
mod registry;

pub use builtin::{BUILTIN_THEMES, builtin_names};
pub use model::{Effects, Fonts, Meta, Rgba, Theme, parse_hex};
pub use registry::ThemeRegistry;
