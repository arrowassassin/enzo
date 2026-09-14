//! The seven keys: two ADC resistor ladders (GPIO1: Back/Confirm/Left/Right, GPIO2:
//! Up/Down) plus the Power GPIO, decoded into `quire_ui::KeyEvent`s with debouncing,
//! long-press, repeat and release timing (brief §2). Pure logic: the HAL feeds samples.

use quire_ui::{Key, KeyEvent, KeyKind};

/// Ladder levels in millivolts (02-hardware.md §3); the ladders idle near full scale.
pub mod levels {
    /// Group 1 levels: (key, mV).
    pub const GROUP1: [(super::Key, u16); 4] =
        [(super::Key::Back, 3512), (super::Key::Confirm, 2694), (super::Key::Left, 1493), (super::Key::Right, 5)];
    /// Group 2 levels.
    pub const GROUP2: [(super::Key, u16); 2] = [(super::Key::Up, 2242), (super::Key::Down, 5)];
    /// Above this the ladder is idle (no key).
    pub const IDLE_ABOVE: u16 = 3850;
}

/// Calibrated ladder levels (a unit can store its own from the calibration screen).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ladders {
    /// Group 1 levels.
    pub group1: [(Key, u16); 4],
    /// Group 2 levels.
    pub group2: [(Key, u16); 2],
    /// Idle threshold.
    pub idle_above: u16,
}

impl Default for Ladders {
    fn default() -> Self {
        Ladders { group1: levels::GROUP1, group2: levels::GROUP2, idle_above: levels::IDLE_ABOVE }
    }
}

impl Ladders {
    /// Decode one group's reading: the level nearest the sample, or none when idle or
    /// farther than half a band from every level.
    pub fn decode(levels: &[(Key, u16)], idle_above: u16, mv: u16) -> Option<Key> {
        if mv >= idle_above {
            return None;
        }
        let mut best: Option<(Key, u16)> = None;
        for (k, lv) in levels {
            let d = lv.abs_diff(mv);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((*k, d));
            }
        }
        let (k, d) = best?;
        // Bands are at least 1100 mV apart; accept within 550 mV.
        (d <= 550).then_some(k)
    }
    /// Decode group 1.
    pub fn group1(&self, mv: u16) -> Option<Key> {
        Self::decode(&self.group1, self.idle_above, mv)
    }
    /// Decode group 2.
    pub fn group2(&self, mv: u16) -> Option<Key> {
        Self::decode(&self.group2, self.idle_above, mv)
    }
}

/// Long-press threshold.
pub const LONG_MS: u32 = 500;
/// Repeat period after a long press.
pub const REPEAT_MS: u32 = 200;
/// Samples that must agree before a change is believed (10 ms sampling → 20 ms).
pub const DEBOUNCE_SAMPLES: u8 = 2;

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    /// Debounced key held on this input.
    held: Option<Key>,
    /// Candidate from the latest samples and how many agreed.
    candidate: Option<Key>,
    agree: u8,
    /// When the held key went down.
    down_at: u32,
    /// Long press already fired.
    long: bool,
    /// Last repeat time.
    last_repeat: u32,
}

/// The key state machine for all three inputs.
#[derive(Clone, Debug, Default)]
pub struct KeyMachine {
    slots: [Slot; 3],
    /// Ladder calibration.
    pub ladders: Ladders,
}

impl KeyMachine {
    /// New with default calibration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one sample set (group1 mV, group2 mV, power pressed) at `now_ms`; returns up
    /// to three events (one per input) in a small buffer.
    pub fn sample(&mut self, g1_mv: u16, g2_mv: u16, power: bool, now_ms: u32) -> heapless::Vec<KeyEvent, 6> {
        let mut out = heapless::Vec::new();
        let reads = [self.ladders.group1(g1_mv), self.ladders.group2(g2_mv), power.then_some(Key::Power)];
        for (slot, read) in self.slots.iter_mut().zip(reads) {
            if read != slot.candidate {
                slot.candidate = read;
                slot.agree = 1;
            } else if slot.agree < DEBOUNCE_SAMPLES {
                slot.agree += 1;
            }
            let believed = if slot.agree >= DEBOUNCE_SAMPLES { slot.candidate } else { slot.held };
            match (slot.held, believed) {
                (None, Some(k)) => {
                    slot.held = Some(k);
                    slot.down_at = now_ms;
                    slot.long = false;
                }
                (Some(k), None) => {
                    slot.held = None;
                    let _ = out.push(if slot.long { KeyEvent { key: k, kind: KeyKind::Release } } else { KeyEvent::press(k) });
                }
                (Some(k), Some(k2)) if k != k2 => {
                    // Slid from one ladder key to another: release the first, start the second.
                    let _ = out.push(if slot.long { KeyEvent { key: k, kind: KeyKind::Release } } else { KeyEvent::press(k) });
                    slot.held = Some(k2);
                    slot.down_at = now_ms;
                    slot.long = false;
                }
                (Some(k), Some(_)) => {
                    let held_ms = now_ms.wrapping_sub(slot.down_at);
                    if !slot.long && held_ms >= LONG_MS {
                        slot.long = true;
                        slot.last_repeat = now_ms;
                        let _ = out.push(KeyEvent::long(k));
                    } else if slot.long && now_ms.wrapping_sub(slot.last_repeat) >= REPEAT_MS {
                        slot.last_repeat = now_ms;
                        let _ = out.push(KeyEvent { key: k, kind: KeyKind::Repeat });
                    }
                }
                (None, None) => {}
            }
        }
        out
    }

    /// Whether any key is currently held (keeps the device out of light sleep).
    pub fn any_held(&self) -> bool {
        self.slots.iter().any(|s| s.held.is_some())
    }

    /// Whether Power is held right now.
    pub fn power_held(&self) -> bool {
        self.slots[2].held == Some(Key::Power)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    fn run(m: &mut KeyMachine, samples: &[(u16, u16, bool)], step: u32) -> Vec<KeyEvent> {
        let mut out = Vec::new();
        for (i, (a, b, p)) in samples.iter().enumerate() {
            out.extend(m.sample(*a, *b, *p, i as u32 * step));
        }
        out
    }

    #[test]
    fn decodes_ladders() {
        let l = Ladders::default();
        assert_eq!(l.group1(3500), Some(Key::Back));
        assert_eq!(l.group1(2700), Some(Key::Confirm));
        assert_eq!(l.group1(1400), Some(Key::Left));
        assert_eq!(l.group1(20), Some(Key::Right));
        assert_eq!(l.group1(4095), None);
        assert_eq!(l.group2(2200), Some(Key::Up));
        assert_eq!(l.group2(0), Some(Key::Down));
        assert_eq!(l.group2(3300), Some(Key::Up).filter(|_| false));
    }

    #[test]
    fn short_press_needs_debounce() {
        let mut m = KeyMachine::new();
        // One noisy sample does nothing; two agreeing samples press, two idle release.
        let ev = run(&mut m, &[(2694, 4095, false), (4095, 4095, false), (4095, 4095, false)], 10);
        assert!(ev.is_empty());
        let ev = run(&mut m, &[(2694, 4095, false), (2694, 4095, false), (4095, 4095, false), (4095, 4095, false)], 10);
        assert_eq!(ev, [KeyEvent::press(Key::Confirm)]);
    }

    #[test]
    fn long_press_repeats_and_releases() {
        let mut m = KeyMachine::new();
        let mut samples: Vec<(u16, u16, bool)> = std::iter::repeat_n((5, 4095, false), 100).collect(); // 1 s held
        samples.extend([(4095, 4095, false), (4095, 4095, false)]);
        let ev = run(&mut m, &samples, 10);
        assert_eq!(ev[0], KeyEvent::long(Key::Right));
        let repeats = ev.iter().filter(|e| e.kind == KeyKind::Repeat).count();
        assert!((2..=3).contains(&repeats), "{repeats} repeats in 500 ms");
        assert_eq!(*ev.last().unwrap(), KeyEvent { key: Key::Right, kind: KeyKind::Release });
        assert!(!ev.iter().any(|e| e.kind == KeyKind::Press));
    }

    #[test]
    fn power_and_side_keys_are_independent() {
        let mut m = KeyMachine::new();
        let ev = run(&mut m, &[(4095, 2242, true), (4095, 2242, true), (4095, 4095, false), (4095, 4095, false)], 10);
        assert!(ev.contains(&KeyEvent::press(Key::Up)));
        assert!(ev.contains(&KeyEvent::press(Key::Power)));
    }
}
