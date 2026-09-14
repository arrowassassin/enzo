//! 41 the Power menu on the compass layout.

use alloc::boxed::Box;
use quire_gfx::Frame;

use crate::screens::reading::draw_compass;
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen, SysRequest, WifiState};

/// The power menu.
pub struct PowerMenu;

impl PowerMenu {
    /// New.
    pub fn new() -> Self {
        PowerMenu
    }
}

impl Default for PowerMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for PowerMenu {
    fn name(&self) -> &'static str {
        "41-power"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let b = cx.env.battery();
        let days = b.days_left.map(|d| alloc::format!(" · {d} days left")).unwrap_or_default();
        let context = alloc::format!("Power · {}%{days}", b.percent);
        let wifi = match cx.env.wifi() {
            WifiState::Off => "Wi-Fi on",
            _ => "Wi-Fi off",
        };
        let pending = cx.lib.is_dirty() || !cx.ingesting.is_empty();
        let hint =
            if pending { "hold Confirm — power off · writes pending" } else { "hold Confirm — power off · Safe to remove card" };
        draw_compass(
            f,
            &context,
            hint,
            [("Sleep", "wake: pwr"), ("Close", "—"), ("Refresh", "full GC"), ("Restart", "—")],
            wifi,
            "Lock keys",
            None,
        );
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => Action::Pop,
            (Key::Left, KeyKind::Press) => Action::Replace(Box::new(super::sleep::SleepScreen::new())),
            (Key::Confirm, KeyKind::Press) => Action::System(SysRequest::RefreshFull),
            (Key::Confirm, KeyKind::Long) => Action::Push(super::Dialog::new(
                "Power off?",
                "The reader turns fully off. Hold Power to start it again.",
                "Cancel",
                "Power off",
            )),
            (Key::Right, KeyKind::Press) => Action::Push(
                super::Dialog::new("Restart?", "Your page is saved.", "Cancel", "Restart").with_result(crate::Result_::Choice(2)),
            ),
            (Key::Up, KeyKind::Press) => match cx.env.wifi() {
                WifiState::Off => Action::System(SysRequest::WifiOn),
                _ => Action::System(SysRequest::WifiOff),
            },
            (Key::Down, KeyKind::Press) => Action::System(SysRequest::LockKeys(true)),
            _ => Action::None,
        }
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: crate::Result_) -> Action<E> {
        match r {
            crate::Result_::Choice(1) => Action::System(SysRequest::PowerOff),
            crate::Result_::Choice(2) => Action::System(SysRequest::Restart),
            _ => Action::Redraw,
        }
    }
}
