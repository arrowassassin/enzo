//! 45 keys locked: a one-refresh strip over the page.

use quire_gfx::Frame;

use crate::{Action, Ctx, Env, Event, KeyEvent, Refresh, Screen};

/// The locked strip; pops itself on the next event.
pub struct LockedStrip {
    shown: bool,
}

impl LockedStrip {
    /// New.
    pub fn new() -> Self {
        LockedStrip { shown: false }
    }
}

impl Default for LockedStrip {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for LockedStrip {
    fn name(&self) -> &'static str {
        "45-locked"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        self.shown = true;
        super::reading::draw_locked_strip(f);
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, _ev: KeyEvent) -> Action<E> {
        Action::Redraw
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Tick | Event::Timer) && self.shown {
            return Action::Pop;
        }
        Action::None
    }
}
