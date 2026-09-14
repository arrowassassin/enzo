//! The visual constants of the brief: grid, rules, rows, patterns.

use quire_gfx::Pattern;

/// Outer margin.
pub const MARGIN: i32 = 24;
/// Left/right inner padding of list rows.
pub const ROW_PAD: i32 = 32;
/// Edge-label rail height.
pub const RAIL_H: i32 = 40;
/// Running head height (screens).
pub const HEAD_H: i32 = 36;
/// List row.
pub const ROW_H: i32 = 56;
/// List row with a thumbnail.
pub const ROW_THUMB_H: i32 = 88;
/// Large-UI row.
pub const ROW_H_LARGE: i32 = 68;
/// Heavy rule.
pub const RULE_HEAVY: u32 = 4;
/// Standard rule.
pub const RULE: u32 = 2;
/// Hairline.
pub const HAIR: u32 = 1;
/// Focus ring.
pub const FOCUS: u32 = 3;
/// Cover thumbnail.
pub const COVER_W: u32 = 152;
/// Cover thumbnail height.
pub const COVER_H: u32 = 228;
/// The Spine strip width.
pub const SPINE_W: i32 = 12;
/// Tracking for small-cap labels (0.08 em of 18 px).
pub const SMALLCAP_TRACKING: i32 = 1;
/// Disabled content (a 25 % screen: the 50 % one destroys 22 px glyphs).
pub const DISABLED: Pattern = Pattern::Sparse;
/// Secondary surface.
pub const SECONDARY: Pattern = Pattern::Hatch { pitch: 6 };
/// The screen behind an overlay.
pub const SCREENED: Pattern = Pattern::Dots50;
/// Long press threshold, milliseconds.
pub const LONG_PRESS_MS: u32 = 500;
/// Hold-repeat period, milliseconds.
pub const REPEAT_MS: u32 = 200;
