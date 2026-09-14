//! 75 image viewer: photos on the card, full-bleed and dithered; Confirm toggles fit.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::image::{Fit, ImageKind};
use quire_fs::Fs;
use quire_gfx::{BlitMode, Frame, Ink};

use crate::widgets::{self, empty_state, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

const FOLDERS: [&str; 3] = ["/images", "/Pictures", "/sleep"];

/// The image viewer.
pub struct Images {
    files: Vec<String>,
    nav: ListNav,
    /// Index being viewed.
    viewing: Option<usize>,
    fill: bool,
    cache: Option<(usize, bool, quire_gfx::Bitmap)>,
    loaded: bool,
}

impl Images {
    /// New.
    pub fn new() -> Self {
        Images { files: Vec::new(), nav: ListNav::new(0, 9), viewing: None, fill: false, cache: None, loaded: false }
    }
    fn load<E: Env>(&mut self, cx: &Ctx<E>) {
        let fs = cx.env.fs();
        self.files.clear();
        for d in FOLDERS {
            for e in fs.read_dir(d).unwrap_or_default() {
                let l = e.name.to_ascii_lowercase();
                if !e.is_dir
                    && (l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png") || l.ends_with(".bmp") || l.ends_with(".pbm"))
                {
                    self.files.push(quire_fs::join(d, &e.name));
                }
            }
        }
        self.files.sort();
        self.nav.set_n(self.files.len());
        self.loaded = true;
    }
    fn bitmap<E: Env>(&mut self, cx: &Ctx<E>, i: usize) -> Option<quire_gfx::Bitmap> {
        if let Some((ci, cf, bm)) = &self.cache {
            if *ci == i && *cf == self.fill {
                return Some(bm.clone());
            }
        }
        let path = self.files.get(i)?.clone();
        let fs = cx.env.fs();
        let bm = if path.ends_with(".pbm") {
            quire_library::cache::load_pbm(fs, &path)?
        } else {
            let file = fs.open(&path).ok()?;
            let fit = if self.fill {
                Fit::fill(quire_gfx::PANEL_W, quire_gfx::PANEL_H)
            } else {
                Fit::inside(quire_gfx::PANEL_W, quire_gfx::PANEL_H)
            };
            quire_doc::image::decode(&file, ImageKind::from_hint(&path), fit).ok()?
        };
        self.cache = Some((i, self.fill, bm.clone()));
        Some(bm)
    }
}

impl Default for Images {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Images {
    fn name(&self) -> &'static str {
        "75-images"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            self.load(cx);
        }
        if let Some(i) = self.viewing {
            f.clear(Ink::White);
            match self.bitmap(cx, i) {
                Some(bm) => {
                    let ox = (f.width() as i32 - bm.w as i32) / 2;
                    let oy = (f.height() as i32 - bm.h as i32) / 2;
                    f.blit(ox, oy, bm.as_ref(), BlitMode::Or);
                }
                None => empty_state(f, 300, "Couldn't open this image", "Baseline JPEG, PNG and BMP are supported."),
            }
            return Refresh::Gc;
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - crate::theme::RAIL_H, row_h);
        running_head(f, "Images", Some(&crate::text::page_indicator(self.nav.page(), self.nav.pages())));
        if self.files.is_empty() {
            empty_state(f, 240, "No images yet", "Drop photos into /images on the card, or the Drop page's Sleep images.");
        }
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let name = quire_fs::file_name(&self.files[i]);
            row(
                f,
                y,
                row_h,
                name,
                Some(quire_fs::parent(&self.files[i])),
                None,
                if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
            );
            y += row_h;
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        if let Some(i) = self.viewing {
            let n = self.files.len().max(1);
            return match ev.key {
                Key::Back => {
                    self.viewing = None;
                    Action::Redraw
                }
                Key::Right | Key::Down => {
                    self.viewing = Some((i + 1) % n);
                    Action::Redraw
                }
                Key::Left | Key::Up => {
                    self.viewing = Some((i + n - 1) % n);
                    Action::Redraw
                }
                Key::Confirm => {
                    self.fill = !self.fill;
                    Action::Redraw
                }
                Key::Power => Action::None,
            };
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Confirm) && !self.files.is_empty() {
            self.viewing = Some(self.nav.focus);
            return Action::Redraw;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

/// Unused import guard.
#[allow(dead_code)]
fn _s() -> String {
    String::new()
}
