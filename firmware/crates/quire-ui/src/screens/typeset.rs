//! 25 type and layout: a live specimen, font rows, size stepper, and everything the text
//! block obeys.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fonts::Family;
use quire_gfx::{Frame, Ink, Rect};
use quire_layout::{Align, Lang, ParaStyle};

use crate::theme::*;
use crate::widgets::{self, rail, running_head, setting_row, ListNav, RowState, SettingValue};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

const SPECIMEN: &str = "Miss Brooke had that kind of beauty which seems to be thrown into relief by poor dress. Her hand and wrist were so finely formed that she could wear sleeves not less bare of style than those in which the Blessed Virgin appeared to Italian painters.";

/// Draw the specimen paragraph in the current profile inside `r`.
pub fn draw_specimen(f: &mut Frame, r: Rect, profile: &quire_layout::Profile) {
    let mut w = quire_qtx::Writer::new();
    w.para(quire_qtx::ParaKind::Body);
    w.text(SPECIMEN);
    let data = w.finish();
    let mut p = *profile;
    p.drop_caps = false;
    p.margin = 0;
    let mut geom = p.geometry(r.w, r.h);
    geom.text = Rect::new(0, 0, r.w, r.h);
    let pg = quire_layout::Paginator::new(&data, p, geom);
    if let Some(page) = pg.page_from(quire_layout::Pos::START) {
        let mut tmp = Frame::new(r.w, r.h);
        quire_layout::render_page(&page, &mut tmp, &p, &quire_layout::NoImages);
        f.blit(r.x, r.y, tmp.as_bitmap(), quire_gfx::BlitMode::Or);
    }
}

/// The Type screen.
pub struct TypeScreen {
    nav: ListNav,
}

impl TypeScreen {
    /// New.
    pub fn new() -> Self {
        TypeScreen { nav: ListNav::new(6, 6) }
    }
}

impl Default for TypeScreen {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_profile<E: Env>(cx: &mut Ctx<E>) {
    let fs = cx.env.fs();
    let settings = &*cx.settings;
    if let Some(r) = cx.reader.as_mut() {
        r.set_profile(fs, settings);
    }
}

impl<E: Env> Screen<E> for TypeScreen {
    fn name(&self) -> &'static str {
        "25-type"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Type", None);
        let w = f.width() as i32;
        let spec = Rect::new(widgets::INSET, widgets::CONTENT_TOP, (w - 2 * widgets::INSET) as u32, 200);
        draw_specimen(f, spec, &cx.settings.profile);
        f.fill_rect(Rect::new(widgets::INSET, spec.bottom() + 8, spec.w, 1), Ink::Black);
        let mut y = spec.bottom() + 16;
        let row_h = cx.settings.row_h();
        let p = &cx.settings.profile;
        let fams = [(Family::Literata, "Literata"), (Family::Atkinson, "Atkinson Hyperlegible"), (Family::Mono, "JetBrains Mono")];
        for (i, (fam, name)) in fams.iter().enumerate() {
            let v = if p.family == *fam { SettingValue::Text(String::from("in use")) } else { SettingValue::Text(String::new()) };
            setting_row(f, y, row_h, name, &v, if self.nav.focus == i { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        setting_row(
            f,
            y,
            row_h,
            "OpenDyslexic",
            &SettingValue::Text(String::from("on SD")),
            if self.nav.focus == 3 { RowState::Focused } else { RowState::Disabled },
        );
        y += row_h;
        setting_row(
            f,
            y,
            row_h,
            "Size",
            &SettingValue::Stepper(alloc::format!("{}", p.size)),
            if self.nav.focus == 4 { RowState::Focused } else { RowState::Normal },
        );
        y += row_h;
        setting_row(
            f,
            y,
            row_h,
            "Darker text",
            &SettingValue::Toggle(p.darker),
            if self.nav.focus == 5 { RowState::Focused } else { RowState::Normal },
        );
        rail(f, ["", "Back", "Change", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        let sizes = quire_fonts::READING_SIZES;
        let idx = sizes.iter().position(|s| *s == cx.settings.profile.size).unwrap_or(3);
        match (self.nav.focus, ev.key) {
            (4, Key::Left) | (4, Key::Right) => {
                let ni = if ev.key == Key::Right { (idx + 1).min(sizes.len() - 1) } else { idx.saturating_sub(1) };
                cx.settings.profile.size = sizes[ni];
                apply_profile(cx);
                return Action::Redraw;
            }
            (_, Key::Up) | (_, Key::Down) => {
                self.nav.key(ev);
                return Action::Redraw;
            }
            (i, Key::Confirm) if i < 3 => {
                cx.settings.profile.family = [Family::Literata, Family::Atkinson, Family::Mono][i];
                apply_profile(cx);
                return Action::Redraw;
            }
            (5, Key::Confirm) => {
                cx.settings.profile.darker = !cx.settings.profile.darker;
                apply_profile(cx);
                return Action::Redraw;
            }
            _ => {}
        }
        Action::None
    }
}

/// One row of the Layout screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutRow {
    LineHeight,
    Margins,
    Alignment,
    Hyphenation,
    ParaStyle,
    DropCaps,
    Language,
    PublisherStyles,
    PublisherFonts,
    RunningHead,
    Spine,
    FullRefresh,
    LargeUi,
    Inverted,
}

const LAYOUT_ROWS: [LayoutRow; 14] = [
    LayoutRow::LineHeight,
    LayoutRow::Margins,
    LayoutRow::Alignment,
    LayoutRow::Hyphenation,
    LayoutRow::ParaStyle,
    LayoutRow::DropCaps,
    LayoutRow::Language,
    LayoutRow::PublisherStyles,
    LayoutRow::PublisherFonts,
    LayoutRow::RunningHead,
    LayoutRow::Spine,
    LayoutRow::FullRefresh,
    LayoutRow::LargeUi,
    LayoutRow::Inverted,
];

/// The Layout screen.
pub struct LayoutScreen {
    nav: ListNav,
}

impl LayoutScreen {
    /// New.
    pub fn new() -> Self {
        LayoutScreen { nav: ListNav::new(LAYOUT_ROWS.len(), 10) }
    }
}

impl Default for LayoutScreen {
    fn default() -> Self {
        Self::new()
    }
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Justify => "Justified",
        Align::Left => "Left",
    }
}
fn para_name(p: ParaStyle) -> &'static str {
    match p {
        ParaStyle::Indent => "Indent",
        ParaStyle::Space => "Space",
    }
}
fn lang_name(l: Lang) -> &'static str {
    match l {
        Lang::English => "English",
        Lang::German => "German",
        Lang::French => "French",
        Lang::Spanish => "Spanish",
        Lang::Italian => "Italian",
        Lang::Dutch => "Dutch",
        Lang::Portuguese => "Portuguese",
        Lang::Russian => "Russian",
        Lang::None => "Off",
    }
}
const LANGS: [Lang; 9] =
    [Lang::English, Lang::German, Lang::French, Lang::Spanish, Lang::Italian, Lang::Dutch, Lang::Portuguese, Lang::Russian, Lang::None];

impl LayoutScreen {
    fn value(&self, cx: &Ctx<impl Env>, row: LayoutRow) -> (&'static str, SettingValue) {
        let s = &*cx.settings;
        let p = &s.profile;
        match row {
            LayoutRow::LineHeight => {
                ("Line height", SettingValue::Stepper(alloc::format!("{}.{:02}", p.line_height_pct / 100, p.line_height_pct % 100)))
            }
            LayoutRow::Margins => {
                ("Margins", SettingValue::Slider(((p.margin as u32 - 16) * 1000 / 32) as u16, alloc::format!("{}", p.margin)))
            }
            LayoutRow::Alignment => ("Alignment", SettingValue::Choice(String::from(align_name(p.align)))),
            LayoutRow::Hyphenation => ("Hyphenation", SettingValue::Toggle(p.hyphenate)),
            LayoutRow::ParaStyle => ("Paragraph style", SettingValue::Choice(String::from(para_name(p.para_style)))),
            LayoutRow::DropCaps => ("Drop caps", SettingValue::Toggle(p.drop_caps)),
            LayoutRow::Language => ("Hyphenation language", SettingValue::Choice(String::from(lang_name(p.lang)))),
            LayoutRow::PublisherStyles => ("Embedded styles", SettingValue::Toggle(s.publisher_styles)),
            LayoutRow::PublisherFonts => ("Publisher fonts", SettingValue::Toggle(s.publisher_fonts)),
            LayoutRow::RunningHead => ("Running head", SettingValue::Toggle(s.running_head)),
            LayoutRow::Spine => ("Spine", SettingValue::Toggle(s.spine)),
            LayoutRow::FullRefresh => ("Full refresh every", SettingValue::Stepper(alloc::format!("{} pages", s.gc_every_pages))),
            LayoutRow::LargeUi => ("Large UI", SettingValue::Toggle(s.large_ui)),
            LayoutRow::Inverted => ("Inverted", SettingValue::Toggle(s.inverted)),
        }
    }
    fn change<E: Env>(&mut self, cx: &mut Ctx<E>, row: LayoutRow, dir: i32) {
        let s = &mut *cx.settings;
        let p = &mut s.profile;
        match row {
            LayoutRow::LineHeight => {
                let opts = [130u16, 145, 160];
                let i = opts.iter().position(|o| *o == p.line_height_pct).unwrap_or(1) as i32;
                p.line_height_pct = opts[(i + dir).rem_euclid(3) as usize];
            }
            LayoutRow::Margins => p.margin = (p.margin as i32 + 8 * dir).clamp(16, 48) as u16,
            LayoutRow::Alignment => p.align = if p.align == Align::Justify { Align::Left } else { Align::Justify },
            LayoutRow::Hyphenation => p.hyphenate = !p.hyphenate,
            LayoutRow::ParaStyle => p.para_style = if p.para_style == ParaStyle::Indent { ParaStyle::Space } else { ParaStyle::Indent },
            LayoutRow::DropCaps => p.drop_caps = !p.drop_caps,
            LayoutRow::Language => {
                let i = LANGS.iter().position(|l| *l == p.lang).unwrap_or(0) as i32;
                p.lang = LANGS[(i + dir).rem_euclid(LANGS.len() as i32) as usize];
            }
            LayoutRow::PublisherStyles => s.publisher_styles = !s.publisher_styles,
            LayoutRow::PublisherFonts => s.publisher_fonts = !s.publisher_fonts,
            LayoutRow::RunningHead => s.running_head = !s.running_head,
            LayoutRow::Spine => s.spine = !s.spine,
            LayoutRow::FullRefresh => s.gc_every_pages = (s.gc_every_pages as i32 + dir).clamp(1, 30) as u8,
            LayoutRow::LargeUi => s.large_ui = !s.large_ui,
            LayoutRow::Inverted => s.inverted = !s.inverted,
        }
        s.profile.running_head = s.running_head;
        s.profile.spine = s.spine;
        apply_profile(cx);
    }
}

impl<E: Env> Screen<E> for LayoutScreen {
    fn name(&self) -> &'static str {
        "25-layout"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let row_h = cx.settings.row_h();
        let per = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        self.nav.per_page = per;
        running_head(f, "Layout", Some(&crate::text::page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        let vals: Vec<(&str, SettingValue)> = LAYOUT_ROWS.iter().map(|r| self.value(cx, *r)).collect();
        for i in self.nav.visible() {
            let (t, v) = &vals[i];
            setting_row(f, y, row_h, t, v, if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["", "Back", "Change", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        let row = LAYOUT_ROWS[self.nav.focus];
        match ev.key {
            Key::Up | Key::Down => {
                self.nav.key(ev);
                Action::Redraw
            }
            Key::Left => {
                self.change(cx, row, -1);
                Action::Redraw
            }
            Key::Right | Key::Confirm => {
                self.change(cx, row, 1);
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}
