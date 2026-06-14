//! Theme data model — the layered token structure parsed from TOML.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A complete theme: metadata + layered tokens.
///
/// Layers (design doc §9): `palette → roles → syntax → fonts → effects`.
/// `palette` is a named-color dictionary; `roles` and `syntax` map semantic
/// names to either a palette key or a literal `#rrggbb[aa]` color.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    /// Identifying metadata.
    pub meta: Meta,
    /// Named base colors (e.g. `bg0 = "#0f1117"`).
    #[serde(default)]
    pub palette: BTreeMap<String, String>,
    /// Semantic UI roles (e.g. `background`, `foreground`, `accent`, `cursor`).
    #[serde(default)]
    pub roles: BTreeMap<String, String>,
    /// Syntax-highlight token colors (e.g. `keyword`, `string`, `comment`).
    #[serde(default)]
    pub syntax: BTreeMap<String, String>,
    /// Font preferences.
    #[serde(default)]
    pub fonts: Fonts,
    /// Optional WGSL effects pipeline configuration.
    #[serde(default)]
    pub effects: Effects,
}

/// Theme metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    /// Human-readable display name.
    pub name: String,
    /// Stable id (kebab-case slug used by `theme.apply`).
    pub id: String,
    /// Author / attribution.
    #[serde(default)]
    pub author: String,
    /// `"dark"` or `"light"`.
    #[serde(default = "default_appearance")]
    pub appearance: String,
    /// `true` if this variant meets WCAG high-contrast requirements.
    #[serde(default)]
    pub high_contrast: bool,
    /// `true` if the palette is colorblind-safe.
    #[serde(default)]
    pub colorblind_safe: bool,
    /// Free-form tags ("8-bit", "modern", "crt", …).
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_appearance() -> String {
    "dark".to_owned()
}

/// Font configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fonts {
    /// Monospace family used for code/terminal text.
    #[serde(default = "default_mono")]
    pub mono: String,
    /// Proportional family used for chrome / UI labels.
    #[serde(default = "default_ui")]
    pub ui: String,
    /// Default code font size in points (×10 to stay integer / `Eq`).
    #[serde(default = "default_size_tenths")]
    pub size_tenths: u16,
}

fn default_mono() -> String {
    "JetBrains Mono".to_owned()
}
fn default_ui() -> String {
    "Inter".to_owned()
}
fn default_size_tenths() -> u16 {
    140 // 14.0pt
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            mono: default_mono(),
            ui: default_ui(),
            size_tenths: default_size_tenths(),
        }
    }
}

/// Effects-pipeline toggles. All default **off** (design doc §9).
///
/// Effects apply to chrome/background only and are force-disabled under
/// reduced-motion / high-contrast by the renderer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each toggle is an independent WGSL effect pass"
)]
pub struct Effects {
    /// CRT scanline overlay.
    #[serde(default)]
    pub scanlines: bool,
    /// Phosphor glow / persistence.
    #[serde(default)]
    pub phosphor: bool,
    /// Screen curvature.
    #[serde(default)]
    pub curvature: bool,
    /// Bloom on bright pixels.
    #[serde(default)]
    pub bloom: bool,
    /// Ordered dithering.
    #[serde(default)]
    pub dither: bool,
}

/// A resolved 8-bit-per-channel RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel (255 = opaque).
    pub a: u8,
}

impl Rgba {
    /// Convert to linear-space `[f32; 4]` for GPU uniforms (0.0–1.0, un-gamma'd).
    #[must_use]
    pub fn to_f32(self) -> [f32; 4] {
        [
            f32::from(self.r) / 255.0,
            f32::from(self.g) / 255.0,
            f32::from(self.b) / 255.0,
            f32::from(self.a) / 255.0,
        ]
    }
}

/// Parse a `#rgb`, `#rrggbb`, or `#rrggbbaa` hex string into [`Rgba`].
///
/// # Errors
/// Returns an error if the string is not a valid hex color.
#[allow(
    clippy::many_single_char_names,
    reason = "r/g/b/a are the conventional colour-channel names"
)]
pub fn parse_hex(s: &str) -> anyhow::Result<Rgba> {
    let h = s.strip_prefix('#').unwrap_or(s);
    let parse2 = |slice: &str| -> anyhow::Result<u8> {
        u8::from_str_radix(slice, 16).map_err(|e| anyhow::anyhow!("bad hex '{slice}': {e}"))
    };
    match h.len() {
        3 => {
            // #rgb → expand each nibble
            let r = parse2(&h[0..1].repeat(2))?;
            let g = parse2(&h[1..2].repeat(2))?;
            let b = parse2(&h[2..3].repeat(2))?;
            Ok(Rgba { r, g, b, a: 255 })
        }
        6 => Ok(Rgba {
            r: parse2(&h[0..2])?,
            g: parse2(&h[2..4])?,
            b: parse2(&h[4..6])?,
            a: 255,
        }),
        8 => Ok(Rgba {
            r: parse2(&h[0..2])?,
            g: parse2(&h[2..4])?,
            b: parse2(&h[4..6])?,
            a: parse2(&h[6..8])?,
        }),
        _ => anyhow::bail!("hex color must be #rgb, #rrggbb, or #rrggbbaa, got '{s}'"),
    }
}

impl Theme {
    /// Parse a theme from a TOML string.
    ///
    /// # Errors
    /// Returns an error if the TOML is malformed or missing required fields.
    pub fn from_toml(src: &str) -> anyhow::Result<Self> {
        toml::from_str(src).map_err(|e| anyhow::anyhow!("parse theme TOML: {e}"))
    }

    /// Serialise the theme back to TOML.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string_pretty(self).map_err(|e| anyhow::anyhow!("serialize theme: {e}"))
    }

    /// Resolve a role name to a concrete `#rrggbb[aa]` color string.
    ///
    /// Roles may reference a palette key (`accent = "blue"`) or hold a literal
    /// hex value (`accent = "#3b82f6"`). Returns `None` if the role is undefined
    /// or its palette reference is dangling.
    #[must_use]
    pub fn role(&self, name: &str) -> Option<String> {
        self.resolve(self.roles.get(name)?)
    }

    /// Resolve a syntax token name to a concrete color string.
    #[must_use]
    pub fn syntax_color(&self, token: &str) -> Option<String> {
        self.resolve(self.syntax.get(token)?)
    }

    /// Resolve a role to parsed [`Rgba`], if defined and valid.
    #[must_use]
    pub fn role_rgba(&self, name: &str) -> Option<Rgba> {
        parse_hex(&self.role(name)?).ok()
    }

    /// Follow a value that may be a palette reference or a literal hex color.
    fn resolve(&self, value: &str) -> Option<String> {
        if value.starts_with('#') {
            Some(value.to_owned())
        } else {
            self.palette.get(value).cloned()
        }
    }

    /// Serialise the fully-resolved theme as a flat JSON object the renderer can
    /// consume directly: `{ roles: {...}, syntax: {...}, effects: {...}, fonts }`.
    #[must_use]
    pub fn to_resolved_json(&self) -> serde_json::Value {
        let roles: BTreeMap<&String, String> = self
            .roles
            .keys()
            .filter_map(|k| self.role(k).map(|v| (k, v)))
            .collect();
        let syntax: BTreeMap<&String, String> = self
            .syntax
            .keys()
            .filter_map(|k| self.syntax_color(k).map(|v| (k, v)))
            .collect();
        serde_json::json!({
            "meta": {
                "id": self.meta.id,
                "name": self.meta.name,
                "appearance": self.meta.appearance,
                "high_contrast": self.meta.high_contrast,
                "colorblind_safe": self.meta.colorblind_safe,
            },
            "roles": roles,
            "syntax": syntax,
            "fonts": {
                "mono": self.fonts.mono,
                "ui": self.fonts.ui,
                "size": f32::from(self.fonts.size_tenths) / 10.0,
            },
            "effects": {
                "scanlines": self.effects.scanlines,
                "phosphor": self.effects.phosphor,
                "curvature": self.effects.curvature,
                "bloom": self.effects.bloom,
                "dither": self.effects.dither,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_rrggbb() {
        let c = parse_hex("#3b82f6").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (0x3b, 0x82, 0xf6, 0xff));
    }

    #[test]
    fn parse_hex_with_alpha() {
        let c = parse_hex("#11223380").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (0x11, 0x22, 0x33, 0x80));
    }

    #[test]
    fn parse_hex_short_form() {
        let c = parse_hex("#0af").unwrap();
        assert_eq!((c.r, c.g, c.b), (0x00, 0xaa, 0xff));
    }

    #[test]
    fn parse_hex_rejects_garbage() {
        assert!(parse_hex("#zz").is_err());
        assert!(parse_hex("nope").is_err());
    }

    #[test]
    fn role_resolves_palette_reference() {
        let toml = r##"
            [meta]
            name = "Test"
            id = "test"
            [palette]
            blue = "#3b82f6"
            [roles]
            accent = "blue"
            cursor = "#ffffff"
        "##;
        let t = Theme::from_toml(toml).unwrap();
        assert_eq!(t.role("accent").as_deref(), Some("#3b82f6"));
        assert_eq!(t.role("cursor").as_deref(), Some("#ffffff"));
        assert_eq!(t.role("missing"), None);
    }

    #[test]
    fn role_rgba_parses() {
        let toml = r##"
            [meta]
            name = "Test"
            id = "test"
            [roles]
            accent = "#3b82f6"
        "##;
        let t = Theme::from_toml(toml).unwrap();
        let rgba = t.role_rgba("accent").unwrap();
        assert_eq!(rgba.r, 0x3b);
    }

    #[test]
    fn roundtrip_toml() {
        let toml = r##"
            [meta]
            name = "Test"
            id = "test"
            [palette]
            blue = "#3b82f6"
            [roles]
            accent = "blue"
        "##;
        let t = Theme::from_toml(toml).unwrap();
        let serialized = t.to_toml().unwrap();
        let t2 = Theme::from_toml(&serialized).unwrap();
        assert_eq!(t, t2);
    }

    #[test]
    fn resolved_json_flattens_references() {
        let toml = r##"
            [meta]
            name = "Test"
            id = "test"
            [palette]
            blue = "#3b82f6"
            [roles]
            accent = "blue"
            [syntax]
            keyword = "blue"
        "##;
        let t = Theme::from_toml(toml).unwrap();
        let j = t.to_resolved_json();
        assert_eq!(j["roles"]["accent"], "#3b82f6");
        assert_eq!(j["syntax"]["keyword"], "#3b82f6");
    }

    #[test]
    fn effects_default_off() {
        let e = Effects::default();
        assert!(!e.scanlines && !e.phosphor && !e.curvature && !e.bloom && !e.dither);
    }

    #[test]
    fn rgba_to_f32_normalises() {
        let c = Rgba {
            r: 255,
            g: 0,
            b: 128,
            a: 255,
        };
        let f = c.to_f32();
        assert!((f[0] - 1.0).abs() < 1e-6);
        assert!((f[1] - 0.0).abs() < 1e-6);
        assert!((f[2] - 128.0 / 255.0).abs() < 1e-6);
    }
}
