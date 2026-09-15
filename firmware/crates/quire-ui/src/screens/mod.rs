//! Every screen of the reader, named after the design artboards.

pub mod apps;
pub mod bookinfo;
pub mod bookshop;
pub mod boot;
pub mod contents;
pub mod cursor;
pub mod dictionary;
pub mod drop;
pub mod endofbook;
pub mod firstrun;
pub mod folders;
pub mod games;
pub mod goto;
pub mod highlights;
pub mod jump;
pub mod library;
pub mod locked;
pub mod power;
pub mod reading;
pub mod settings;
pub mod sleep;
pub mod stats;
pub mod typeset;
pub mod wifi;

use alloc::boxed::Box;
use alloc::string::String;
use quire_gfx::Frame;

use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

/// A two-action dialog card over the screen beneath.
pub struct Dialog {
    title: String,
    body: String,
    cancel: String,
    confirm: String,
    /// What to hand the parent when confirmed.
    on_confirm: Result_,
}

impl Dialog {
    /// A dialog whose confirmation pops with `Result_::Choice(1)` (cancel gives `Cancel`).
    pub fn new(title: &str, body: &str, cancel: &str, confirm: &str) -> Box<Self> {
        Box::new(Dialog {
            title: title.into(),
            body: body.into(),
            cancel: cancel.into(),
            confirm: confirm.into(),
            on_confirm: Result_::Choice(1),
        })
    }
    /// A dialog that hands back a specific result on confirm.
    pub fn with_result(mut self: Box<Self>, r: Result_) -> Box<Self> {
        self.on_confirm = r;
        self
    }
}

impl<E: Env> Screen<E> for Dialog {
    fn name(&self) -> &'static str {
        "11-dialog"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        crate::widgets::screen(f, f.bounds());
        crate::widgets::dialog(f, &self.title, &self.body, &self.cancel, &self.confirm);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::PopWith(Result_::Cancel),
            Key::Confirm => Action::PopWith(self.on_confirm.clone()),
            _ => Action::None,
        }
    }
}

/// A working card: title, subtitle, progress; pops itself when told to.
pub struct Working {
    title: String,
    subtitle: String,
    permille: u32,
    status: String,
}

impl Working {
    /// New.
    pub fn new(title: &str, subtitle: &str, status: &str) -> Box<Self> {
        Box::new(Working { title: title.into(), subtitle: subtitle.into(), permille: 0, status: status.into() })
    }
    /// Update progress.
    pub fn set(&mut self, permille: u32, status: &str) {
        self.permille = permille;
        self.status = status.into();
    }
}

impl<E: Env> Screen<E> for Working {
    fn name(&self) -> &'static str {
        "12-working"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        crate::widgets::screen(f, f.bounds());
        crate::widgets::working_card(f, &self.title, &self.subtitle, self.permille, &self.status);
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.is(Key::Back) {
            Action::PopWith(Result_::Cancel)
        } else {
            Action::None
        }
    }
}

/// Format "38 min left in this chapter" style lines.
pub fn left_line(secs: u32, what: &str) -> String {
    if secs == 0 {
        return alloc::format!("Last page of {what}");
    }
    alloc::format!("{} left in {what}", quire_library::time::fmt_duration(secs))
}

/// "Finish by Thursday" / "Finish by 2 Oct" / "Finish today".
pub fn finish_by_line(today: u16, finish_day: u16) -> String {
    alloc::format!("Finish by {}", finish_word(today, finish_day))
}

/// The word for a finish day: "today", "tomorrow", a weekday within the week, else a date.
pub fn finish_word(today: u16, day: u16) -> String {
    let d = day.saturating_sub(today);
    match d {
        0 => String::from("today"),
        1 => String::from("tomorrow"),
        2..=6 => String::from(quire_library::time::weekday_name_long(quire_library::time::weekday(day))),
        _ => quire_library::time::fmt_date(day),
    }
}

/// Short weekday for poster tiles: "Thu".
pub fn finish_short(today: u16, day: u16) -> String {
    let d = day.saturating_sub(today);
    match d {
        0 => String::from("Today"),
        1 => String::from("Tmrw"),
        2..=6 => String::from(quire_library::time::weekday_name(quire_library::time::weekday(day))),
        _ => quire_library::time::fmt_date(day),
    }
}
