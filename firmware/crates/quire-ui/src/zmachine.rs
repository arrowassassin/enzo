//! A Z-machine interpreter for Infocom and Inform story files.
//!
//! The Z-machine is the virtual machine behind Zork, the Infocom catalogue and every
//! Inform 6 game; this module runs versions 3, 4, 5, 7 and 8 of it (the text-only
//! versions — 6 is graphical and 1–2 are museum pieces) in `no_std` with a memory budget
//! sized for the ESP32-C3:
//!
//! * the story file stays on the card (any [`ReadAt`] source — a file, or a `Vec<u8>`
//!   in tests): the game's dynamic memory (header field 0x0e, at most 48 KB) is read
//!   into RAM and mutated in place, while static and high memory are served through a
//!   small cache of 512-byte sectors read on demand. `restart`, `verify` and the
//!   delta-compressed snapshots re-read the original bytes from the source rather than
//!   keeping a second copy;
//! * the evaluation stack is a `Vec<u16>` bounded to 4096 words and the call stack to
//!   256 frames — exceeding either is a [`Step::Error`], never a panic;
//! * output is buffered in a `String` the caller drains with [`Machine::take_output`];
//!   `run` returns early with [`Step::Budget`] once the buffer holds 16 KB so it never
//!   grows without bound. The v5+ upper window (split-screen status) is a small
//!   character grid read with [`Machine::upper_window`]; v3 status lines are computed on
//!   demand with [`Machine::status_line`].
//!
//! Every memory access is bounds-checked and a malformed story (random bytes, a truncated
//! header, jumps outside memory, runaway recursion) ends the run with `Step::Error(msg)`.
//!
//! # Driving the machine
//!
//! ```ignore
//! let mut m = Machine::new(story_file)?; // anything that implements `ReadAt`
//! loop {
//!     match m.run(5_000) {
//!         Step::Budget => { ui.print(&m.take_output()); continue; }
//!         Step::WaitLine => { ui.print(&m.take_output()); m.input(&ui.read_line()); }
//!         Step::WaitChar => { ui.print(&m.take_output()); m.input_char(ui.read_key()); }
//!         Step::Halt => break,
//!         Step::Error(e) => { ui.print(&e); break; }
//!     }
//! }
//! ```
//!
//! The line the player types is echoed (plus a newline) into the output buffer, as the
//! standard requires, so the UI should not echo it a second time.
//!
//! # Saving
//!
//! [`Machine::save`] produces a snapshot (see [`Machine::save`] for the format) and
//! [`Machine::restore`] loads one — at any time, including while the game waits for input.
//! The `save`, `restore`, `save_undo` and `restore_undo` opcodes use the same format:
//! an in-game `save` stores its snapshot in an internal slot the UI fetches with
//! [`Machine::take_save`] (the game sees success); an in-game `restore` succeeds only if
//! the UI has previously handed it a snapshot with [`Machine::offer_restore`], otherwise
//! the game sees failure. Undo is a single internal slot.
//!
//! # Header
//!
//! The interpreter reports itself as interpreter number 6 (IBM PC) version `'Q'`, no
//! colours, no sound, no pictures, no timed input, fixed-pitch font, undo available, and a
//! 40-column by 24-row screen so games format for the small e-ink panel.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use quire_fs::ReadAt;

type R<T> = Result<T, String>;

/// Story files larger than this are rejected before anything is allocated (a v8 story
/// cannot address more anyway).
const MAX_STORY: usize = 512 * 1024;
/// Largest dynamic-memory area kept resident; stories wanting more are refused.
pub const MAX_DYN: usize = 48 * 1024;
/// Size of one cached story sector.
const SECTOR: usize = 512;
/// Sectors of static/high memory held in RAM at once.
const SECTORS: usize = 8;
/// Evaluation stack bound, in words.
const MAX_STACK: usize = 4096;
/// Call stack bound, in frames.
const MAX_FRAMES: usize = 256;
/// Screen width in characters as advertised in the header.
const COLS: usize = 40;
/// Screen height in rows as advertised in the header.
const ROWS: usize = 24;
/// `run` returns `Budget` early once the output buffer holds this much text.
const OUTPUT_FLUSH: usize = 16 * 1024;
/// Nesting bound for memory output streams (the standard says 16).
const MAX_STREAM3: usize = 16;
/// Magic at the front of a snapshot.
const SNAPSHOT_MAGIC: &[u8; 4] = b"QZS1";

const DEFAULT_ALPHABET: [&[u8; 26]; 3] = [b"abcdefghijklmnopqrstuvwxyz", b"ABCDEFGHIJKLMNOPQRSTUVWXYZ", b" ^0123456789.,!?_#'\"/\\-:()"];

/// The default Unicode translation table: ZSCII 155..=223 in order.
const DEFAULT_UNICODE: &str = "äöüÄÖÜß»«ëïÿËÏáéíóúýÁÉÍÓÚÝàèìòùÀÈÌÒÙâêîôûÂÊÎÔÛåÅøØãñõÃÑÕæÆçÇþðÞÐ£œŒ¡¿";

/// What [`Machine::run`] stopped for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Ran out of the instruction budget; call `run` again.
    Budget,
    /// Waiting for a line of input (`input`).
    WaitLine,
    /// Waiting for a single character (`input_char`).
    WaitChar,
    /// The game ended.
    Halt,
    /// An internal error (message).
    Error(String),
}

#[derive(Clone, Copy)]
struct Frame {
    ret_pc: u32,
    /// Variable the routine's result goes to; `None` for `call_*n` forms.
    store: Option<u8>,
    nlocals: u8,
    nargs: u8,
    /// Evaluation stack height when the frame was pushed.
    sp: u16,
    locals: [u16; 15],
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Running,
    WaitLine { text: u16, parse: u16, store: Option<u8> },
    WaitChar { store: u8 },
    Halted,
    Error(String),
}

/// Static and high memory, paged from the story source through a small LRU of sectors.
struct Pager {
    /// Sector number held by each slot (`u32::MAX` = empty).
    tags: [u32; SECTORS],
    /// Last use of each slot, for eviction.
    age: [u32; SECTORS],
    clock: u32,
    /// The slot that served the previous read (checked first).
    last: usize,
    bufs: [[u8; SECTOR]; SECTORS],
}

impl Pager {
    fn new() -> Self {
        Pager { tags: [u32::MAX; SECTORS], age: [0; SECTORS], clock: 0, last: 0, bufs: [[0; SECTOR]; SECTORS] }
    }

    /// Forget every cached sector (the source changed).
    fn clear(&mut self) {
        self.tags = [u32::MAX; SECTORS];
    }

    /// The slot holding sector `sec`, reading it from `src` when needed; `None` on a
    /// read failure.
    fn slot(&mut self, src: &dyn ReadAt, sec: u32) -> Option<usize> {
        self.clock = self.clock.wrapping_add(1);
        if self.tags[self.last] == sec {
            self.age[self.last] = self.clock;
            return Some(self.last);
        }
        let mut victim = 0;
        for i in 0..SECTORS {
            if self.tags[i] == sec {
                self.age[i] = self.clock;
                self.last = i;
                return Some(i);
            }
            if self.age[i] < self.age[victim] {
                victim = i;
            }
        }
        let off = sec as u64 * SECTOR as u64;
        let want = (src.len().saturating_sub(off)).min(SECTOR as u64) as usize;
        if want == 0 {
            return None;
        }
        self.tags[victim] = u32::MAX;
        src.read_exact_at(off, &mut self.bufs[victim][..want]).ok()?;
        self.tags[victim] = sec;
        self.age[victim] = self.clock;
        self.last = victim;
        Some(victim)
    }

    /// The byte at story offset `a` (the caller has checked `a` is inside the file).
    fn byte(&mut self, src: &dyn ReadAt, a: usize) -> Option<u8> {
        let slot = self.slot(src, (a / SECTOR) as u32)?;
        Some(self.bufs[slot][a % SECTOR])
    }
}

/// The state of a running story without its story file: dynamic memory, stacks,
/// screen, output. It is `'static`, so a screen can own it and hand it the open file
/// (any [`ReadAt`]) for each burst of execution with [`Core::attach`]. [`Machine`] pairs
/// a `Core` with a source it owns.
pub struct Core {
    /// Sector cache for static and high memory.
    pager: RefCell<Pager>,
    /// Story file length.
    len: usize,
    /// Dynamic memory (`story[..static_base]`), resident and mutated in place.
    dynmem: Vec<u8>,
    version: u8,
    static_base: usize,
    pc: usize,
    stack: Vec<u16>,
    frames: Vec<Frame>,
    state: State,
    output: String,
    /// Upper window cells, `upper_rows * COLS` of them.
    upper: Vec<char>,
    upper_rows: usize,
    /// Selected window: 0 lower, 1 upper.
    window: u8,
    cursor: (usize, usize),
    screen_on: bool,
    /// Open memory output streams: (table address, characters written so far).
    stream3: Vec<(u16, u16)>,
    cleared: bool,
    font: u16,
    rng: u32,
    rng_pred: u16,
    rng_count: u16,
    ticks: u32,
    pending_save: Option<Vec<u8>>,
    offered_restore: Option<Vec<u8>>,
    undo: Option<Vec<u8>>,
}

fn fault<T>(msg: &str) -> R<T> {
    Err(String::from(msg))
}

fn arg(ops: &[u16], i: usize) -> u16 {
    ops.get(i).copied().unwrap_or(0)
}

fn be16(d: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([d[at], d[at + 1]])
}

/// Map a character typed by the player to ZSCII (ASCII, newline, or the default
/// Unicode table); `None` for anything the Z-machine cannot represent.
fn char_to_zscii(c: char) -> Option<u8> {
    match c {
        '\n' => Some(13),
        ' '..='~' => Some(c as u8),
        _ => DEFAULT_UNICODE.chars().position(|u| u == c).map(|i| 155 + i as u8),
    }
}

/// A [`Core`] attached to its story source for the duration of a call.
pub struct Attached<'a> {
    core: &'a mut Core,
    src: &'a dyn ReadAt,
}

impl core::ops::Deref for Attached<'_> {
    type Target = Core;
    fn deref(&self) -> &Core {
        self.core
    }
}

impl core::ops::DerefMut for Attached<'_> {
    fn deref_mut(&mut self) -> &mut Core {
        self.core
    }
}

/// A running Z-machine story that owns its source (a file, or — the default — a
/// `Vec<u8>` in tests).
pub struct Machine<S: ReadAt = Vec<u8>> {
    core: Core,
    src: S,
}

impl<S: ReadAt> Machine<S> {
    /// Load a story: see [`Core::new`].
    pub fn new(story: S) -> Result<Machine<S>, &'static str> {
        let core = Core::new(&story)?;
        Ok(Machine { core, src: story })
    }
    /// The state attached to the owned source.
    pub fn attached(&mut self) -> Attached<'_> {
        Attached { core: &mut self.core, src: &self.src }
    }
    /// The state without its source.
    pub fn core(&self) -> &Core {
        &self.core
    }
    /// The story file's Z-machine version (3, 4, 5, 7 or 8).
    pub fn version(&self) -> u8 {
        self.core.version
    }
    /// Execute up to `budget` instructions: see [`Attached::run`].
    pub fn run(&mut self, budget: u32) -> Step {
        self.attached().run(budget)
    }
    /// Take buffered output text (the status line is separate).
    pub fn take_output(&mut self) -> String {
        self.core.take_output()
    }
    /// Whether the game cleared the screen since the last call: see [`Core::take_clear`].
    pub fn take_clear(&mut self) -> bool {
        self.core.take_clear()
    }
    /// Provide a line of input after `WaitLine`: see [`Attached::input`].
    pub fn input(&mut self, line: &str) {
        self.attached().input(line)
    }
    /// Provide a character after `WaitChar`: see [`Attached::input_char`].
    pub fn input_char(&mut self, c: u16) {
        self.attached().input_char(c)
    }
    /// The v3 status line: see [`Attached::status_line`].
    pub fn status_line(&mut self) -> Option<(String, i16, i16, bool)> {
        self.attached().status_line()
    }
    /// Text of the upper window, as lines.
    pub fn upper_window(&self) -> Vec<String> {
        self.core.upper_window()
    }
    /// Whether the game has finished (`quit`, or an error).
    pub fn halted(&self) -> bool {
        self.core.halted()
    }
    /// Seed the random number generator.
    pub fn seed(&mut self, seed: u32) {
        self.core.seed(seed)
    }
    /// Take the snapshot written by the game's most recent `save` opcode, if any.
    pub fn take_save(&mut self) -> Option<Vec<u8>> {
        self.core.take_save()
    }
    /// Hand the game a snapshot for its next `restore` opcode.
    pub fn offer_restore(&mut self, data: Vec<u8>) {
        self.core.offer_restore(data)
    }
    /// Restart the story from the beginning: see [`Attached::restart`].
    pub fn restart(&mut self) {
        self.attached().restart()
    }
    /// Snapshot the whole machine state: see [`Attached::save`].
    pub fn save(&mut self) -> Vec<u8> {
        self.attached().save()
    }
    /// Load a snapshot: see [`Attached::restore`].
    pub fn restore(&mut self, data: &[u8]) -> bool {
        self.attached().restore(data)
    }
}

impl Core {
    /// Load a story from `story`. Fails (allocating nothing but the dynamic memory) when
    /// the file is over 512 KB, shorter than a header, an unsupported version, keeps more
    /// than [`MAX_DYN`] of dynamic memory, or has a header whose memory map does not fit
    /// the file.
    pub fn new(story: &dyn ReadAt) -> Result<Core, &'static str> {
        let len = story.len();
        if len > MAX_STORY as u64 {
            return Err("story file larger than 512 KB");
        }
        if len < 64 {
            return Err("story file shorter than a header");
        }
        let len = len as usize;
        let mut header = [0u8; 64];
        if story.read_exact_at(0, &mut header).is_err() {
            return Err("could not read the story file");
        }
        let version = header[0];
        if !matches!(version, 3 | 4 | 5 | 7 | 8) {
            return Err("unsupported Z-machine version");
        }
        let static_base = be16(&header, 0x0e) as usize;
        if static_base < 64 || static_base > len {
            return Err("static memory base outside the file");
        }
        if static_base > MAX_DYN {
            return Err("this story keeps more than 48 KB of dynamic memory, more than this reader can hold");
        }
        if be16(&header, 0x06) as usize >= len {
            return Err("initial program counter outside the file");
        }
        let mut dynmem = alloc::vec![0u8; static_base];
        if story.read_exact_at(0, &mut dynmem).is_err() {
            return Err("could not read the story file");
        }
        let mut m = Core {
            pager: RefCell::new(Pager::new()),
            len,
            dynmem,
            version,
            static_base,
            pc: 0,
            stack: Vec::new(),
            frames: Vec::new(),
            state: State::Running,
            output: String::new(),
            upper: Vec::new(),
            upper_rows: 0,
            window: 0,
            cursor: (0, 0),
            screen_on: true,
            stream3: Vec::new(),
            cleared: false,
            font: 1,
            rng: 0x2545_f491,
            rng_pred: 0,
            rng_count: 0,
            ticks: 0,
            pending_save: None,
            offered_restore: None,
            undo: None,
        };
        m.reset();
        Ok(m)
    }

    /// Attach the story source for a burst of execution. The source must be the same
    /// story the core was loaded from (a re-opened file, typically).
    pub fn attach<'a>(&'a mut self, src: &'a dyn ReadAt) -> Attached<'a> {
        Attached { core: self, src }
    }

    /// Forget cached story sectors (after the source was re-opened, say).
    pub fn drop_cache(&mut self) {
        self.pager.borrow_mut().clear();
    }

    /// The story file's Z-machine version (3, 4, 5, 7 or 8).
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Take buffered output text (the status line is separate).
    pub fn take_output(&mut self) -> String {
        core::mem::take(&mut self.output)
    }

    /// Whether the game asked for the lower window (the whole screen) to be cleared since
    /// the last call; the UI should wipe its transcript view before drawing new output.
    pub fn take_clear(&mut self) -> bool {
        core::mem::replace(&mut self.cleared, false)
    }

    /// Text of the upper window (v5+ split-screen status), if any, as lines.
    pub fn upper_window(&self) -> Vec<String> {
        (0..self.upper_rows)
            .map(|r| {
                let row: String = self.upper[r * COLS..(r + 1) * COLS].iter().collect();
                String::from(row.trim_end())
            })
            .collect()
    }

    /// Whether the game has finished (`quit`, or an error).
    pub fn halted(&self) -> bool {
        matches!(self.state, State::Halted | State::Error(_))
    }

    /// Whether the game is waiting for a line of input.
    pub fn wants_line(&self) -> bool {
        matches!(self.state, State::WaitLine { .. })
    }

    /// Seed the random number generator (for example from a clock at boot); the sequence
    /// is otherwise deterministic from `new`.
    pub fn seed(&mut self, seed: u32) {
        self.rng = if seed == 0 { 0x2545_f491 } else { seed };
        self.rng_pred = 0;
    }

    /// Take the snapshot written by the game's most recent `save` opcode, if any.
    pub fn take_save(&mut self) -> Option<Vec<u8>> {
        self.pending_save.take()
    }

    /// Hand the game a snapshot for its next `restore` opcode to load. Without one, an
    /// in-game `restore` reports failure.
    pub fn offer_restore(&mut self, data: Vec<u8>) {
        self.offered_restore = Some(data);
    }

    fn blocked(&self) -> Option<Step> {
        match &self.state {
            State::Running => None,
            State::WaitLine { .. } => Some(Step::WaitLine),
            State::WaitChar { .. } => Some(Step::WaitChar),
            State::Halted => Some(Step::Halt),
            State::Error(m) => Some(Step::Error(m.clone())),
        }
    }

    fn reset_screen(&mut self) {
        self.upper.clear();
        self.upper_rows = 0;
        self.window = 0;
        self.cursor = (0, 0);
        self.screen_on = true;
        self.stream3.clear();
        self.font = 1;
    }

    fn frame(&self) -> &Frame {
        // `frames` always holds the main frame; `reset` puts it there.
        self.frames.last().expect("frame stack never empty")
    }

    fn push(&mut self, v: u16) -> R<()> {
        if self.stack.len() >= MAX_STACK {
            return fault("stack overflow");
        }
        self.stack.push(v);
        Ok(())
    }

    fn pop(&mut self) -> R<u16> {
        if self.stack.len() <= self.frame().sp as usize {
            return fault("stack underflow");
        }
        Ok(self.stack.pop().unwrap_or(0))
    }

    fn upper_put(&mut self, c: char) {
        let (row, col) = self.cursor;
        if c == '\n' {
            self.cursor = (row + 1, 0);
            return;
        }
        if row < self.upper_rows && col < COLS {
            self.upper[row * COLS + col] = c;
            self.cursor.1 = col + 1;
        }
    }

    fn split_window(&mut self, lines: u16) {
        let rows = (lines as usize).min(ROWS);
        self.upper.resize(rows * COLS, ' ');
        if self.version == 3 {
            self.upper.iter_mut().for_each(|c| *c = ' ');
        }
        self.upper_rows = rows;
        if self.cursor.0 >= rows {
            self.cursor = (0, 0);
        }
    }

    fn erase_window(&mut self, w: u16) {
        match w as i16 {
            -1 => {
                self.reset_screen();
                self.cleared = true;
            }
            -2 => {
                self.upper.iter_mut().for_each(|c| *c = ' ');
                self.cursor = (0, 0);
                self.cleared = true;
            }
            0 => self.cleared = true,
            1 => {
                self.upper.iter_mut().for_each(|c| *c = ' ');
                self.cursor = (0, 0);
            }
            _ => {}
        }
    }

    // ----------------------------------------------------------------------------------
    // Random numbers

    fn random(&mut self, range: i16) -> u16 {
        if range < 0 {
            let s = range.unsigned_abs();
            if s < 1000 {
                self.rng_pred = s;
                self.rng_count = 0;
            } else {
                self.rng_pred = 0;
                self.rng = s as u32;
            }
            return 0;
        }
        if range == 0 {
            self.rng_pred = 0;
            self.rng ^= self.ticks.wrapping_mul(0x9e37_79b9);
            return 0;
        }
        let r = range as u32;
        if self.rng_pred > 0 {
            self.rng_count = if self.rng_count >= self.rng_pred { 1 } else { self.rng_count + 1 };
            return ((self.rng_count as u32 - 1) % r + 1) as u16;
        }
        if self.rng == 0 {
            self.rng = 0x2545_f491;
        }
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        ((x >> 4) % r + 1) as u16
    }

    /// Start from the current dynamic memory: header, stacks, screen.
    fn reset(&mut self) {
        self.setup_header();
        self.stack.clear();
        self.frames.clear();
        self.frames.push(Frame { ret_pc: 0, store: None, nlocals: 0, nargs: 0, sp: 0, locals: [0; 15] });
        self.pc = be16(&self.dynmem, 0x06) as usize;
        self.state = State::Running;
        self.reset_screen();
    }

    fn setup_header(&mut self) {
        let version = self.version;
        let m = &mut self.dynmem;
        if version <= 3 {
            // Status line available, screen splitting available, variable pitch off.
            m[1] = (m[1] & !0x70) | 0x20;
        } else {
            // No colours/pictures/bold/italic/sound/timed input; fixed-space available.
            m[1] = 0x10;
        }
        // Flags 2: keep transcript, fixed pitch and undo requests; refuse the rest.
        m[0x10] = 0;
        m[0x11] &= 0x13;
        m[0x1e] = 6;
        m[0x1f] = b'Q';
        m[0x20] = ROWS as u8;
        m[0x21] = COLS as u8;
        if version >= 5 {
            m[0x22] = 0;
            m[0x23] = COLS as u8;
            m[0x24] = 0;
            m[0x25] = ROWS as u8;
            m[0x26] = 1;
            m[0x27] = 1;
            m[0x2c] = 9; // default background: white
            m[0x2d] = 2; // default foreground: black
        }
        m[0x32] = 1;
        m[0x33] = 1;
    }
}

impl Attached<'_> {
    /// Execute up to `budget` instructions.
    ///
    /// Returns early with [`Step::Budget`] when the output buffer has grown past 16 KB so
    /// the caller can drain it, and with the input/halt/error states as they arise.
    pub fn run(&mut self, budget: u32) -> Step {
        for _ in 0..budget {
            if let Some(step) = self.blocked() {
                return step;
            }
            if self.output.len() >= OUTPUT_FLUSH {
                return Step::Budget;
            }
            let at = self.pc;
            if let Err(msg) = self.step() {
                let msg = format!("{msg} (pc {at:#x})");
                self.state = State::Error(msg.clone());
                return Step::Error(msg);
            }
        }
        self.blocked().unwrap_or(Step::Budget)
    }

    /// Provide a line of input after `WaitLine`. Ignored in any other state. The line is
    /// echoed to the output buffer followed by a newline.
    pub fn input(&mut self, line: &str) {
        let State::WaitLine { text, parse, store } = self.state.clone() else { return };
        self.state = match self.accept_line(text as usize, parse as usize, store, line) {
            Ok(()) => State::Running,
            Err(e) => State::Error(e),
        };
    }

    /// Provide a character after `WaitChar` (ZSCII: 13 = return, 8 = delete, 27 = escape,
    /// 129–132 arrows, 32–126 ASCII). Ignored in any other state.
    pub fn input_char(&mut self, c: u16) {
        let State::WaitChar { store } = self.state else { return };
        self.state = match self.write_var(store, c) {
            Ok(()) => State::Running,
            Err(e) => State::Error(e),
        };
    }

    /// Current status line for v3 games: (location name, score, turns) or
    /// (location, hours, minutes) for time games, with `is_time` telling which; `None`
    /// for v4+ (those draw their own via the upper window — see [`Machine::upper_window`]).
    pub fn status_line(&mut self) -> Option<(String, i16, i16, bool)> {
        if self.version > 3 {
            return None;
        }
        let g = self.rw(0x0c).ok()? as usize;
        let loc = self.rw(g).ok()?;
        let a = self.rw(g + 2).ok()? as i16;
        let b = self.rw(g + 4).ok()? as i16;
        let name = if loc == 0 { String::new() } else { self.obj_name(loc).unwrap_or_default() };
        let is_time = self.dynmem[1] & 0x02 != 0;
        Some((name, a, b, is_time))
    }

    // ----------------------------------------------------------------------------------
    // Reset and header

    /// Restart the story from the beginning, as the `restart` opcode does. Buffered output
    /// is kept; the transcript and fixed-pitch header bits survive, as the standard asks.
    pub fn restart(&mut self) {
        let flags2 = self.dynmem[0x11] & 0x03;
        if let Err(e) = self.reload_dyn() {
            self.state = State::Error(e);
            return;
        }
        self.dynmem[0x11] = (self.dynmem[0x11] & !0x03) | flags2;
        self.reset();
    }

    /// Re-read the original dynamic memory from the story source.
    fn reload_dyn(&mut self) -> R<()> {
        let n = self.static_base;
        let src = self.src;
        src.read_exact_at(0, &mut self.core.dynmem[..n]).map_err(|_| String::from("story read failed"))
    }

    // ----------------------------------------------------------------------------------
    // Memory

    fn rb(&self, a: usize) -> R<u8> {
        if a < self.static_base {
            return Ok(self.dynmem[a]);
        }
        if a >= self.len {
            return Err(format!("read outside memory at {a:#x}"));
        }
        self.pager.borrow_mut().byte(self.src, a).ok_or_else(|| String::from("story read failed"))
    }

    fn rw(&self, a: usize) -> R<u16> {
        let h = self.rb(a)?;
        let l = self.rb(a.wrapping_add(1))?;
        Ok(u16::from_be_bytes([h, l]))
    }

    fn wb(&mut self, a: usize, v: u8) -> R<()> {
        if a >= self.static_base {
            return Err(format!("write outside dynamic memory at {a:#x}"));
        }
        self.dynmem[a] = v;
        Ok(())
    }

    fn ww(&mut self, a: usize, v: u16) -> R<()> {
        if a.wrapping_add(1) >= self.static_base {
            return Err(format!("write outside dynamic memory at {a:#x}"));
        }
        let [h, l] = v.to_be_bytes();
        self.dynmem[a] = h;
        self.dynmem[a + 1] = l;
        Ok(())
    }

    fn fetch(&mut self) -> R<u8> {
        let b = self.rb(self.pc).map_err(|_| String::from("program counter outside memory"))?;
        self.pc += 1;
        Ok(b)
    }

    fn fetch_word(&mut self) -> R<u16> {
        let h = self.fetch()?;
        let l = self.fetch()?;
        Ok(u16::from_be_bytes([h, l]))
    }

    fn unpack(&self, p: u16, routine: bool) -> R<usize> {
        let p = p as usize;
        Ok(match self.version {
            3 => p * 2,
            4 | 5 => p * 4,
            7 => p * 4 + 8 * self.rw(if routine { 0x28 } else { 0x2a })? as usize,
            _ => p * 8,
        })
    }

    fn set_pc(&mut self, pc: i64) -> R<()> {
        if pc < 0 || pc as usize >= self.len {
            return fault("jump outside memory");
        }
        self.pc = pc as usize;
        Ok(())
    }

    // ----------------------------------------------------------------------------------
    // Stack and variables

    fn read_var(&mut self, v: u8) -> R<u16> {
        match v {
            0 => self.pop(),
            1..=15 => Ok(self.frame().locals[v as usize - 1]),
            _ => {
                let g = self.rw(0x0c)? as usize;
                self.rw(g + 2 * (v as usize - 16))
            }
        }
    }

    fn write_var(&mut self, v: u8, val: u16) -> R<()> {
        match v {
            0 => self.push(val),
            1..=15 => {
                let f = self.frames.last_mut().ok_or_else(|| String::from("no frame"))?;
                f.locals[v as usize - 1] = val;
                Ok(())
            }
            _ => {
                let g = self.rw(0x0c)? as usize;
                self.ww(g + 2 * (v as usize - 16), val)
            }
        }
    }

    /// Indirect variable reads (`load`, `inc`, ...) peek at the stack instead of popping.
    fn read_var_ind(&mut self, v: u8) -> R<u16> {
        if v == 0 {
            let top = self.pop()?;
            self.push(top)?;
            Ok(top)
        } else {
            self.read_var(v)
        }
    }

    /// Indirect variable writes (`store`, `pull`, ...) overwrite the top of the stack.
    fn write_var_ind(&mut self, v: u8, val: u16) -> R<()> {
        if v == 0 {
            self.pop()?;
            self.push(val)
        } else {
            self.write_var(v, val)
        }
    }

    fn store(&mut self, val: u16) -> R<()> {
        let v = self.fetch()?;
        self.write_var(v, val)
    }

    fn branch(&mut self, cond: bool) -> R<()> {
        let b = self.fetch()?;
        let on_true = b & 0x80 != 0;
        let off: i32 = if b & 0x40 != 0 {
            (b & 0x3f) as i32
        } else {
            let raw = (((b & 0x3f) as i32) << 8) | self.fetch()? as i32;
            if raw & 0x2000 != 0 {
                raw - 0x4000
            } else {
                raw
            }
        };
        if cond != on_true {
            return Ok(());
        }
        match off {
            0 => self.ret(0),
            1 => self.ret(1),
            _ => self.set_pc(self.pc as i64 + off as i64 - 2),
        }
    }

    fn call(&mut self, ops: &[u16], store: Option<u8>) -> R<()> {
        let packed = arg(ops, 0);
        if packed == 0 {
            if let Some(v) = store {
                self.write_var(v, 0)?;
            }
            return Ok(());
        }
        if self.frames.len() >= MAX_FRAMES {
            return fault("call stack overflow");
        }
        let addr = self.unpack(packed, true)?;
        let n = self.rb(addr)? as usize;
        if n > 15 {
            return fault("routine with more than 15 locals");
        }
        let mut pc = addr + 1;
        let mut locals = [0u16; 15];
        if self.version <= 4 {
            for l in locals.iter_mut().take(n) {
                *l = self.rw(pc)?;
                pc += 2;
            }
        }
        let args = ops.get(1..).unwrap_or(&[]);
        for (l, a) in locals.iter_mut().zip(args).take(n) {
            *l = *a;
        }
        if pc >= self.len {
            return fault("routine outside memory");
        }
        let ret_pc = self.pc as u32;
        let sp = self.stack.len() as u16;
        self.frames.push(Frame { ret_pc, store, nlocals: n as u8, nargs: args.len() as u8, sp, locals });
        self.pc = pc;
        Ok(())
    }

    fn ret(&mut self, val: u16) -> R<()> {
        if self.frames.len() <= 1 {
            return fault("return from the main routine");
        }
        let f = self.frames.pop().unwrap_or(Frame { ret_pc: 0, store: None, nlocals: 0, nargs: 0, sp: 0, locals: [0; 15] });
        self.stack.truncate(f.sp as usize);
        self.pc = f.ret_pc as usize;
        if let Some(v) = f.store {
            self.write_var(v, val)?;
        }
        Ok(())
    }

    // ----------------------------------------------------------------------------------
    // Text

    /// Map ZSCII to a character for output; `None` for codes that print nothing.
    fn zscii_to_char(&self, z: u16) -> Option<char> {
        match z {
            0 => None,
            9 => Some(' '),
            11 => Some(' '),
            13 => Some('\n'),
            32..=126 => Some(z as u8 as char),
            155..=223 => {
                let i = (z - 155) as usize;
                if let Some(c) = self.custom_unicode(i) {
                    return c;
                }
                DEFAULT_UNICODE.chars().nth(i)
            }
            _ => Some('?'),
        }
    }

    /// Look `i` up in the story's own Unicode table if it has one (`Some(None)` for an
    /// unprintable entry, `None` when there is no table).
    fn custom_unicode(&self, i: usize) -> Option<Option<char>> {
        if self.version < 5 {
            return None;
        }
        let ext = self.rw(0x36).ok()? as usize;
        if ext == 0 || self.rw(ext).ok()? < 3 {
            return None;
        }
        let table = self.rw(ext + 6).ok()? as usize;
        if table == 0 {
            return None;
        }
        let n = self.rb(table).ok()? as usize;
        if i >= n {
            return Some(None);
        }
        Some(char::from_u32(self.rw(table + 1 + 2 * i).ok()? as u32))
    }

    fn alphabet_char(&self, alpha: usize, code: u8) -> R<u16> {
        let i = code as usize - 6;
        if self.version >= 5 {
            let t = self.rw(0x34)? as usize;
            if t != 0 {
                return Ok(self.rb(t + alpha * 26 + i)? as u16);
            }
        }
        Ok(DEFAULT_ALPHABET[alpha][i] as u16)
    }

    /// Decode the Z-string at `addr` into `out`; returns the address just past it.
    fn decode_text(&self, mut addr: usize, out: &mut String, abbrevs_ok: bool) -> R<usize> {
        let mut shift = 0usize;
        let mut abbrev: Option<u16> = None;
        let mut tenbit: Option<(u8, u16)> = None; // (stage, high bits)
        loop {
            let w = self.rw(addr)?;
            addr += 2;
            for c in [(w >> 10) & 31, (w >> 5) & 31, w & 31] {
                if let Some(bank) = abbrev.take() {
                    let table = self.rw(0x18)? as usize;
                    let entry = self.rw(table + 2 * (32 * (bank as usize - 1) + c as usize))? as usize;
                    self.decode_text(entry * 2, out, false)?;
                    continue;
                }
                if let Some((stage, hi)) = tenbit.take() {
                    if stage == 0 {
                        tenbit = Some((1, c));
                    } else if let Some(ch) = self.zscii_to_char((hi << 5) | c) {
                        out.push(ch);
                    }
                    continue;
                }
                let alpha = core::mem::take(&mut shift);
                match c {
                    0 => out.push(' '),
                    1..=3 => {
                        if abbrevs_ok {
                            abbrev = Some(c);
                        }
                    }
                    4 => shift = 1,
                    5 => shift = 2,
                    6 if alpha == 2 => tenbit = Some((0, 0)),
                    7 if alpha == 2 => out.push('\n'),
                    _ => {
                        if let Some(ch) = self.zscii_to_char(self.alphabet_char(alpha, c as u8)?) {
                            out.push(ch);
                        }
                    }
                }
            }
            if w & 0x8000 != 0 {
                return Ok(addr);
            }
        }
    }

    fn text_at(&self, addr: usize) -> R<String> {
        let mut s = String::new();
        self.decode_text(addr, &mut s, true)?;
        Ok(s)
    }

    /// Find a ZSCII code in the (possibly custom) alphabets: (alphabet, Z-character).
    fn alphabet_lookup(&self, z: u8) -> Option<(usize, u8)> {
        for alpha in 0..3 {
            for code in 6u8..32 {
                if alpha == 2 && code < 8 {
                    continue;
                }
                if self.alphabet_char(alpha, code).ok()? == z as u16 {
                    return Some((alpha, code));
                }
            }
        }
        None
    }

    /// Encode a dictionary word: 6 Z-characters in 2 words (v3) or 9 in 3 words (v4+).
    fn encode_word(&self, word: &[u8]) -> [u16; 3] {
        let n = if self.version <= 3 { 6 } else { 9 };
        let mut z: Vec<u8> = Vec::with_capacity(n + 4);
        for &b in word {
            let b = b.to_ascii_lowercase();
            if b == b' ' {
                z.push(0);
            } else if let Some((alpha, code)) = self.alphabet_lookup(b) {
                if alpha > 0 {
                    z.push(3 + alpha as u8);
                }
                z.push(code);
            } else {
                z.extend_from_slice(&[5, 6, b >> 5, b & 31]);
            }
            if z.len() >= n {
                break;
            }
        }
        z.resize(n, 5);
        let mut out = [0u16; 3];
        for (i, ch) in z.chunks(3).enumerate() {
            out[i] = ((ch[0] as u16) << 10) | ((ch[1] as u16) << 5) | ch[2] as u16;
        }
        out[n / 3 - 1] |= 0x8000;
        out
    }

    /// Address of the dictionary entry for `word`, or 0.
    fn dict_lookup(&self, dict: usize, word: &[u8]) -> R<u16> {
        let nsep = self.rb(dict)? as usize;
        let entry_len = self.rb(dict + 1 + nsep)? as usize;
        let count = self.rw(dict + 2 + nsep)? as i16;
        let base = dict + 4 + nsep;
        let key = self.encode_word(word);
        let key_words = if self.version <= 3 { 2 } else { 3 };
        for i in 0..count.unsigned_abs() as usize {
            let a = base + i * entry_len;
            let mut hit = true;
            for (k, &kw) in key.iter().enumerate().take(key_words) {
                if self.rw(a + 2 * k)? != kw {
                    hit = false;
                    break;
                }
            }
            if hit {
                return Ok(a as u16);
            }
        }
        Ok(0)
    }

    fn tokenise(&mut self, text: usize, parse: usize, dict: usize, keep_unknown: bool) -> R<()> {
        let (start, len) = if self.version <= 4 {
            let mut n = 0;
            while self.rb(text + 1 + n)? != 0 {
                n += 1;
            }
            (text + 1, n)
        } else {
            (text + 2, self.rb(text + 1)? as usize)
        };
        let nsep = self.rb(dict)? as usize;
        let mut seps = Vec::with_capacity(nsep);
        for i in 0..nsep {
            seps.push(self.rb(dict + 1 + i)?);
        }
        let max = self.rb(parse)? as usize;
        let mut count = 0;
        let mut i = 0;
        while i < len && count < max {
            let c = self.rb(start + i)?;
            if c == b' ' {
                i += 1;
                continue;
            }
            let ws = i;
            let we = if seps.contains(&c) {
                i + 1
            } else {
                let mut e = i;
                while e < len {
                    let c = self.rb(start + e)?;
                    if c == b' ' || seps.contains(&c) {
                        break;
                    }
                    e += 1;
                }
                e
            };
            let mut word = Vec::with_capacity(we - ws);
            for k in ws..we {
                word.push(self.rb(start + k)?);
            }
            let addr = self.dict_lookup(dict, &word)?;
            let e = parse + 2 + count * 4;
            if !(keep_unknown && addr == 0) {
                self.ww(e, addr)?;
                self.wb(e + 2, (we - ws) as u8)?;
                self.wb(e + 3, (start + ws - text) as u8)?;
            }
            count += 1;
            i = we;
        }
        self.wb(parse + 1, count as u8)
    }

    fn accept_line(&mut self, text: usize, parse: usize, store: Option<u8>, line: &str) -> R<()> {
        let max = self.rb(text)? as usize;
        let bytes: Vec<u8> =
            line.chars().filter(|c| *c != '\n' && *c != '\r').map(|c| char_to_zscii(c).unwrap_or(b'?').to_ascii_lowercase()).collect();
        if self.version <= 4 {
            let n = bytes.len().min(max.saturating_sub(1));
            for (i, &b) in bytes.iter().enumerate().take(n) {
                self.wb(text + 1 + i, b)?;
            }
            self.wb(text + 1 + n, 0)?;
        } else {
            let n = bytes.len().min(max);
            self.wb(text + 1, n as u8)?;
            for (i, &b) in bytes.iter().enumerate().take(n) {
                self.wb(text + 2 + i, b)?;
            }
        }
        self.output.push_str(line.trim_end_matches(['\n', '\r']));
        self.output.push('\n');
        if parse != 0 {
            let dict = self.rw(0x08)? as usize;
            self.tokenise(text, parse, dict, false)?;
        }
        if let Some(v) = store {
            self.write_var(v, 13)?;
        }
        Ok(())
    }

    // ----------------------------------------------------------------------------------
    // Output

    fn out_char(&mut self, c: char) -> R<()> {
        if let Some(&(table, count)) = self.stream3.last() {
            if count == u16::MAX {
                return fault("memory output stream overflow");
            }
            let z = char_to_zscii(c).unwrap_or(b'?');
            self.wb(table as usize + 2 + count as usize, z)?;
            if let Some(s) = self.stream3.last_mut() {
                s.1 += 1;
            }
            return Ok(());
        }
        if !self.screen_on {
            return Ok(());
        }
        if self.window == 1 {
            self.upper_put(c);
        } else {
            self.output.push(c);
        }
        Ok(())
    }

    fn print_str(&mut self, s: &str) -> R<()> {
        for c in s.chars() {
            self.out_char(c)?;
        }
        Ok(())
    }

    fn print_zscii(&mut self, z: u16) -> R<()> {
        match self.zscii_to_char(z) {
            Some(c) => self.out_char(c),
            None => Ok(()),
        }
    }

    // ----------------------------------------------------------------------------------
    // Objects

    fn obj_addr(&self, obj: u16) -> R<usize> {
        if obj == 0 {
            return fault("object 0");
        }
        let base = self.rw(0x0a)? as usize;
        let (defaults, size) = if self.version <= 3 { (62, 9) } else { (126, 14) };
        let a = base + defaults + (obj as usize - 1) * size;
        if a + size > self.len {
            return fault("object outside memory");
        }
        Ok(a)
    }

    /// Parent (0), sibling (1) or child (2) of `obj`; object 0 has none.
    fn rel(&self, obj: u16, which: usize) -> R<u16> {
        if obj == 0 {
            return Ok(0);
        }
        let a = self.obj_addr(obj)?;
        if self.version <= 3 {
            Ok(self.rb(a + 4 + which)? as u16)
        } else {
            self.rw(a + 6 + 2 * which)
        }
    }

    fn set_rel(&mut self, obj: u16, which: usize, val: u16) -> R<()> {
        if obj == 0 {
            return Ok(());
        }
        let a = self.obj_addr(obj)?;
        if self.version <= 3 {
            if val > 255 {
                return fault("object number over 255 in a v3 story");
            }
            self.wb(a + 4 + which, val as u8)
        } else {
            self.ww(a + 6 + 2 * which, val)
        }
    }

    fn attr_pos(&self, obj: u16, attr: u16) -> R<(usize, u8)> {
        let max = if self.version <= 3 { 32 } else { 48 };
        if attr >= max {
            return fault("attribute number out of range");
        }
        let a = self.obj_addr(obj)?;
        Ok((a + attr as usize / 8, 0x80 >> (attr % 8)))
    }

    fn test_attr(&self, obj: u16, attr: u16) -> R<bool> {
        if obj == 0 {
            return Ok(false);
        }
        let (a, bit) = self.attr_pos(obj, attr)?;
        Ok(self.rb(a)? & bit != 0)
    }

    fn set_attr(&mut self, obj: u16, attr: u16, on: bool) -> R<()> {
        if obj == 0 {
            return Ok(());
        }
        let (a, bit) = self.attr_pos(obj, attr)?;
        let b = self.rb(a)?;
        self.wb(a, if on { b | bit } else { b & !bit })
    }

    fn remove_obj(&mut self, obj: u16) -> R<()> {
        if obj == 0 {
            return Ok(());
        }
        let parent = self.rel(obj, 0)?;
        if parent != 0 {
            let sib = self.rel(obj, 1)?;
            let mut cur = self.rel(parent, 2)?;
            if cur == obj {
                self.set_rel(parent, 2, sib)?;
            } else {
                let mut guard = 0;
                while cur != 0 {
                    let next = self.rel(cur, 1)?;
                    if next == obj {
                        self.set_rel(cur, 1, sib)?;
                        break;
                    }
                    cur = next;
                    guard += 1;
                    if guard > 0xffff {
                        return fault("cyclic object tree");
                    }
                }
            }
        }
        self.set_rel(obj, 0, 0)?;
        self.set_rel(obj, 1, 0)
    }

    fn insert_obj(&mut self, obj: u16, dest: u16) -> R<()> {
        if obj == 0 {
            return Ok(());
        }
        self.remove_obj(obj)?;
        let first = self.rel(dest, 2)?;
        self.set_rel(obj, 1, first)?;
        self.set_rel(obj, 0, dest)?;
        self.set_rel(dest, 2, obj)
    }

    fn prop_table(&self, obj: u16) -> R<usize> {
        let a = self.obj_addr(obj)?;
        Ok(self.rw(a + if self.version <= 3 { 7 } else { 12 })? as usize)
    }

    fn obj_name(&self, obj: u16) -> R<String> {
        let t = self.prop_table(obj)?;
        if self.rb(t)? == 0 {
            return Ok(String::new());
        }
        self.text_at(t + 1)
    }

    /// Property at `addr`: (number, length, data address); `None` at the terminator.
    fn prop_at(&self, addr: usize) -> R<Option<(u8, usize, usize)>> {
        let b = self.rb(addr)?;
        if b == 0 {
            return Ok(None);
        }
        if self.version <= 3 {
            return Ok(Some((b & 31, (b >> 5) as usize + 1, addr + 1)));
        }
        if b & 0x80 != 0 {
            let len = (self.rb(addr + 1)? & 63) as usize;
            Ok(Some((b & 63, if len == 0 { 64 } else { len }, addr + 2)))
        } else {
            Ok(Some((b & 63, if b & 0x40 != 0 { 2 } else { 1 }, addr + 1)))
        }
    }

    fn first_prop(&self, obj: u16) -> R<usize> {
        let t = self.prop_table(obj)?;
        Ok(t + 1 + 2 * self.rb(t)? as usize)
    }

    fn find_prop(&self, obj: u16, num: u16) -> R<Option<(usize, usize)>> {
        let mut a = self.first_prop(obj)?;
        for _ in 0..64 {
            match self.prop_at(a)? {
                None => return Ok(None),
                Some((n, len, data)) => {
                    if n as u16 == num {
                        return Ok(Some((len, data)));
                    }
                    a = data + len;
                }
            }
        }
        fault("runaway property list")
    }

    fn prop_len_at(&self, data: usize) -> R<u16> {
        if data == 0 {
            return Ok(0);
        }
        let b = self.rb(data - 1)?;
        Ok(if self.version <= 3 {
            (b >> 5) as u16 + 1
        } else if b & 0x80 != 0 {
            let l = (b & 63) as u16;
            if l == 0 {
                64
            } else {
                l
            }
        } else if b & 0x40 != 0 {
            2
        } else {
            1
        })
    }

    fn get_prop(&self, obj: u16, num: u16) -> R<u16> {
        let max = if self.version <= 3 { 31 } else { 63 };
        if num == 0 || num > max {
            return fault("property number out of range");
        }
        if obj != 0 {
            if let Some((len, data)) = self.find_prop(obj, num)? {
                return if len == 1 { Ok(self.rb(data)? as u16) } else { self.rw(data) };
            }
        }
        let base = self.rw(0x0a)? as usize;
        self.rw(base + 2 * (num as usize - 1))
    }

    fn get_next_prop(&self, obj: u16, num: u16) -> R<u16> {
        if obj == 0 {
            return Ok(0);
        }
        let a = if num == 0 {
            self.first_prop(obj)?
        } else {
            match self.find_prop(obj, num)? {
                Some((len, data)) => data + len,
                None => return Ok(0),
            }
        };
        Ok(self.prop_at(a)?.map_or(0, |(n, _, _)| n as u16))
    }

    fn put_prop(&mut self, obj: u16, num: u16, val: u16) -> R<()> {
        if obj == 0 {
            return Ok(());
        }
        match self.find_prop(obj, num)? {
            Some((1, data)) => self.wb(data, val as u8),
            Some((2, data)) => self.ww(data, val),
            Some(_) => fault("put_prop on a property longer than two bytes"),
            None => fault("put_prop on a property the object lacks"),
        }
    }

    // ----------------------------------------------------------------------------------
    // Snapshots

    /// Snapshot the whole machine state as a byte vector. The format ("QZS1") is:
    ///
    /// ```text
    /// 0   "QZS1"
    /// 4   release (u16), serial (6 bytes), checksum (u16) — must match on restore
    /// 14  state tag: 0 = taken by a save opcode (pc at its store/branch byte, so a
    ///     restore reports "restored" to the game), 1 = waiting for a line, 2 = waiting
    ///     for a character, 3 = running (taken by the UI between instructions)
    /// 15  text buffer (u16), parse buffer (u16), store variable (u8, 0xff = none)
    /// 20  pc (u32)
    /// 24  rng state (u32), predictable-mode range (u16), counter (u16)
    /// 32  stack: count (u16) then that many words
    ///     frames: count (u16) then per frame return pc (u32), store variable (u8,
    ///     0xff = none), local count (u8), argument count (u8), stack base (u16), locals
    ///     dynamic memory: length (u32) then Quetzal-style bytes — each byte is
    ///     XOR-ed with the original story; a zero is followed by a count of further
    ///     zeros (0..=255) and trailing zeros are dropped
    /// ```
    ///
    /// All integers are big-endian. Snapshots are typically a few hundred bytes to a few
    /// KB. The in-game `save`/`save_undo` opcodes use the same format.
    ///
    /// Compressing the dynamic memory re-reads the story's original bytes from the card;
    /// should that fail the result is empty (and a `restore` of it fails).
    pub fn save(&self) -> Vec<u8> {
        self.snapshot(false)
    }

    fn snapshot(&self, at_save_opcode: bool) -> Vec<u8> {
        let Some(dyn_bytes) = self.compress_dyn() else { return Vec::new() };
        let mut out = Vec::with_capacity(512 + dyn_bytes.len());
        out.extend_from_slice(SNAPSHOT_MAGIC);
        out.extend_from_slice(&self.dynmem[2..4]);
        out.extend_from_slice(&self.dynmem[0x12..0x18]);
        out.extend_from_slice(&self.dynmem[0x1c..0x1e]);
        let (tag, text, parse, store) = match &self.state {
            State::WaitLine { text, parse, store } => (1u8, *text, *parse, store.unwrap_or(0xff)),
            State::WaitChar { store } => (2, 0, 0, *store),
            _ if at_save_opcode => (0, 0, 0, 0xff),
            _ => (3, 0, 0, 0xff),
        };
        out.push(tag);
        out.extend_from_slice(&text.to_be_bytes());
        out.extend_from_slice(&parse.to_be_bytes());
        out.push(store);
        out.extend_from_slice(&(self.pc as u32).to_be_bytes());
        out.extend_from_slice(&self.rng.to_be_bytes());
        out.extend_from_slice(&self.rng_pred.to_be_bytes());
        out.extend_from_slice(&self.rng_count.to_be_bytes());
        out.extend_from_slice(&(self.stack.len() as u16).to_be_bytes());
        for w in &self.stack {
            out.extend_from_slice(&w.to_be_bytes());
        }
        out.extend_from_slice(&(self.frames.len() as u16).to_be_bytes());
        for f in &self.frames {
            out.extend_from_slice(&f.ret_pc.to_be_bytes());
            out.push(f.store.unwrap_or(0xff));
            out.push(f.nlocals);
            out.push(f.nargs);
            out.extend_from_slice(&f.sp.to_be_bytes());
            for l in &f.locals[..f.nlocals as usize] {
                out.extend_from_slice(&l.to_be_bytes());
            }
        }
        out.extend_from_slice(&(dyn_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&dyn_bytes);
        out
    }

    /// XOR dynamic memory against the story's original bytes (streamed from the source
    /// a sector at a time), zero runs as `0, count`; trailing zeros are dropped.
    fn compress_dyn(&self) -> Option<Vec<u8>> {
        let n = self.static_base;
        let mut out = Vec::new();
        let mut orig = [0u8; SECTOR];
        let mut zeros = 0usize;
        let mut at = 0;
        while at < n {
            let want = (n - at).min(SECTOR);
            self.src.read_exact_at(at as u64, &mut orig[..want]).ok()?;
            for (k, o) in orig[..want].iter().enumerate() {
                let x = self.dynmem[at + k] ^ o;
                if x == 0 {
                    zeros += 1;
                    if zeros == 256 {
                        out.push(0);
                        out.push(255);
                        zeros = 0;
                    }
                } else {
                    if zeros > 0 {
                        out.push(0);
                        out.push((zeros - 1) as u8);
                        zeros = 0;
                    }
                    out.push(x);
                }
            }
            at += want;
        }
        Some(out)
    }

    /// Check that a compressed dynamic-memory delta decodes to at most `n` bytes.
    fn delta_fits(data: &[u8], n: usize) -> bool {
        let mut i = 0;
        let mut p = 0;
        while p < data.len() {
            let b = data[p];
            p += 1;
            if b != 0 {
                i += 1;
            } else {
                let Some(&run) = data.get(p) else { return false };
                p += 1;
                i += run as usize + 1;
            }
            if i > n {
                return false;
            }
        }
        true
    }

    /// Apply a delta that passed [`Machine::delta_fits`] to dynamic memory.
    fn apply_delta(&mut self, data: &[u8]) -> R<()> {
        self.reload_dyn()?;
        let mut i = 0;
        let mut p = 0;
        while p < data.len() {
            let b = data[p];
            p += 1;
            if b != 0 {
                self.dynmem[i] ^= b;
                i += 1;
            } else {
                i += data.get(p).copied().unwrap_or(0) as usize + 1;
                p += 1;
            }
        }
        Ok(())
    }

    /// Load a snapshot made by [`Machine::save`] (or by the game's own `save` opcode).
    /// Returns `false`, leaving the machine untouched, if the data is malformed or belongs
    /// to a different story. Buffered output is kept; the upper window is cleared.
    pub fn restore(&mut self, data: &[u8]) -> bool {
        let mut r = Cursor { d: data, p: 0 };
        let Some(magic) = r.bytes(4) else { return false };
        if magic != SNAPSHOT_MAGIC {
            return false;
        }
        let Some(ident) = r.bytes(10) else { return false };
        if ident[..2] != self.dynmem[2..4] || ident[2..8] != self.dynmem[0x12..0x18] || ident[8..] != self.dynmem[0x1c..0x1e] {
            return false;
        }
        let (Some(tag), Some(text), Some(parse), Some(store)) = (r.u8(), r.u16(), r.u16(), r.u8()) else { return false };
        let (Some(pc), Some(rng), Some(rng_pred), Some(rng_count)) = (r.u32(), r.u32(), r.u16(), r.u16()) else {
            return false;
        };
        if pc as usize >= self.len {
            return false;
        }
        let Some(nstack) = r.u16() else { return false };
        if nstack as usize > MAX_STACK {
            return false;
        }
        let mut stack = Vec::with_capacity(nstack as usize);
        for _ in 0..nstack {
            let Some(w) = r.u16() else { return false };
            stack.push(w);
        }
        let Some(nframes) = r.u16() else { return false };
        if nframes == 0 || nframes as usize > MAX_FRAMES {
            return false;
        }
        let mut frames = Vec::with_capacity(nframes as usize);
        for _ in 0..nframes {
            let (Some(ret_pc), Some(store), Some(nlocals), Some(nargs), Some(sp)) = (r.u32(), r.u8(), r.u8(), r.u8(), r.u16()) else {
                return false;
            };
            if nlocals > 15 || sp as usize > stack.len() || ret_pc as usize >= self.len {
                return false;
            }
            let mut locals = [0u16; 15];
            for l in locals.iter_mut().take(nlocals as usize) {
                let Some(v) = r.u16() else { return false };
                *l = v;
            }
            frames.push(Frame { ret_pc, store: if store == 0xff { None } else { Some(store) }, nlocals, nargs, sp, locals });
        }
        let Some(dyn_len) = r.u32() else { return false };
        let Some(dyn_bytes) = r.bytes(dyn_len as usize) else { return false };
        if !Self::delta_fits(dyn_bytes, self.static_base) {
            return false;
        }
        let flags2 = self.dynmem[0x11] & 0x03;
        if let Err(e) = self.apply_delta(dyn_bytes) {
            self.state = State::Error(e);
            return false;
        }
        self.dynmem[0x11] = (self.dynmem[0x11] & !0x03) | flags2;
        self.setup_header();
        self.stack = stack;
        self.frames = frames;
        self.pc = pc as usize;
        self.rng = rng;
        self.rng_pred = rng_pred;
        self.rng_count = rng_count;
        self.reset_screen();
        self.state = match tag {
            1 => State::WaitLine { text, parse, store: if store == 0xff { None } else { Some(store) } },
            2 => State::WaitChar { store },
            _ => State::Running,
        };
        if tag == 0 {
            // We are at the save instruction's store/branch byte: report "restored".
            let r = if self.version <= 3 { self.branch(true) } else { self.store(2) };
            if let Err(e) = r {
                self.state = State::Error(e);
            }
        }
        true
    }

    // ----------------------------------------------------------------------------------
    // Execution

    fn operand(&mut self, ty: u8) -> R<u16> {
        match ty {
            0 => self.fetch_word(),
            1 => Ok(self.fetch()? as u16),
            2 => {
                let v = self.fetch()?;
                self.read_var(v)
            }
            _ => fault("omitted operand"),
        }
    }

    fn step(&mut self) -> R<()> {
        self.ticks = self.ticks.wrapping_add(1);
        let op = self.fetch()?;
        let mut ops = [0u16; 8];
        match op {
            0x00..=0x7f => {
                ops[0] = self.operand(if op & 0x40 != 0 { 2 } else { 1 })?;
                ops[1] = self.operand(if op & 0x20 != 0 { 2 } else { 1 })?;
                self.exec_2op(op & 0x1f, &ops[..2])
            }
            0x80..=0xaf => {
                let a = self.operand((op >> 4) & 3)?;
                self.exec_1op(op & 0x0f, a)
            }
            0xbe if self.version >= 5 => {
                let ext = self.fetch()?;
                let n = self.var_operands(&mut ops, false)?;
                self.exec_ext(ext, &ops[..n])
            }
            0xb0..=0xbf => self.exec_0op(op & 0x0f),
            _ => {
                let n = self.var_operands(&mut ops, op == 0xec || op == 0xfa)?;
                if op < 0xe0 {
                    self.exec_2op(op & 0x1f, &ops[..n])
                } else {
                    self.exec_var(op & 0x1f, &ops[..n])
                }
            }
        }
    }

    fn var_operands(&mut self, ops: &mut [u16; 8], two_bytes: bool) -> R<usize> {
        let mut types = [self.fetch()?, 0xff];
        if two_bytes {
            types[1] = self.fetch()?;
        }
        let mut n = 0;
        for t in types {
            for i in 0..4 {
                let ty = (t >> (6 - 2 * i)) & 3;
                if ty == 3 {
                    return Ok(n);
                }
                ops[n] = self.operand(ty)?;
                n += 1;
            }
        }
        Ok(n)
    }

    fn exec_2op(&mut self, op: u8, ops: &[u16]) -> R<()> {
        let a = arg(ops, 0);
        let b = arg(ops, 1);
        match op {
            0x01 => {
                let hit = ops.len() >= 2 && ops[1..].contains(&a);
                self.branch(hit)
            }
            0x02 => self.branch((a as i16) < (b as i16)),
            0x03 => self.branch((a as i16) > (b as i16)),
            0x04 => {
                let v = self.read_var_ind(a as u8)?.wrapping_sub(1);
                self.write_var_ind(a as u8, v)?;
                self.branch((v as i16) < (b as i16))
            }
            0x05 => {
                let v = self.read_var_ind(a as u8)?.wrapping_add(1);
                self.write_var_ind(a as u8, v)?;
                self.branch((v as i16) > (b as i16))
            }
            0x06 => {
                let p = self.rel(a, 0)?;
                self.branch(p == b)
            }
            0x07 => self.branch(a & b == b),
            0x08 => self.store(a | b),
            0x09 => self.store(a & b),
            0x0a => {
                let t = self.test_attr(a, b)?;
                self.branch(t)
            }
            0x0b => self.set_attr(a, b, true),
            0x0c => self.set_attr(a, b, false),
            0x0d => self.write_var_ind(a as u8, b),
            0x0e => self.insert_obj(a, b),
            0x0f => {
                let v = self.rw(a.wrapping_add(b.wrapping_mul(2)) as usize)?;
                self.store(v)
            }
            0x10 => {
                let v = self.rb(a.wrapping_add(b) as usize)?;
                self.store(v as u16)
            }
            0x11 => {
                let v = self.get_prop(a, b)?;
                self.store(v)
            }
            0x12 => {
                let v = if a == 0 { None } else { self.find_prop(a, b)? };
                self.store(v.map_or(0, |(_, data)| data as u16))
            }
            0x13 => {
                let v = self.get_next_prop(a, b)?;
                self.store(v)
            }
            0x14 => self.store(a.wrapping_add(b)),
            0x15 => self.store(a.wrapping_sub(b)),
            0x16 => self.store(a.wrapping_mul(b)),
            0x17 => {
                if b == 0 {
                    return fault("division by zero");
                }
                self.store((a as i16).wrapping_div(b as i16) as u16)
            }
            0x18 => {
                if b == 0 {
                    return fault("division by zero");
                }
                self.store((a as i16).wrapping_rem(b as i16) as u16)
            }
            0x19 => {
                let v = self.fetch()?;
                self.call(ops, Some(v))
            }
            0x1a => self.call(ops, None),
            0x1b => Ok(()),
            0x1c => {
                let depth = b as usize;
                if depth == 0 || depth > self.frames.len() {
                    return fault("throw to a frame that does not exist");
                }
                self.frames.truncate(depth);
                self.ret(a)
            }
            _ => Err(format!("illegal 2OP opcode {op:#x}")),
        }
    }

    fn exec_1op(&mut self, op: u8, a: u16) -> R<()> {
        match op {
            0x00 => self.branch(a == 0),
            0x01 => {
                let v = self.rel(a, 1)?;
                self.store(v)?;
                self.branch(v != 0)
            }
            0x02 => {
                let v = self.rel(a, 2)?;
                self.store(v)?;
                self.branch(v != 0)
            }
            0x03 => {
                let v = self.rel(a, 0)?;
                self.store(v)
            }
            0x04 => {
                let v = self.prop_len_at(a as usize)?;
                self.store(v)
            }
            0x05 => {
                let v = self.read_var_ind(a as u8)?.wrapping_add(1);
                self.write_var_ind(a as u8, v)
            }
            0x06 => {
                let v = self.read_var_ind(a as u8)?.wrapping_sub(1);
                self.write_var_ind(a as u8, v)
            }
            0x07 => {
                let s = self.text_at(a as usize)?;
                self.print_str(&s)
            }
            0x08 => {
                let v = self.fetch()?;
                self.call(&[a], Some(v))
            }
            0x09 => self.remove_obj(a),
            0x0a => {
                if a == 0 {
                    return Ok(());
                }
                let s = self.obj_name(a)?;
                self.print_str(&s)
            }
            0x0b => self.ret(a),
            0x0c => self.set_pc(self.pc as i64 + (a as i16) as i64 - 2),
            0x0d => {
                let addr = self.unpack(a, false)?;
                let s = self.text_at(addr)?;
                self.print_str(&s)
            }
            0x0e => {
                let v = self.read_var_ind(a as u8)?;
                self.store(v)
            }
            0x0f => {
                if self.version <= 4 {
                    self.store(!a)
                } else {
                    self.call(&[a], None)
                }
            }
            _ => Err(format!("illegal 1OP opcode {op:#x}")),
        }
    }

    fn exec_0op(&mut self, op: u8) -> R<()> {
        match op {
            0x00 => self.ret(1),
            0x01 => self.ret(0),
            0x02 => {
                let mut s = String::new();
                self.pc = self.decode_text(self.pc, &mut s, true)?;
                self.print_str(&s)
            }
            0x03 => {
                let mut s = String::new();
                self.pc = self.decode_text(self.pc, &mut s, true)?;
                self.print_str(&s)?;
                self.out_char('\n')?;
                self.ret(1)
            }
            0x04 => Ok(()),
            0x05 => {
                self.pending_save = Some(self.snapshot(true));
                if self.version <= 3 {
                    self.branch(true)
                } else {
                    self.store(1)
                }
            }
            0x06 => self.op_restore(),
            0x07 => {
                self.restart();
                Ok(())
            }
            0x08 => {
                let v = self.pop()?;
                self.ret(v)
            }
            0x09 => {
                if self.version <= 4 {
                    self.pop().map(|_| ())
                } else {
                    self.store(self.frames.len() as u16)
                }
            }
            0x0a => {
                self.state = State::Halted;
                Ok(())
            }
            0x0b => self.out_char('\n'),
            0x0c => Ok(()),
            0x0d => {
                let ok = self.verify();
                self.branch(ok)
            }
            0x0f => self.branch(true),
            _ => Err(format!("illegal 0OP opcode {op:#x}")),
        }
    }

    fn op_restore(&mut self) -> R<()> {
        if let Some(data) = self.offered_restore.take() {
            if self.restore(&data) {
                return Ok(());
            }
        }
        if self.version <= 3 {
            self.branch(false)
        } else {
            self.store(0)
        }
    }

    fn verify(&self) -> bool {
        let scale = match self.version {
            3 => 2,
            4 | 5 => 4,
            _ => 8,
        };
        let len = (be16(&self.dynmem, 0x1a) as usize * scale).min(self.len);
        // Sum the file as written, streamed from the source a sector at a time.
        let mut buf = [0u8; SECTOR];
        let mut sum = 0u16;
        let mut at = 0x40;
        while at < len {
            let want = (len - at).min(SECTOR);
            if self.src.read_exact_at(at as u64, &mut buf[..want]).is_err() {
                return false;
            }
            for b in &buf[..want] {
                sum = sum.wrapping_add(*b as u16);
            }
            at += want;
        }
        sum == be16(&self.dynmem, 0x1c)
    }

    fn exec_var(&mut self, op: u8, ops: &[u16]) -> R<()> {
        let a = arg(ops, 0);
        let b = arg(ops, 1);
        let c = arg(ops, 2);
        match op {
            0x00 | 0x0c => {
                let v = self.fetch()?;
                self.call(ops, Some(v))
            }
            0x01 => self.ww(a.wrapping_add(b.wrapping_mul(2)) as usize, c),
            0x02 => self.wb(a.wrapping_add(b) as usize, c as u8),
            0x03 => self.put_prop(a, b, c),
            0x04 => {
                let store = if self.version >= 5 { Some(self.fetch()?) } else { None };
                self.state = State::WaitLine { text: a, parse: b, store };
                Ok(())
            }
            0x05 => self.print_zscii(a),
            0x06 => {
                let s = format!("{}", a as i16);
                self.print_str(&s)
            }
            0x07 => {
                let v = self.random(a as i16);
                self.store(v)
            }
            0x08 => self.push(a),
            0x09 => {
                let v = self.pop()?;
                self.write_var_ind(a as u8, v)
            }
            0x0a => {
                self.split_window(a);
                Ok(())
            }
            0x0b => {
                self.window = (a == 1) as u8;
                if a == 1 {
                    self.cursor = (0, 0);
                }
                Ok(())
            }
            0x0d => {
                self.erase_window(a);
                Ok(())
            }
            0x0e => {
                if self.window == 1 && a == 1 {
                    let (row, col) = self.cursor;
                    if row < self.upper_rows {
                        self.upper[row * COLS + col.min(COLS)..(row + 1) * COLS].iter_mut().for_each(|c| *c = ' ');
                    }
                }
                Ok(())
            }
            0x0f => {
                if self.window == 1 && (a as i16) > 0 {
                    let row = (a as usize - 1).min(ROWS);
                    let col = (b as usize).saturating_sub(1).min(COLS);
                    self.cursor = (row, col);
                }
                Ok(())
            }
            0x10 => {
                let (row, col) = if self.window == 1 { self.cursor } else { (self.upper_rows, 0) };
                self.ww(a as usize, row as u16 + 1)?;
                self.ww(a as usize + 2, col as u16 + 1)
            }
            0x11 | 0x12 | 0x14 | 0x15 => Ok(()),
            0x13 => self.output_stream(a as i16, b),
            0x16 => {
                let store = self.fetch()?;
                self.state = State::WaitChar { store };
                Ok(())
            }
            0x17 => {
                let form = if ops.len() > 3 { arg(ops, 3) } else { 0x82 };
                let words = form & 0x80 != 0;
                let step = (form & 0x7f) as usize;
                if step == 0 {
                    return fault("scan_table with zero field length");
                }
                let mut found = 0u16;
                let mut addr = b as usize;
                for _ in 0..c {
                    let v = if words { self.rw(addr)? } else { self.rb(addr)? as u16 };
                    if v == a {
                        found = addr as u16;
                        break;
                    }
                    addr += step;
                }
                self.store(found)?;
                self.branch(found != 0)
            }
            0x18 => self.store(!a),
            0x19 | 0x1a => self.call(ops, None),
            0x1b => {
                let dict = if c == 0 { self.rw(0x08)? as usize } else { c as usize };
                self.tokenise(a as usize, b as usize, dict, arg(ops, 3) != 0)
            }
            0x1c => {
                let from = a as usize + c as usize;
                let end = from + b as usize;
                if end > self.len {
                    return fault("encode_text outside memory");
                }
                let mut word = Vec::with_capacity(end - from);
                for k in from..end {
                    word.push(self.rb(k)?);
                }
                let enc = self.encode_word(&word);
                let dest = arg(ops, 3) as usize;
                for (i, w) in enc.iter().enumerate().take(if self.version <= 3 { 2 } else { 3 }) {
                    self.ww(dest + 2 * i, *w)?;
                }
                Ok(())
            }
            0x1d => self.copy_table(a, b, c as i16),
            0x1e => self.print_table(a, b, c, if ops.len() > 3 { arg(ops, 3) } else { 0 }),
            0x1f => {
                let n = self.frame().nargs as u16;
                self.branch(a <= n)
            }
            _ => Err(format!("illegal VAR opcode {op:#x}")),
        }
    }

    fn output_stream(&mut self, n: i16, table: u16) -> R<()> {
        match n {
            1 => self.screen_on = true,
            -1 => self.screen_on = false,
            3 => {
                if self.stream3.len() >= MAX_STREAM3 {
                    return fault("too many nested memory output streams");
                }
                self.ww(table as usize, 0)?;
                self.stream3.push((table, 0));
            }
            -3 => {
                if let Some((table, count)) = self.stream3.pop() {
                    self.ww(table as usize, count)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn copy_table(&mut self, first: u16, second: u16, size: i16) -> R<()> {
        let n = size.unsigned_abs() as usize;
        let first = first as usize;
        if second == 0 {
            for i in 0..n {
                self.wb(first + i, 0)?;
            }
            return Ok(());
        }
        let second = second as usize;
        if size < 0 || second <= first {
            for i in 0..n {
                let b = self.rb(first + i)?;
                self.wb(second + i, b)?;
            }
        } else {
            for i in (0..n).rev() {
                let b = self.rb(first + i)?;
                self.wb(second + i, b)?;
            }
        }
        Ok(())
    }

    fn print_table(&mut self, text: u16, width: u16, height: u16, skip: u16) -> R<()> {
        let height = if height == 0 { 1 } else { height };
        if width > 255 || height > 255 {
            return fault("print_table too large");
        }
        let mut addr = text as usize;
        for row in 0..height {
            if row > 0 {
                self.out_char('\n')?;
            }
            for i in 0..width as usize {
                let z = self.rb(addr + i)? as u16;
                self.print_zscii(z)?;
            }
            addr += width as usize + skip as usize;
        }
        Ok(())
    }

    fn exec_ext(&mut self, op: u8, ops: &[u16]) -> R<()> {
        let a = arg(ops, 0);
        let b = arg(ops, 1);
        match op {
            0x00 => {
                self.pending_save = Some(self.snapshot(true));
                self.store(1)
            }
            0x01 => self.op_restore(),
            0x02 => {
                let places = b as i16;
                let v = if places >= 0 { a.checked_shl(places as u32).unwrap_or(0) } else { a.checked_shr((-places) as u32).unwrap_or(0) };
                self.store(v)
            }
            0x03 => {
                let places = b as i16;
                let v = if places >= 0 {
                    a.checked_shl(places as u32).unwrap_or(0)
                } else {
                    (a as i16).checked_shr((-places) as u32).unwrap_or(if (a as i16) < 0 { -1 } else { 0 }) as u16
                };
                self.store(v)
            }
            0x04 => {
                let prev = self.font;
                let v = match a {
                    0 => prev,
                    1 | 4 => {
                        self.font = a;
                        prev
                    }
                    _ => 0,
                };
                self.store(v)
            }
            0x09 => {
                self.undo = Some(self.snapshot(true));
                self.store(1)
            }
            0x0a => {
                if let Some(data) = self.undo.clone() {
                    if self.restore(&data) {
                        return Ok(());
                    }
                }
                self.store(0)
            }
            0x0b => {
                if let Some(c) = char::from_u32(a as u32) {
                    self.out_char(c)?;
                }
                Ok(())
            }
            0x0c => {
                let v = match a {
                    32..=126 => 3,
                    _ if char::from_u32(a as u32).is_some() => 1,
                    _ => 0,
                };
                self.store(v)
            }
            0x0d | 0x0e => Ok(()),
            _ => Err(format!("illegal EXT opcode {op:#x}")),
        }
    }
}

/// Bounds-checked big-endian reader for snapshots.
struct Cursor<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cursor<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.bytes(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.bytes(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use std::panic::catch_unwind;
    use std::string::ToString;

    const ADVENT: &[u8] = include_bytes!("../data/advent.z5");

    /// Boot a story from memory: a `Vec<u8>` is a `ReadAt`, so the machine pages it
    /// exactly as it would a file on the card.
    fn boot(story: &[u8]) -> Machine<Vec<u8>> {
        Machine::new(story.to_vec()).expect("story loads")
    }

    fn dynmem(m: &Machine<Vec<u8>>) -> &[u8] {
        &m.core.dynmem
    }

    /// Run until the machine wants a line, collecting output.
    fn run_to_prompt(m: &mut Machine<Vec<u8>>) -> String {
        let mut out = String::new();
        for _ in 0..1000 {
            match m.run(20_000) {
                Step::Budget => out.push_str(&m.take_output()),
                Step::WaitLine => {
                    out.push_str(&m.take_output());
                    return out;
                }
                other => panic!("unexpected {other:?} with output {out:?}"),
            }
        }
        panic!("never reached a prompt: {out:?}");
    }

    fn command(m: &mut Machine<Vec<u8>>, line: &str) -> String {
        m.input(line);
        run_to_prompt(m)
    }

    // -- hand-assembled stories -------------------------------------------------------

    fn zstring(s: &str) -> Vec<u8> {
        let mut z: Vec<u8> = s.bytes().map(|b| if b == b' ' { 0 } else { b - b'a' + 6 }).collect();
        while !z.len().is_multiple_of(3) {
            z.push(5);
        }
        let words = z.len() / 3;
        let mut out = Vec::new();
        for (k, ch) in z.chunks(3).enumerate() {
            let mut w = ((ch[0] as u16) << 10) | ((ch[1] as u16) << 5) | ch[2] as u16;
            if k + 1 == words {
                w |= 0x8000;
            }
            out.extend_from_slice(&w.to_be_bytes());
        }
        out
    }

    /// A v3 story: dictionary at 0x40, objects at 0x80, globals at 0x100, text buffer at
    /// 0x200, parse buffer at 0x240, static memory from 0x300, code at 0x400.
    fn tiny_story(code: &[u8], initial_pc: u16) -> Vec<u8> {
        let mut s = vec![0u8; 0x400 + code.len()];
        s[0] = 3;
        s[0x04..0x06].copy_from_slice(&0x400u16.to_be_bytes());
        s[0x06..0x08].copy_from_slice(&initial_pc.to_be_bytes());
        s[0x08..0x0a].copy_from_slice(&0x40u16.to_be_bytes());
        s[0x0a..0x0c].copy_from_slice(&0x80u16.to_be_bytes());
        s[0x0c..0x0e].copy_from_slice(&0x100u16.to_be_bytes());
        s[0x0e..0x10].copy_from_slice(&0x300u16.to_be_bytes());
        s[0x18..0x1a].copy_from_slice(&0x2e0u16.to_be_bytes());
        let half = (s.len() / 2) as u16;
        s[0x1a..0x1c].copy_from_slice(&half.to_be_bytes());
        // Dictionary: no separators, 7-byte entries, "look" and "quit".
        s[0x40] = 0;
        s[0x41] = 7;
        s[0x42..0x44].copy_from_slice(&2u16.to_be_bytes());
        s[0x44..0x48].copy_from_slice(&zstring("look")[..4]);
        s[0x4b..0x4f].copy_from_slice(&zstring("quit")[..4]);
        s[0x200] = 40;
        s[0x240] = 10;
        s[0x400..].copy_from_slice(code);
        s
    }

    #[test]
    fn tiny_story_prints_reads_and_quits() {
        let mut code = vec![0xb2];
        code.extend(zstring("hello world"));
        code.push(0xbb); // new_line
        code.extend_from_slice(&[0xe4, 0x0f, 0x02, 0x00, 0x02, 0x40]); // sread 0x200 0x240
        code.push(0xb2);
        code.extend(zstring("bye"));
        code.push(0xbb);
        code.push(0xba); // quit
        let mut m = boot(&tiny_story(&code, 0x400));
        assert_eq!(m.version(), 3);
        assert_eq!(m.run(100), Step::WaitLine);
        assert_eq!(m.take_output(), "hello world\n");
        assert_eq!(m.run(100), Step::WaitLine);
        m.input("Quit  look");
        assert_eq!(m.run(100), Step::Halt);
        assert!(m.halted());
        assert_eq!(m.take_output(), "Quit  look\nbye\n");
        // Text buffer holds the lower-cased line; parse buffer has two matched words.
        assert_eq!(&dynmem(&m)[0x201..0x20c], b"quit  look\0");
        assert_eq!(dynmem(&m)[0x241], 2);
        assert_eq!(be16(dynmem(&m), 0x242), 0x4b);
        assert_eq!(dynmem(&m)[0x244], 4);
        assert_eq!(dynmem(&m)[0x245], 1);
        assert_eq!(be16(dynmem(&m), 0x246), 0x44);
        assert_eq!(dynmem(&m)[0x249], 7);
        assert_eq!(m.run(100), Step::Halt);
    }

    #[test]
    fn deep_recursion_is_an_error() {
        // Routine at 0x400 with no locals that calls itself forever.
        let code = [0x00, 0xe0, 0x3f, 0x02, 0x00, 0x00];
        let mut m = boot(&tiny_story(&code, 0x401));
        match m.run(10_000) {
            Step::Error(e) => assert!(e.contains("call stack"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(m.halted());
    }

    #[test]
    fn jump_outside_memory_is_an_error() {
        let mut m = boot(&tiny_story(&[0x8c, 0x7f, 0xff], 0x400));
        assert!(matches!(m.run(10), Step::Error(_)));
        let mut m = boot(&tiny_story(&[0x8c, 0x80, 0x00], 0x400));
        assert!(matches!(m.run(10), Step::Error(_)));
    }

    #[test]
    fn stack_overflow_is_an_error() {
        // push 1; jump back to the push (offset -4 from the byte after the jump).
        let code = [0xe8, 0x7f, 0x01, 0x8c, 0xff, 0xfc];
        let mut m = boot(&tiny_story(&code, 0x400));
        match m.run(100_000) {
            Step::Error(e) => assert!(e.contains("stack overflow"), "{e}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_bad_stories() {
        assert!(Machine::new(vec![3u8; MAX_STORY + 1]).is_err());
        assert!(Machine::new(vec![3u8; 10]).is_err());
        let mut s = tiny_story(&[0xba], 0x400);
        s[0] = 6;
        assert!(Machine::new(s.clone()).is_err());
        s[0] = 1;
        assert!(Machine::new(s.clone()).is_err());
        s[0] = 3;
        s[0x0e] = 0xff;
        s[0x0f] = 0xff;
        assert!(Machine::new(s).is_err());
        // More dynamic memory than the reader keeps resident is refused with a reason.
        let mut big = tiny_story(&[0xba], 0x400);
        big.resize(70 * 1024, 0);
        big[0x0e] = 0xf0;
        big[0x0f] = 0x00;
        let err = Machine::new(big).err().expect("refused");
        assert!(err.contains("48 KB"), "{err}");
    }

    #[test]
    fn static_memory_is_paged_from_the_source() {
        // advent.z5 is 138 KB; only its dynamic memory is resident.
        let mut m = boot(ADVENT);
        assert!(m.core.dynmem.len() <= MAX_DYN);
        assert_eq!(m.core.len, ADVENT.len());
        let static_base = m.core.static_base;
        let at = m.attached();
        // Reads across the whole file agree with the bytes, including across sector edges.
        for a in [static_base, SECTOR - 1, SECTOR, 7 * SECTOR + 511, ADVENT.len() - 2, ADVENT.len() - 1] {
            if a >= static_base {
                assert_eq!(at.rb(a).unwrap(), ADVENT[a], "byte {a:#x}");
            }
        }
        assert_eq!(at.rw(ADVENT.len() - 2).unwrap(), be16(ADVENT, ADVENT.len() - 2));
        assert!(at.rb(ADVENT.len()).is_err());
        assert!(at.rw(ADVENT.len() - 1).is_err());
        // A source that fails to read is an error, not a panic.
        struct Flaky(Vec<u8>);
        impl ReadAt for Flaky {
            fn len(&self) -> u64 {
                self.0.len() as u64
            }
            fn read_at(&self, offset: u64, buf: &mut [u8]) -> quire_fs::FsResult<usize> {
                if offset >= 0x8000 {
                    return Err(quire_fs::FsError::Io(String::from("card gone")));
                }
                self.0.as_slice().read_at(offset, buf)
            }
        }
        let mut m = Machine::new(Flaky(ADVENT.to_vec())).expect("loads");
        let mut saw_error = false;
        for _ in 0..200 {
            match m.run(5_000) {
                Step::WaitLine => m.input("east"),
                Step::WaitChar => m.input_char(13),
                Step::Budget => {}
                Step::Error(_) => {
                    saw_error = true;
                    break;
                }
                Step::Halt => break,
            }
        }
        assert!(saw_error || m.halted(), "a failing read ends the run cleanly");
    }

    /// A small deterministic generator for the fuzz tests.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
    }

    #[test]
    fn random_stories_never_panic() {
        let mut g = Lcg(1);
        for round in 0..200 {
            let len = 64 + (g.next() % 4096) as usize;
            let mut s: Vec<u8> = (0..len).map(|_| g.next() as u8).collect();
            s[0] = [3, 4, 5, 7, 8][round % 5];
            if round % 2 == 0 {
                // Plausible memory map so the machine actually runs the garbage.
                s[0x0e] = 0;
                s[0x0f] = 0x40;
                s[0x06] = 0;
                s[0x07] = 0x40;
            }
            let r = catch_unwind(move || {
                if let Ok(mut m) = Machine::new(s) {
                    for _ in 0..8 {
                        match m.run(5_000) {
                            Step::WaitLine => m.input("look"),
                            Step::WaitChar => m.input_char(13),
                            Step::Budget => {}
                            Step::Halt | Step::Error(_) => break,
                        }
                    }
                    m.take_output();
                    m.status_line();
                    m.upper_window();
                    let snap = m.save();
                    m.restore(&snap);
                    m.restart();
                }
            });
            assert!(r.is_ok(), "panic on round {round}");
        }
    }

    #[test]
    fn corrupted_real_stories_never_panic() {
        let mut g = Lcg(7);
        for round in 0..60 {
            let mut s = ADVENT.to_vec();
            let _ = round;
            for _ in 0..1 + g.next() % 64 {
                let at = 0x40 + (g.next() as usize % (s.len() - 0x40));
                s[at] = g.next() as u8;
            }
            let r = catch_unwind(move || {
                if let Ok(mut m) = Machine::new(s) {
                    for _ in 0..12 {
                        match m.run(5_000) {
                            Step::WaitLine => m.input("get all"),
                            Step::WaitChar => m.input_char(13),
                            Step::Budget => {}
                            Step::Halt | Step::Error(_) => break,
                        }
                    }
                    m.take_output();
                    m.status_line();
                }
            });
            assert!(r.is_ok(), "panic on round {round}");
        }
    }

    #[test]
    fn corrupted_snapshots_never_panic() {
        let mut m = boot(ADVENT);
        run_to_prompt(&mut m);
        let snap = m.save();
        let mut g = Lcg(3);
        for _ in 0..100 {
            let mut s = snap.clone();
            let cut = g.next() as usize % s.len();
            if g.next().is_multiple_of(2) {
                s.truncate(cut);
            } else {
                s[cut] = g.next() as u8;
            }
            let mut m2 = boot(ADVENT);
            run_to_prompt(&mut m2);
            assert!(catch_unwind(move || {
                m2.restore(&s);
                m2.run(1000);
            })
            .is_ok());
        }
    }

    // -- real stories -----------------------------------------------------------------

    #[test]
    fn verify_and_random() {
        let mut m = boot(ADVENT);
        assert!(m.attached().verify());
        m.core.dynmem[0x1c] ^= 1;
        assert!(!m.attached().verify());
        let m = &mut m.core;
        m.random(-5);
        let seq: Vec<u16> = (0..7).map(|_| m.random(10)).collect();
        assert_eq!(seq, [1, 2, 3, 4, 5, 1, 2]);
        m.random(-5000);
        let a: Vec<u16> = (0..20).map(|_| m.random(100)).collect();
        m.random(-5000);
        let b: Vec<u16> = (0..20).map(|_| m.random(100)).collect();
        assert_eq!(a, b);
        assert!(a.iter().all(|&v| (1..=100).contains(&v)));
        m.seed(1);
        let c: Vec<u16> = (0..20).map(|_| m.random(100)).collect();
        assert_ne!(a, c);
    }

    #[test]
    fn advent_boots_and_walks() {
        let mut m = boot(ADVENT);
        assert_eq!(m.version(), 5);
        let banner = run_to_prompt(&mut m);
        assert!(banner.contains("ADVENTURE"), "{banner}");
        assert!(banner.contains("At End Of Road"), "{banner}");
        assert!(m.status_line().is_none());
        // The upper window holds the room name and score.
        let upper = m.upper_window();
        assert!(upper.iter().any(|l| l.contains("At End Of Road")), "{upper:?}");
        assert!(command(&mut m, "east").contains("Inside Building"));
        let out = command(&mut m, "get all");
        assert!(out.contains("brass lantern: Taken."), "{out}");
        assert!(out.contains("small bottle: Taken."), "{out}");
        assert!(command(&mut m, "west").contains("At End Of Road"));
        assert!(command(&mut m, "south").contains("In A Valley"));
        assert!(command(&mut m, "south").contains("At Slit In Streambed"));
        assert!(command(&mut m, "south").contains("Outside Grate"));
        let out = command(&mut m, "unlock grate with keys");
        assert!(out.contains("unlock"), "{out}");
        command(&mut m, "open it");
        assert!(command(&mut m, "down").contains("Below the Grate"));
        assert!(command(&mut m, "west").contains("In Cobble Crawl"));
        assert!(command(&mut m, "west").contains("It is pitch dark"));
        let out = command(&mut m, "turn on lamp");
        assert!(out.contains("In Debris Room"), "{out}");
        assert!(out.contains("Magic word XYZZY"), "{out}");
        assert!(command(&mut m, "xyzzy").contains("Inside Building"));
        let upper = m.upper_window();
        assert!(upper.iter().any(|l| l.contains("Inside Building")), "{upper:?}");
    }

    #[test]
    fn advent_menu_uses_read_char() {
        let mut m = boot(ADVENT);
        run_to_prompt(&mut m);
        m.input("help");
        let mut out = String::new();
        loop {
            match m.run(20_000) {
                Step::Budget => out.push_str(&m.take_output()),
                Step::WaitChar => break,
                other => panic!("{other:?}"),
            }
        }
        out.push_str(&m.take_output());
        let upper = m.upper_window();
        assert!(upper.iter().any(|l| l.contains("About Adventure") || l.contains("N = next")), "{upper:?}\n{out}");
        m.input_char(13);
        let mut out = String::new();
        loop {
            match m.run(20_000) {
                Step::Budget => out.push_str(&m.take_output()),
                Step::WaitChar => break,
                other => panic!("{other:?}"),
            }
        }
        out.push_str(&m.take_output());
        assert!(out.contains("I know of places, actions, and things."), "{out}");
        // Any key returns to the menu, then Q leaves it; back at the prompt.
        m.input_char(b' ' as u16);
        assert_eq!(m.run(20_000), Step::WaitChar);
        m.take_output();
        m.input_char(b'q' as u16);
        let out = run_to_prompt(&mut m);
        assert!(out.contains("At End Of Road") || m.upper_window().iter().any(|l| l.contains("At End Of Road")), "{out}");
    }

    #[test]
    fn advent_in_game_save_undo_restore() {
        let mut m = boot(ADVENT);
        run_to_prompt(&mut m);
        command(&mut m, "east");
        let out = command(&mut m, "save");
        assert!(out.contains("Ok"), "{out}");
        let slot = m.take_save().unwrap();
        assert!(command(&mut m, "west").contains("At End Of Road"));
        let out = command(&mut m, "undo");
        assert!(out.contains("Inside Building"), "{out}");
        assert!(command(&mut m, "west").contains("At End Of Road"));
        m.offer_restore(slot);
        let out = command(&mut m, "restore");
        assert!(out.contains("Ok"), "{out}");
        assert!(command(&mut m, "look").contains("Inside Building"));
        let out = command(&mut m, "restart");
        assert!(out.contains("Are you sure"), "{out}");
        let out = command(&mut m, "yes");
        assert!(out.contains("At End Of Road"), "{out}");
    }

    #[test]
    fn text_roundtrip_through_dictionary() {
        let mut m = boot(ADVENT);
        let m = m.attached();
        let dict = m.rw(0x08).unwrap() as usize;
        let addr = m.dict_lookup(dict, b"lantern").unwrap();
        assert_ne!(addr, 0);
        let mut s = String::new();
        m.decode_text(addr as usize, &mut s, false).unwrap();
        assert_eq!(s, "lantern");
        assert_eq!(m.dict_lookup(dict, b"zzzzzz").unwrap(), 0);
        let quoted = char_to_zscii('é').map(|z| m.zscii_to_char(z as u16));
        assert_eq!(quoted, Some(Some('é')));
        assert_eq!("x".to_string(), m.zscii_to_char(b'x' as u16).unwrap().to_string());
    }
}
