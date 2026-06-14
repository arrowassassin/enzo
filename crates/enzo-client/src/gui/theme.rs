//! Enzo's visual theme for egui — faithful to the design mockups.
//!
//! Pixel-display labels (Silkscreen) over `JetBrains Mono` body text, the exact
//! purple-tinted palette from `design/mockups/*.html`, chunky 2–3px borders,
//! and gently rounded surfaces. Phosphor teal marks active/hovered elements.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};

use crate::terminal::Color as TermColor;

// ── Palette (exact mockup values) ──────────────────────────────────────────────

/// Page / deepest background.
pub const BG_PAGE: Color32 = c(0x0e, 0x0c, 0x14);
/// Main content surface.
pub const BG_SURFACE: Color32 = c(0x16, 0x13, 0x1f);
/// Icon dock.
pub const BG_DOCK: Color32 = c(0x12, 0x0f, 0x1a);
/// Sidebar.
pub const BG_SIDE: Color32 = c(0x1a, 0x16, 0x26);
/// Header / status / tab bars.
pub const BG_BAR: Color32 = c(0x22, 0x1d, 0x30);
/// Inset card / block background.
pub const BG_CARD: Color32 = c(0x1d, 0x1a, 0x28);
/// Alternate grid row.
pub const BG_ALT: Color32 = c(0x1a, 0x16, 0x26);

/// Chunky panel border.
pub const BORDER: Color32 = c(0x3a, 0x34, 0x50);
/// Subtle row divider.
pub const DIVIDER: Color32 = c(0x22, 0x1d, 0x30);

/// Primary text.
pub const FG0: Color32 = c(0xe8, 0xe4, 0xf5);
/// Secondary text.
pub const FG1: Color32 = c(0x9f, 0x97, 0xc4);
/// Tertiary / data text.
pub const FG2: Color32 = c(0xc9, 0xc4, 0xdc);
/// Muted / disabled.
pub const MUTED: Color32 = c(0x88, 0x87, 0x80);
/// Faint.
pub const FAINT: Color32 = c(0x5f, 0x5e, 0x6e);

/// Phosphor teal — the signature accent.
pub const TEAL: Color32 = c(0x5d, 0xca, 0xa5);
/// Pixel-label purple (Silkscreen headers).
pub const PURPLE: Color32 = c(0x7f, 0x77, 0xdd);
/// AI text purple.
pub const PURPLE_LT: Color32 = c(0xaf, 0xa9, 0xec);
/// AI / selection fill.
pub const PURPLE_BG: Color32 = c(0x53, 0x4a, 0xb7);
/// AI badge text.
pub const PURPLE_FG: Color32 = c(0xee, 0xed, 0xfe);
/// Shell / success green.
pub const GREEN: Color32 = c(0x63, 0x99, 0x22);
/// Value / string green.
pub const GREEN_LT: Color32 = c(0x97, 0xc4, 0x59);
/// Function blue.
pub const BLUE: Color32 = c(0x85, 0xb7, 0xeb);
/// Command-mode blue.
pub const BLUE_CMD: Color32 = c(0x2f, 0x80, 0xb8);
/// Amber / warning.
pub const AMBER: Color32 = c(0xef, 0x9f, 0x27);
/// Ref-mode amber.
pub const AMBER_REF: Color32 = c(0xba, 0x75, 0x17);
/// Danger red.
pub const RED: Color32 = c(0xe2, 0x4b, 0x4a);
/// Soft red (diagnostics).
pub const RED_LT: Color32 = c(0xf0, 0x95, 0x95);
/// Keyword purple (syntax).
pub const KEYWORD: Color32 = c(0xaf, 0xa9, 0xec);
/// Terminal default foreground.
pub const TERM_FG: Color32 = c(0xd6, 0xd2, 0xe6);

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// ── Fonts ───────────────────────────────────────────────────────────────────────

/// `FontFamily::Name` used for Silkscreen pixel labels.
#[must_use]
pub fn pixel_family() -> FontFamily {
    FontFamily::Name("pixel".into())
}

/// A Silkscreen pixel-font `FontId` at `size`.
#[must_use]
pub fn pixel(size: f32) -> FontId {
    FontId::new(size, pixel_family())
}

/// `FontFamily::Name` used for Tabler icon glyphs.
#[must_use]
pub fn icon_family() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// A Tabler icon-font `FontId` at `size`.
#[must_use]
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, icon_family())
}

// ── Tabler icon codepoints (from `@tabler/icons-webfont@3.7.0`) ─────────────────
//
// Each glyph matches the `<i class="ti ti-…">` used in `design/mockups/*.html`.

/// `ti-terminal-2`
pub const ICON_TERMINAL: char = '\u{ebef}';
/// `ti-code`
pub const ICON_CODE: char = '\u{ea77}';
/// `ti-world`
pub const ICON_WORLD: char = '\u{eb54}';
/// `ti-database`
pub const ICON_DATABASE: char = '\u{ea88}';
/// `ti-robot`
pub const ICON_ROBOT: char = '\u{f00b}';
/// `ti-settings`
pub const ICON_SETTINGS: char = '\u{eb20}';
/// `ti-chevron-down`
pub const ICON_CHEVRON_DOWN: char = '\u{ea5f}';
/// `ti-chevron-right`
pub const ICON_CHEVRON_RIGHT: char = '\u{ea61}';
/// `ti-table`
pub const ICON_TABLE: char = '\u{eba1}';
/// `ti-plug-connected`
pub const ICON_PLUG_CONNECTED: char = '\u{f00a}';
/// `ti-plug`
pub const ICON_PLUG: char = '\u{ebd9}';
/// `ti-player-play`
pub const ICON_PLAYER_PLAY: char = '\u{ed46}';
/// `ti-folder`
pub const ICON_FOLDER: char = '\u{eaad}';
/// `ti-git-branch`
pub const ICON_GIT_BRANCH: char = '\u{eab2}';
/// `ti-search`
pub const ICON_SEARCH: char = '\u{eb1c}';
/// `ti-alert-triangle`
pub const ICON_ALERT_TRIANGLE: char = '\u{ea06}';

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "jbmono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "silkscreen".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/Silkscreen-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "tabler".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/tabler-icons.ttf"
        ))),
    );

    // JetBrains Mono is the primary face for both families (keep egui's symbol /
    // emoji fallbacks after it so glyphs like ⚙ ◍ ▤ still resolve).
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jbmono".to_owned());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "jbmono".to_owned());
    fonts.families.insert(
        pixel_family(),
        vec!["silkscreen".to_owned(), "jbmono".to_owned()],
    );
    fonts
        .families
        .insert(icon_family(), vec!["tabler".to_owned()]);

    ctx.set_fonts(fonts);
}

// ── Style installation ─────────────────────────────────────────────────────────

/// Install Enzo's fonts + visuals into `ctx`.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Monospace)),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Monospace)),
        (TextStyle::Small, FontId::new(11.0, FontFamily::Monospace)),
    ]
    .into();

    style.visuals = visuals();
    // Steady (non-blinking) text caret: keeps the UI time-independent, which the
    // headless snapshot tests rely on (a blinking caret would be non-deterministic).
    style.visuals.text_cursor.blink = false;
    style.spacing.item_spacing = egui::vec2(7.0, 5.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.window_margin = egui::Margin::same(0);
    style.spacing.scroll.bar_width = 9.0;
    ctx.set_style(style);
}

fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    let round = CornerRadius::same(4);

    v.dark_mode = true;
    v.override_text_color = Some(FG0);
    v.panel_fill = BG_SURFACE;
    v.window_fill = BG_SURFACE;
    v.extreme_bg_color = BG_PAGE;
    v.faint_bg_color = BG_ALT;
    v.window_stroke = Stroke::new(2.0, BORDER);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = round;
    v.selection.bg_fill = PURPLE_BG;
    v.selection.stroke = Stroke::new(1.0, PURPLE_LT);
    v.hyperlink_color = TEAL;

    v.widgets.noninteractive.bg_fill = BG_SURFACE;
    v.widgets.noninteractive.weak_bg_fill = BG_SURFACE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, FG1);
    v.widgets.noninteractive.corner_radius = round;

    v.widgets.inactive.bg_fill = BG_CARD;
    v.widgets.inactive.weak_bg_fill = BG_BAR;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, FG1);
    v.widgets.inactive.corner_radius = round;

    v.widgets.hovered.bg_fill = BG_BAR;
    v.widgets.hovered.weak_bg_fill = BG_CARD;
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, TEAL);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, FG0);
    v.widgets.hovered.corner_radius = round;

    v.widgets.active.bg_fill = PURPLE_BG;
    v.widgets.active.weak_bg_fill = BG_BAR;
    v.widgets.active.bg_stroke = Stroke::new(1.5, TEAL);
    v.widgets.active.fg_stroke = Stroke::new(2.0, FG0);
    v.widgets.active.corner_radius = round;

    v.widgets.open.bg_fill = BG_BAR;
    v.widgets.open.bg_stroke = Stroke::new(1.5, TEAL);
    v.widgets.open.fg_stroke = Stroke::new(1.5, FG0);
    v.widgets.open.corner_radius = round;

    v
}

// ── Terminal colour mapping ────────────────────────────────────────────────────

/// Map a terminal cell colour to an egui colour for the foreground.
#[must_use]
pub fn term_color(c: TermColor) -> Color32 {
    match c {
        TermColor::Default => TERM_FG,
        TermColor::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        TermColor::Indexed(i) => indexed(i),
    }
}

/// Map an xterm 256-colour index to an egui colour.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "cube/grey ramps are bounded 0..=255"
)]
pub fn indexed(idx: u8) -> Color32 {
    match idx {
        0 => c(0x16, 0x1b, 0x22),
        1 => RED,
        2 => GREEN_LT,
        3 => AMBER,
        4 => BLUE,
        5 => PURPLE_LT,
        6 => TEAL,
        7 => FG1,
        8 => FAINT,
        9 => c(0xff, 0x7b, 0x72),
        10 => c(0x7e, 0xe7, 0x87),
        11 => c(0xf2, 0xcc, 0x60),
        12 => c(0x79, 0xc0, 0xff),
        13 => c(0xd2, 0xa8, 0xff),
        14 => c(0x56, 0xd4, 0xdd),
        15 => FG0,
        16..=231 => {
            let i = idx - 16;
            Color32::from_rgb((i / 36) * 51, ((i / 6) % 6) * 51, (i % 6) * 51)
        }
        232..=255 => {
            let l = 8 + (idx - 232) * 10;
            Color32::from_rgb(l, l, l)
        }
    }
}
