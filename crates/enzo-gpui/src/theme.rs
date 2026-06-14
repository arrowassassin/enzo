//! Enzo palette, fonts, and icon glyphs — the exact values from
//! `design/mockups/*.html`, expressed for GPUI.
//!
//! GPUI styles map almost 1:1 onto the mockups' CSS, so colours here are the
//! literal mockup hex values, type sizes are the literal `px`, and the three
//! embedded font families (Silkscreen pixel labels, JetBrains Mono body/code,
//! Tabler icons) match the `<i class="ti …">` glyphs used in the mockups.

use std::borrow::Cow;

use gpui::{App, Rgba, SharedString};

// ── Palette (exact mockup hex) ──────────────────────────────────────────────

/// Const-friendly hex → `Rgba` (gpui's `rgb()` is not `const`).
#[must_use]
pub const fn rgb_hex(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// Page / deepest background (`#0e0c14`).
pub const BG_PAGE: Rgba = rgb_hex(0x0e0c14);
/// Main content surface (`#16131f`).
pub const BG_SURFACE: Rgba = rgb_hex(0x16131f);
/// Icon dock (`#120f1a`).
pub const BG_DOCK: Rgba = rgb_hex(0x120f1a);
/// Sidebar (`#1a1626`).
pub const BG_SIDE: Rgba = rgb_hex(0x1a1626);
/// Header / status / tab bars (`#221d30`).
pub const BG_BAR: Rgba = rgb_hex(0x221d30);
/// Inset card / input / block background (`#1d1a28`).
pub const BG_CARD: Rgba = rgb_hex(0x1d1a28);
/// Chunky panel border (`#3a3450`).
pub const BORDER: Rgba = rgb_hex(0x3a3450);
/// Subtle row divider (`#221d30`).
pub const DIVIDER: Rgba = rgb_hex(0x221d30);

/// Primary text (`#e8e4f5`).
pub const FG0: Rgba = rgb_hex(0xe8e4f5);
/// Secondary text (`#9f97c4`).
pub const FG1: Rgba = rgb_hex(0x9f97c4);
/// Tertiary / data text (`#c9c4dc`).
pub const FG2: Rgba = rgb_hex(0xc9c4dc);
/// Muted / disabled (`#888780`).
pub const MUTED: Rgba = rgb_hex(0x888780);
/// Faint (`#5f5e6e`).
pub const FAINT: Rgba = rgb_hex(0x5f5e6e);

/// Phosphor teal — the signature accent (`#5dcaa5`).
pub const TEAL: Rgba = rgb_hex(0x5dcaa5);
/// Pixel-label purple, Silkscreen headers (`#7f77dd`).
pub const PURPLE: Rgba = rgb_hex(0x7f77dd);
/// AI text purple (`#afa9ec`).
pub const PURPLE_LT: Rgba = rgb_hex(0xafa9ec);
/// AI / selection fill (`#534ab7`).
pub const PURPLE_BG: Rgba = rgb_hex(0x534ab7);
/// AI badge text (`#eeedfe`).
pub const PURPLE_FG: Rgba = rgb_hex(0xeeedfe);
/// Shell / success green (`#639922`).
pub const GREEN: Rgba = rgb_hex(0x639922);
/// Value / string green (`#97c459`).
pub const GREEN_LT: Rgba = rgb_hex(0x97c459);
/// Dark green ink on green buttons (`#173404`).
pub const GREEN_INK: Rgba = rgb_hex(0x173404);
/// Function / table blue (`#85b7eb`).
pub const BLUE: Rgba = rgb_hex(0x85b7eb);
/// Amber / warning (`#ef9f27`).
pub const AMBER: Rgba = rgb_hex(0xef9f27);
/// Ref-mode / encrypted amber border (`#ba7517`).
pub const AMBER_REF: Rgba = rgb_hex(0xba7517);
/// Amber banner background (`#2a1f12`).
pub const AMBER_BG: Rgba = rgb_hex(0x2a1f12);
/// Danger red (`#e24b4a`).
pub const RED: Rgba = rgb_hex(0xe24b4a);
/// Soft red (diagnostics) (`#f09595`).
pub const RED_LT: Rgba = rgb_hex(0xf09595);

// ── Fonts ───────────────────────────────────────────────────────────────────

/// JetBrains Mono — body and code text.
pub const FONT_MONO: &str = "JetBrains Mono";
/// Silkscreen — 8px uppercase pixel labels / badges / status.
pub const FONT_PIXEL: &str = "Silkscreen";
/// Tabler icons — dock / sidebar glyphs.
pub const FONT_ICON: &str = "tabler-icons";

/// Register the three embedded font families with the text system.
pub fn install_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/JetBrainsMono-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/Silkscreen-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/tabler-icons.ttf").as_slice()),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::error!("failed to register enzo fonts: {e:#}");
    }
}

// ── Tabler icon glyphs (from `@tabler/icons-webfont@3.7.0`) ──────────────────
//
// Each codepoint matches the `<i class="ti ti-…">` used in `design/mockups`.

/// `ti-terminal-2`
pub const ICON_TERMINAL: &str = "\u{ebef}";
/// `ti-code`
pub const ICON_CODE: &str = "\u{ea77}";
/// `ti-world`
pub const ICON_WORLD: &str = "\u{eb54}";
/// `ti-database`
pub const ICON_DATABASE: &str = "\u{ea88}";
/// `ti-robot`
pub const ICON_ROBOT: &str = "\u{f00b}";
/// `ti-settings`
pub const ICON_SETTINGS: &str = "\u{eb20}";
/// `ti-chevron-down`
pub const ICON_CHEVRON_DOWN: &str = "\u{ea5f}";
/// `ti-chevron-right`
pub const ICON_CHEVRON_RIGHT: &str = "\u{ea61}";
/// `ti-table`
pub const ICON_TABLE: &str = "\u{eba1}";
/// `ti-plug-connected`
pub const ICON_PLUG_CONNECTED: &str = "\u{f00a}";
/// `ti-plug`
pub const ICON_PLUG: &str = "\u{ebd9}";
/// `ti-player-play`
pub const ICON_PLAYER_PLAY: &str = "\u{ed46}";
/// `ti-folder`
pub const ICON_FOLDER: &str = "\u{eaad}";
/// `ti-git-branch`
pub const ICON_GIT_BRANCH: &str = "\u{eab2}";
/// `ti-search`
pub const ICON_SEARCH: &str = "\u{eb1c}";
/// `ti-player-play` etc. — convenience to build an icon `SharedString`.
#[must_use]
pub fn icon(glyph: &str) -> SharedString {
    SharedString::from(glyph.to_owned())
}
