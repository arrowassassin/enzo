//! 73 clock and timer: 56 px time, date in small caps, a pomodoro ring, visual alarms.
//!
//! The panel is refreshed only when something on it changes: once a minute while the
//! clock is idle, once a second while the timer runs.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{Frame, Ink, Rect, TextStyle};
use quire_library::time;

use crate::text::{draw_centered, draw_label, line_h, small_caps};
use crate::theme::*;
use crate::widgets::{self, rail, running_head, setting_row, RowState, SettingValue};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen, SysRequest};

/// The clock screen.
pub struct Clock {
    /// Pomodoro: (end timestamp, total seconds) while running.
    timer: Option<(u32, u32)>,
    timer_len: u32,
    /// Alarms as minutes of day.
    alarms: Vec<u16>,
    focus: usize,
    /// Set when the timer finished and the screen shows it.
    rang: bool,
    /// Minute of day the clock was last drawn for (`u16::MAX` = never).
    drawn_minute: u16,
}

impl Clock {
    /// New.
    pub fn new() -> Self {
        Clock { timer: None, timer_len: 25 * 60, alarms: Vec::new(), focus: 0, rang: false, drawn_minute: u16::MAX }
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

/// The 2 px stepped ring: `steps` segments around a circle of radius `r`, the first
/// `done` of them solid 2 px arcs, the rest a 2 px dot every 6 px of arc, with a short
/// gap between segments.
fn ring(f: &mut Frame, cx: i32, cy: i32, r: i32, steps: u32, done: u32) {
    let n = steps.max(1);
    let circumference = core::f32::consts::TAU * r as f32;
    // Steps in turns: one pixel of arc for the solid segments, six for the dotted ones.
    let solid_dt = 1.0 / circumference;
    let dotted_dt = 6.0 / circumference;
    for i in 0..n {
        // Each segment spans 80 % of its share of the turn; the rest is the gap.
        let a0 = i as f32 / n as f32;
        let a1 = (i as f32 + 0.8) / n as f32;
        let dt = if i < done { solid_dt } else { dotted_dt };
        let mut t = a0;
        while t < a1 {
            let (s, c) = sin_cos(t * core::f32::consts::TAU);
            let x = cx + (c * r as f32) as i32;
            let y = cy + (s * r as f32) as i32;
            f.fill_rect(Rect::new(x - 1, y - 1, 2, 2), Ink::Black);
            t += dt;
        }
    }
}

/// Sine and cosine without libm: a Taylor series good to ~1e-4 on [-π, π].
fn sin_cos(a: f32) -> (f32, f32) {
    let mut x = a - core::f32::consts::TAU * ((a / core::f32::consts::TAU) as i32) as f32;
    if x > core::f32::consts::PI {
        x -= core::f32::consts::TAU;
    }
    if x < -core::f32::consts::PI {
        x += core::f32::consts::TAU;
    }
    let x2 = x * x;
    let s = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))));
    let c = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0 * (1.0 - x2 / 56.0)));
    (s, c)
}

impl<E: Env> Screen<E> for Clock {
    fn name(&self) -> &'static str {
        "73-clock"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Clock", None);
        let now = cx.env.now();
        self.drawn_minute = time::minute_of_day(now);
        let w = f.width() as i32;
        let hero = quire_fonts::ui::hero();
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP + 20;
        draw_centered(f, hero, w / 2, y + hero.ascent(), &time::fmt_clock(now, cx.settings.clock_24h), TextStyle::INK);
        y += hero.ascent() + hero.below() + 8;
        let day = time::day_of(now);
        draw_centered(f, fl, w / 2, y + fl.ascent(), &small_caps(&time::fmt_weekday_date(day)), crate::text::label_style(false));
        y += line_h(fl) + 28;
        // Pomodoro ring.
        let (left, total) = match self.timer {
            Some((end, total)) => (end.saturating_sub(now), total),
            None => (self.timer_len, self.timer_len),
        };
        let steps = 24;
        let done = if total == 0 { 0 } else { ((total - left) as u64 * steps as u64 / total as u64) as u32 };
        let (cx, cy, radius) = (w / 2, y + 80, 78);
        ring(f, cx, cy, radius, steps, done);
        let poster = quire_fonts::ui::poster();
        let state = if self.rang {
            "Done"
        } else if self.timer.is_some() {
            "Running"
        } else {
            "Timer"
        };
        // The numeral's cap height plus the label line, centred as one block on the ring.
        let cap = poster.glyph('0').map(|g| g.bitmap.h as i32).unwrap_or(poster.ascent() * 7 / 10);
        let gap = 6;
        let block = cap + gap + fl.ascent();
        let top = cy - block / 2;
        draw_centered(f, poster, cx, top + cap, &alloc::format!("{:02}:{:02}", left / 60, left % 60), TextStyle::INK);
        draw_centered(f, fl, cx, top + cap + gap + fl.ascent(), &small_caps(state), crate::text::label_style(false));
        y += 2 * radius + 40;
        // Alarms (visual only: the device shows them when awake).
        draw_label(f, widgets::INSET, y + fl.ascent(), "Alarms", false);
        y += line_h(fl) + 4;
        let rows: Vec<(String, SettingValue)> = {
            let mut v: Vec<(String, SettingValue)> =
                alloc::vec![(String::from("Timer length"), SettingValue::Stepper(alloc::format!("{} min", self.timer_len / 60)))];
            for a in &self.alarms {
                v.push((alloc::format!("{:02}:{:02}", a / 60, a % 60), SettingValue::Text(String::from("visual only"))));
            }
            v.push((String::from("Add alarm"), SettingValue::Nav));
            v
        };
        for (i, (t, v)) in rows.iter().enumerate() {
            if y + ROW_H > f.height() as i32 - RAIL_H {
                break;
            }
            setting_row(f, y, ROW_H, t, v, if self.focus == i { RowState::Focused } else { RowState::Normal });
            y += ROW_H;
        }
        rail(f, ["", "Back", if self.timer.is_some() { "Stop" } else { "Start" }, ""], None);
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        let now = cx.env.now();
        let n = self.alarms.len() + 2;
        match ev.key {
            Key::Back => Action::Pop,
            Key::Up => {
                self.focus = (self.focus + n - 1) % n;
                Action::Redraw
            }
            Key::Down => {
                self.focus = (self.focus + 1) % n;
                Action::Redraw
            }
            Key::Left | Key::Right => {
                let d: i32 = if ev.key == Key::Left { -1 } else { 1 };
                if self.focus == 0 {
                    let mins = (self.timer_len / 60) as i32;
                    let opts: [u32; 9] = [5, 10, 15, 20, 25, 30, 45, 60, 90];
                    let i = opts.iter().position(|o| *o as i32 == mins).unwrap_or(4) as i32;
                    self.timer_len = opts[(i + d).rem_euclid(opts.len() as i32) as usize] * 60;
                } else if self.focus >= 1 && self.focus - 1 < self.alarms.len() {
                    let a = &mut self.alarms[self.focus - 1];
                    *a = ((*a as i32 + 15 * d).rem_euclid(1440)) as u16;
                }
                Action::Redraw
            }
            Key::Confirm => {
                if self.focus == n - 1 {
                    let m = time::minute_of_day(now);
                    self.alarms.push(((m / 60 + 1) * 60) % 1440);
                    return Action::Redraw;
                }
                if self.focus >= 1 && self.focus - 1 < self.alarms.len() {
                    self.alarms.remove(self.focus - 1);
                    return Action::Redraw;
                }
                self.rang = false;
                if self.timer.is_some() {
                    self.timer = None;
                } else {
                    self.timer = Some((now + self.timer_len, self.timer_len));
                    cx.env.request(SysRequest::Timer(1000));
                }
                Action::Redraw
            }
            Key::Power => Action::None,
        }
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Tick | Event::Timer => {
                let now = cx.env.now();
                let running = self.timer.is_some();
                let mut changed = running;
                if let Some((end, _)) = self.timer {
                    if now >= end {
                        self.timer = None;
                        self.rang = true;
                        cx.env.request(SysRequest::RefreshFull);
                        changed = true;
                    }
                }
                let m = time::minute_of_day(now);
                if self.alarms.contains(&m) && now.is_multiple_of(60) && !self.rang {
                    self.rang = true;
                    changed = true;
                }
                // Idle, the clock face changes once a minute.
                if m != self.drawn_minute {
                    changed = true;
                }
                if changed {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}
