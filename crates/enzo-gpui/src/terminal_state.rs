//! VT100/xterm terminal state machine built on the `vte` parser.
//!
//! [`Terminal`] processes raw PTY bytes and maintains a scrollable cell grid.

use vte::{Params, Parser, Perform};

/// Columns used when no PTY size is negotiated.
pub const DEFAULT_COLS: u16 = 220;
/// Rows used when no PTY size is negotiated.
pub const DEFAULT_ROWS: u16 = 50;

/// Foreground or background colour of a terminal cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    /// Inherit the theme default (Matrix-green fg / dark bg).
    #[default]
    Default,
    /// One of the 256 standard xterm palette entries.
    Indexed(u8),
    /// 24-bit RGB colour (r, g, b).
    Rgb(u8, u8, u8),
}

/// Text attributes applied to a single cell.
#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Bold / bright text.
    pub bold: bool,
    /// Underline decoration.
    pub underline: bool,
    /// Swap fg/bg colours.
    pub reverse: bool,
}

/// One character-cell in the terminal grid.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// The Unicode scalar rendered in this cell (space = empty).
    pub ch: char,
    /// Text attributes.
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: Style::default(),
        }
    }
}

/// One OSC-133 semantic command block (command + its output + exit status).
#[derive(Clone, Debug, Default)]
pub struct Block {
    /// The command line the user ran (text between the `B` and `C` marks).
    pub command: String,
    /// The command's output (text between the `C` and `D` marks).
    pub output: String,
    /// Exit code reported by the `D` mark, once finished.
    pub exit: Option<i32>,
    /// True until the `D` mark arrives.
    pub running: bool,
}

/// OSC-133 semantic phase of the byte stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before any prompt, or after a command finished (prompt text).
    Idle,
    /// Inside the prompt (`A`..`B`).
    Prompt,
    /// The user's command line (`B`..`C`).
    Command,
    /// Command output (`C`..`D`).
    Output,
}

// ── Public terminal API ──────────────────────────────────────────────────────

/// VT100/xterm terminal emulator.
///
/// Call [`Terminal::process`] with raw bytes from the PTY.  The resulting grid
/// is available through [`Terminal::cells`].
pub struct Terminal {
    parser: Parser,
    inner: TerminalInner,
}

impl Terminal {
    /// Create a new terminal with `cols` columns and `rows` rows.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        let bottom = rows.saturating_sub(1);
        Self {
            parser: Parser::new(),
            inner: TerminalInner {
                cols,
                rows,
                grid: vec![Cell::default(); cols as usize * rows as usize],
                cursor_col: 0,
                cursor_row: 0,
                saved_col: 0,
                saved_row: 0,
                current_style: Style::default(),
                scroll_top: 0,
                scroll_bottom: bottom,
                phase: Phase::Idle,
                blocks: Vec::new(),
                alt_screen: false,
            },
        }
    }

    /// The OSC-133 semantic command blocks parsed from the stream so far.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.inner.blocks
    }

    /// Whether a full-screen (alternate-screen) program is currently active.
    #[must_use]
    pub fn alt_screen(&self) -> bool {
        self.inner.alt_screen
    }

    /// Feed raw PTY bytes into the state machine.
    pub fn process(&mut self, data: &[u8]) {
        for &b in data {
            self.parser.advance(&mut self.inner, b);
        }
    }

    /// The complete cell grid, in row-major order (`row * cols + col`).
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.inner.grid
    }

    /// Current cursor position as `(col, row)`, 0-indexed.
    #[must_use]
    pub fn cursor(&self) -> (u16, u16) {
        (self.inner.cursor_col, self.inner.cursor_row)
    }

    /// Number of columns.
    #[must_use]
    pub fn cols(&self) -> u16 {
        self.inner.cols
    }

    /// Number of rows.
    #[must_use]
    pub fn rows(&self) -> u16 {
        self.inner.rows
    }
}

// ── Internal state ───────────────────────────────────────────────────────────

struct TerminalInner {
    cols: u16,
    rows: u16,
    grid: Vec<Cell>,
    cursor_col: u16,
    cursor_row: u16,
    saved_col: u16,
    saved_row: u16,
    current_style: Style,
    scroll_top: u16,
    scroll_bottom: u16,
    // ── OSC-133 semantic blocks ──
    phase: Phase,
    blocks: Vec<Block>,
    /// True while a full-screen (alternate-screen) program is active; the client
    /// renders the raw grid then instead of command blocks.
    alt_screen: bool,
}

impl TerminalInner {
    /// Mutable handle to the in-progress block (the last one), if any.
    fn cur_block(&mut self) -> Option<&mut Block> {
        self.blocks.last_mut()
    }

    /// Route a printed char into the active block's command/output buffer.
    fn block_print(&mut self, ch: char) {
        // Don't capture full-screen TUI repaints into a block (rendered as grid).
        if self.alt_screen {
            return;
        }
        match self.phase {
            Phase::Command => {
                if let Some(b) = self.cur_block() {
                    b.command.push(ch);
                }
            }
            Phase::Output => {
                if let Some(b) = self.cur_block() {
                    b.output.push(ch);
                }
            }
            Phase::Idle | Phase::Prompt => {}
        }
    }

    /// Handle an OSC-133 semantic mark (`A`/`B`/`C`/`D[;exit]`).
    fn osc_133(&mut self, kind: u8, exit: Option<i32>) {
        match kind {
            b'A' => {
                // New prompt → start a fresh block.
                self.phase = Phase::Prompt;
                self.blocks.push(Block {
                    running: true,
                    ..Block::default()
                });
            }
            b'B' => self.phase = Phase::Command,
            b'C' => self.phase = Phase::Output,
            b'D' => {
                if let Some(b) = self.cur_block() {
                    b.exit = exit;
                    b.running = false;
                    // Tidy the captured command (drop trailing newline echo).
                    let trimmed = b.command.trim_end().to_owned();
                    b.command = trimmed;
                }
                self.phase = Phase::Idle;
            }
            _ => {}
        }
    }
}

impl TerminalInner {
    fn idx(&self, col: u16, row: u16) -> usize {
        row as usize * self.cols as usize + col as usize
    }

    fn put_char(&mut self, ch: char) {
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.linefeed();
        }
        let i = self.idx(self.cursor_col, self.cursor_row);
        self.grid[i] = Cell {
            ch,
            style: self.current_style,
        };
        self.cursor_col = self.cursor_col.saturating_add(1);
    }

    fn linefeed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
        }
    }

    fn scroll_up(&mut self, n: u16) {
        let top = self.scroll_top as usize;
        let bot = self.scroll_bottom as usize;
        let cols = self.cols as usize;
        for _ in 0..n {
            for r in top..bot {
                for c in 0..cols {
                    self.grid[r * cols + c] = self.grid[(r + 1) * cols + c];
                }
            }
            for c in 0..cols {
                self.grid[bot * cols + c] = Cell::default();
            }
        }
    }

    fn scroll_down(&mut self, n: u16) {
        let top = self.scroll_top as usize;
        let bot = self.scroll_bottom as usize;
        let cols = self.cols as usize;
        for _ in 0..n {
            for r in (top + 1..=bot).rev() {
                for c in 0..cols {
                    self.grid[r * cols + c] = self.grid[(r - 1) * cols + c];
                }
            }
            for c in 0..cols {
                self.grid[top * cols + c] = Cell::default();
            }
        }
    }

    fn erase_cells(&mut self, start_col: u16, end_col: u16, row: u16) {
        for c in start_col..end_col {
            let i = self.idx(c, row);
            self.grid[i] = Cell::default();
        }
    }

    fn erase_line_right(&mut self) {
        self.erase_cells(self.cursor_col, self.cols, self.cursor_row);
    }

    fn erase_line_left(&mut self) {
        self.erase_cells(0, self.cursor_col.saturating_add(1), self.cursor_row);
    }

    fn erase_line(&mut self) {
        self.erase_cells(0, self.cols, self.cursor_row);
    }

    fn erase_screen_below(&mut self) {
        self.erase_line_right();
        let row = self.cursor_row;
        for r in (row + 1)..self.rows {
            self.erase_cells(0, self.cols, r);
        }
    }

    fn erase_screen_above(&mut self) {
        self.erase_line_left();
        let row = self.cursor_row;
        for r in 0..row {
            self.erase_cells(0, self.cols, r);
        }
    }

    fn erase_screen(&mut self) {
        self.grid.fill(Cell::default());
    }

    fn insert_lines(&mut self, n: u16) {
        let top = self.cursor_row as usize;
        let bot = self.scroll_bottom as usize;
        let cols = self.cols as usize;
        let n = n as usize;
        for r in (top..=bot.saturating_sub(n)).rev() {
            for c in 0..cols {
                self.grid[(r + n) * cols + c] = self.grid[r * cols + c];
            }
        }
        for r in top..(top + n).min(bot + 1) {
            for c in 0..cols {
                self.grid[r * cols + c] = Cell::default();
            }
        }
    }

    fn delete_lines(&mut self, n: u16) {
        let top = self.cursor_row as usize;
        let bot = self.scroll_bottom as usize;
        let cols = self.cols as usize;
        let n = n as usize;
        for r in top..=bot.saturating_sub(n) {
            for c in 0..cols {
                self.grid[r * cols + c] = self.grid[(r + n) * cols + c];
            }
        }
        for r in (bot + 1).saturating_sub(n)..=bot {
            for c in 0..cols {
                self.grid[r * cols + c] = Cell::default();
            }
        }
    }

    fn delete_chars(&mut self, n: u16) {
        let row = self.cursor_row as usize;
        let col = self.cursor_col as usize;
        let cols = self.cols as usize;
        let n = n as usize;
        for c in col..(cols.saturating_sub(n)) {
            self.grid[row * cols + c] = self.grid[row * cols + c + n];
        }
        for c in cols.saturating_sub(n)..cols {
            self.grid[row * cols + c] = Cell::default();
        }
    }

    fn apply_sgr(&mut self, params: &Params) {
        let mut iter = params.iter();
        while let Some(p) = iter.next() {
            // SGR params > 255 are ignored; all standard codes fit in u8.
            let n = u8::try_from(p.first().copied().unwrap_or(0)).unwrap_or(0);
            match n {
                0 => self.current_style = Style::default(),
                1 => self.current_style.bold = true,
                4 => self.current_style.underline = true,
                7 => self.current_style.reverse = true,
                22 => self.current_style.bold = false,
                24 => self.current_style.underline = false,
                27 => self.current_style.reverse = false,
                30..=37 => self.current_style.fg = Color::Indexed(n - 30),
                38 => self.apply_extended_color(p, &mut iter, true),
                39 => self.current_style.fg = Color::Default,
                40..=47 => self.current_style.bg = Color::Indexed(n - 40),
                48 => self.apply_extended_color(p, &mut iter, false),
                49 => self.current_style.bg = Color::Default,
                90..=97 => self.current_style.fg = Color::Indexed(n - 90 + 8),
                100..=107 => self.current_style.bg = Color::Indexed(n - 100 + 8),
                _ => {}
            }
        }
    }

    fn apply_extended_color(
        &mut self,
        first_p: &[u16],
        iter: &mut dyn Iterator<Item = &[u16]>,
        is_fg: bool,
    ) {
        // Handle both `38;5;idx` (semicolon) and `38:5:idx` (colon / sub-params).
        let inline = first_p.len() > 1;
        let mode = if inline {
            first_p.get(1).copied().unwrap_or(0)
        } else {
            iter.next().and_then(|p| p.first().copied()).unwrap_or(0)
        };

        let u8_param = |raw: u16| u8::try_from(raw).unwrap_or(0);
        let next_u8 = |it: &mut dyn Iterator<Item = &[u16]>| {
            u8_param(it.next().and_then(|p| p.first().copied()).unwrap_or(0))
        };

        let color = match mode {
            5 => {
                let idx = if inline {
                    u8_param(first_p.get(2).copied().unwrap_or(0))
                } else {
                    next_u8(iter)
                };
                Color::Indexed(idx)
            }
            2 => {
                let (r, g, b) = if inline {
                    (
                        u8_param(first_p.get(2).copied().unwrap_or(0)),
                        u8_param(first_p.get(3).copied().unwrap_or(0)),
                        u8_param(first_p.get(4).copied().unwrap_or(0)),
                    )
                } else {
                    (next_u8(iter), next_u8(iter), next_u8(iter))
                };
                Color::Rgb(r, g, b)
            }
            _ => return,
        };

        if is_fg {
            self.current_style.fg = color;
        } else {
            self.current_style.bg = color;
        }
    }

    fn clamp_cursor(&mut self) {
        self.cursor_col = self.cursor_col.min(self.cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(self.rows.saturating_sub(1));
    }
}

// ── vte Perform implementation ───────────────────────────────────────────────

fn p1(params: &Params, default: u16) -> u16 {
    params
        .iter()
        .next()
        .and_then(|p| p.first().copied())
        .map_or(default, |n| if n == 0 { default } else { n })
}

fn p2(params: &Params, d1: u16, d2: u16) -> (u16, u16) {
    let mut it = params.iter();
    let a = it
        .next()
        .and_then(|p| p.first().copied())
        .map_or(d1, |n| if n == 0 { d1 } else { n });
    let b = it
        .next()
        .and_then(|p| p.first().copied())
        .map_or(d2, |n| if n == 0 { d2 } else { n });
    (a, b)
}

impl Perform for TerminalInner {
    fn print(&mut self, c: char) {
        self.block_print(c);
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => {
                // BS — also erase from the captured command line.
                if self.phase == Phase::Command
                    && let Some(b) = self.cur_block()
                {
                    b.command.pop();
                }
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            0x09 => {
                // HT — advance to next 8-column tab stop
                let next = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = next.min(self.cols.saturating_sub(1));
            }
            0x0A..=0x0C => {
                // LF / VT / FF — newline in command output.
                if self.phase == Phase::Output
                    && let Some(b) = self.cur_block()
                {
                    b.output.push('\n');
                }
                self.linefeed();
            }
            0x0D => self.cursor_col = 0, // CR
            _ => {}
        }
    }

    /// OSC dispatch — capture OSC 133 semantic prompt marks; ignore the rest
    /// (titles, hyperlinks, etc.) for now.
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // OSC 133 ; <kind> [; <exit>]
        if params.first().map(|p| *p) != Some(b"133".as_slice()) {
            return;
        }
        let Some(kind) = params.get(1).and_then(|p| p.first().copied()) else {
            return;
        };
        let exit = params
            .get(2)
            .and_then(|p| std::str::from_utf8(p).ok())
            .and_then(|s| s.trim().parse::<i32>().ok());
        self.osc_133(kind, exit);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, c: char) {
        match (c, intermediates) {
            ('A', _) => {
                let n = p1(params, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            ('B', _) => {
                let n = p1(params, 1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
            }
            ('C', _) => {
                let n = p1(params, 1);
                self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
            }
            ('D', _) => {
                let n = p1(params, 1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            ('E', _) => {
                let n = p1(params, 1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
                self.cursor_col = 0;
            }
            ('F', _) => {
                let n = p1(params, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
            }
            ('G', _) => {
                let n = p1(params, 1);
                self.cursor_col = n.saturating_sub(1).min(self.cols.saturating_sub(1));
            }
            ('H' | 'f', _) => {
                let (row, col) = p2(params, 1, 1);
                self.cursor_row = row.saturating_sub(1).min(self.rows.saturating_sub(1));
                self.cursor_col = col.saturating_sub(1).min(self.cols.saturating_sub(1));
            }
            ('J', _) => match p1(params, 0) {
                0 => self.erase_screen_below(),
                1 => self.erase_screen_above(),
                _ => self.erase_screen(),
            },
            ('K', _) => match p1(params, 0) {
                0 => self.erase_line_right(),
                1 => self.erase_line_left(),
                _ => self.erase_line(),
            },
            ('L', _) => {
                let n = p1(params, 1);
                self.insert_lines(n);
            }
            ('M', _) => {
                let n = p1(params, 1);
                self.delete_lines(n);
            }
            ('P', _) => {
                let n = p1(params, 1);
                self.delete_chars(n);
            }
            ('S', _) => {
                let n = p1(params, 1);
                self.scroll_up(n);
            }
            ('T', _) => {
                let n = p1(params, 1);
                self.scroll_down(n);
            }
            ('X', _) => {
                // Erase n characters at cursor (fill with spaces).
                let n = p1(params, 1);
                let end = (self.cursor_col + n).min(self.cols);
                self.erase_cells(self.cursor_col, end, self.cursor_row);
            }
            ('d', _) => {
                let n = p1(params, 1);
                self.cursor_row = n.saturating_sub(1).min(self.rows.saturating_sub(1));
            }
            ('m', _) => self.apply_sgr(params),
            ('r', _) => {
                let (top, bot) = p2(params, 1, self.rows);
                self.scroll_top = top.saturating_sub(1).min(self.rows.saturating_sub(1));
                self.scroll_bottom = bot.saturating_sub(1).min(self.rows.saturating_sub(1));
                self.cursor_col = 0;
                self.cursor_row = 0;
            }
            ('h' | 'l', [b'?']) => {
                // Alternate-screen private modes (1049/1047/47): track so the
                // client falls back to grid rendering for full-screen TUIs.
                if matches!(p1(params, 0), 1049 | 1047 | 47) {
                    self.alt_screen = c == 'h';
                }
            }
            _ => {} // other private-mode h/l — no-op
        }
        self.clamp_cursor();
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'c' => {
                // RIS — full reset.
                let cols = self.cols;
                let rows = self.rows;
                self.grid = vec![Cell::default(); cols as usize * rows as usize];
                self.cursor_col = 0;
                self.cursor_row = 0;
                self.saved_col = 0;
                self.saved_row = 0;
                self.current_style = Style::default();
                self.scroll_top = 0;
                self.scroll_bottom = rows.saturating_sub(1);
            }
            b'7' => {
                self.saved_col = self.cursor_col;
                self.saved_row = self.cursor_row;
            }
            b'8' => {
                self.cursor_col = self.saved_col;
                self.cursor_row = self.saved_row;
            }
            _ => {}
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(cols, rows)
    }

    fn cell(t: &Terminal, col: u16, row: u16) -> Cell {
        t.cells()[row as usize * t.cols() as usize + col as usize]
    }

    #[test]
    fn print_advances_cursor() {
        let mut t = term(10, 5);
        t.process(b"hi");
        assert_eq!(cell(&t, 0, 0).ch, 'h');
        assert_eq!(cell(&t, 1, 0).ch, 'i');
        assert_eq!(t.cursor(), (2, 0));
    }

    /// `ESC ] <body> BEL`
    fn osc(body: &str) -> Vec<u8> {
        let mut v = vec![0x1b, b']'];
        v.extend(body.bytes());
        v.push(0x07);
        v
    }

    #[test]
    fn osc133_extracts_command_block() {
        let mut t = term(80, 24);
        let mut s = Vec::new();
        s.extend(osc("133;A")); // prompt start → new block
        s.extend(b"root@vm:~# "); // prompt text (ignored)
        s.extend(osc("133;B")); // command start
        s.extend(b"echo hi"); // command echo
        s.extend(b"\r\n");
        s.extend(osc("133;C")); // pre-exec → output
        s.extend(b"hi\r\n"); // output
        s.extend(osc("133;D;0")); // finished, exit 0
        t.process(&s);

        let blocks = t.blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].command, "echo hi");
        assert_eq!(blocks[0].output, "hi\n");
        assert_eq!(blocks[0].exit, Some(0));
        assert!(!blocks[0].running);
    }

    #[test]
    fn alt_screen_toggles_and_suppresses_block_capture() {
        let mut t = term(80, 24);
        t.process(&osc("133;A"));
        t.process(&osc("133;B"));
        t.process(b"vim");
        t.process(&osc("133;C")); // output phase
        assert!(!t.alt_screen());
        t.process(b"\x1b[?1049h"); // enter alt screen
        assert!(t.alt_screen());
        t.process(b"~~~ editor junk ~~~"); // must NOT land in the block output
        t.process(b"\x1b[?1049l"); // leave alt screen
        assert!(!t.alt_screen());
        assert_eq!(t.blocks()[0].command, "vim");
        assert!(t.blocks()[0].output.is_empty());
    }

    #[test]
    fn osc133_nonzero_exit_and_running() {
        let mut t = term(80, 24);
        t.process(&osc("133;A"));
        t.process(&osc("133;B"));
        t.process(b"false");
        t.process(&osc("133;C"));
        // no D yet → still running
        assert!(t.blocks()[0].running);
        t.process(&osc("133;D;1"));
        assert_eq!(t.blocks()[0].exit, Some(1));
        assert!(!t.blocks()[0].running);
    }

    #[test]
    fn cr_resets_column() {
        let mut t = term(10, 5);
        t.process(b"abc\r");
        assert_eq!(t.cursor(), (0, 0));
    }

    #[test]
    fn lf_advances_row() {
        let mut t = term(10, 5);
        t.process(b"\n");
        assert_eq!(t.cursor(), (0, 1));
    }

    #[test]
    fn lf_at_bottom_scrolls() {
        let mut t = term(5, 3);
        t.process(b"A\r\nB\r\nC\r\n"); // third LF scrolls
        assert_eq!(cell(&t, 0, 0).ch, 'B');
        assert_eq!(cell(&t, 0, 1).ch, 'C');
    }

    #[test]
    fn backspace_moves_left() {
        let mut t = term(10, 5);
        t.process(b"ab\x08");
        assert_eq!(t.cursor(), (1, 0));
    }

    #[test]
    fn tab_moves_to_next_stop() {
        let mut t = term(20, 5);
        t.process(b"\t");
        assert_eq!(t.cursor().0, 8);
        t.process(b"\t");
        assert_eq!(t.cursor().0, 16);
    }

    #[test]
    fn csi_cursor_up() {
        let mut t = term(10, 10);
        t.process(b"\x1b[5B\x1b[2A"); // down 5, up 2
        assert_eq!(t.cursor(), (0, 3));
    }

    #[test]
    fn csi_cursor_down() {
        let mut t = term(10, 10);
        t.process(b"\x1b[3B");
        assert_eq!(t.cursor(), (0, 3));
    }

    #[test]
    fn csi_cursor_forward_back() {
        let mut t = term(20, 5);
        t.process(b"\x1b[10C\x1b[3D");
        assert_eq!(t.cursor(), (7, 0));
    }

    #[test]
    fn csi_cursor_next_preceding_line() {
        let mut t = term(20, 10);
        t.process(b"\x1b[5E\x1b[2F");
        assert_eq!(t.cursor(), (0, 3));
    }

    #[test]
    fn csi_cursor_position() {
        let mut t = term(20, 10);
        t.process(b"\x1b[3;5H");
        assert_eq!(t.cursor(), (4, 2));
    }

    #[test]
    fn csi_cursor_position_default() {
        let mut t = term(20, 10);
        t.process(b"\x1b[5;5H\x1b[H"); // move, then reset
        assert_eq!(t.cursor(), (0, 0));
    }

    #[test]
    fn csi_g_horizontal_abs() {
        let mut t = term(20, 5);
        t.process(b"\x1b[10G");
        assert_eq!(t.cursor().0, 9);
    }

    #[test]
    fn csi_d_vertical_abs() {
        let mut t = term(20, 10);
        t.process(b"\x1b[5d");
        assert_eq!(t.cursor().1, 4);
    }

    #[test]
    fn csi_erase_line_right() {
        let mut t = term(5, 3);
        t.process(b"abcde");
        t.process(b"\x1b[1;3H\x1b[K"); // cursor (3,1) → erase right
        // col 0 col 1 intact, col 2-4 erased
        assert_eq!(cell(&t, 0, 0).ch, 'a');
        assert_eq!(cell(&t, 1, 0).ch, 'b');
        assert_eq!(cell(&t, 2, 0).ch, ' ');
        assert_eq!(cell(&t, 3, 0).ch, ' ');
    }

    #[test]
    fn csi_erase_line_left() {
        let mut t = term(5, 3);
        t.process(b"abcde\x1b[1;4H\x1b[1K");
        assert_eq!(cell(&t, 0, 0).ch, ' ');
        assert_eq!(cell(&t, 1, 0).ch, ' ');
        assert_eq!(cell(&t, 2, 0).ch, ' ');
        assert_eq!(cell(&t, 3, 0).ch, ' ');
        assert_eq!(cell(&t, 4, 0).ch, 'e');
    }

    #[test]
    fn csi_erase_entire_line() {
        let mut t = term(5, 3);
        t.process(b"abcde\x1b[1;1H\x1b[2K");
        for c in 0..5 {
            assert_eq!(cell(&t, c, 0).ch, ' ');
        }
    }

    #[test]
    fn csi_erase_display_below() {
        let mut t = term(3, 3);
        t.process(b"aaa\r\nbbb\r\nccc\x1b[2;2H\x1b[J");
        assert_eq!(cell(&t, 0, 0).ch, 'a'); // untouched
        assert_eq!(cell(&t, 1, 1).ch, ' '); // erased from cursor
    }

    #[test]
    fn csi_erase_display_above() {
        let mut t = term(3, 3);
        t.process(b"aaa\r\nbbb\r\nccc\x1b[2;2H\x1b[1J");
        assert_eq!(cell(&t, 0, 0).ch, ' ');
        assert_eq!(cell(&t, 2, 2).ch, 'c'); // untouched
    }

    #[test]
    fn csi_erase_display_all() {
        let mut t = term(3, 3);
        t.process(b"aaa\r\nbbb\r\nccc\x1b[2J");
        for c in t.cells() {
            assert_eq!(c.ch, ' ');
        }
    }

    #[test]
    fn csi_scroll_up_s() {
        let mut t = term(3, 3);
        t.process(b"aaa\r\nbbb\r\nccc\x1b[S"); // scroll up 1
        assert_eq!(cell(&t, 0, 0).ch, 'b');
        assert_eq!(cell(&t, 0, 1).ch, 'c');
        assert_eq!(cell(&t, 0, 2).ch, ' ');
    }

    #[test]
    fn csi_scroll_down_t() {
        let mut t = term(3, 3);
        t.process(b"aaa\r\nbbb\r\nccc\x1b[T"); // scroll down 1
        assert_eq!(cell(&t, 0, 0).ch, ' ');
        assert_eq!(cell(&t, 0, 1).ch, 'a');
        assert_eq!(cell(&t, 0, 2).ch, 'b');
    }

    #[test]
    fn csi_insert_delete_lines() {
        let mut t = term(3, 4);
        t.process(b"aaa\r\nbbb\r\nccc\r\nddd\x1b[2;1H\x1b[L"); // insert line at row 2
        assert_eq!(cell(&t, 0, 0).ch, 'a');
        assert_eq!(cell(&t, 0, 1).ch, ' '); // inserted blank line
        assert_eq!(cell(&t, 0, 2).ch, 'b');
    }

    #[test]
    fn csi_delete_lines() {
        let mut t = term(3, 4);
        t.process(b"aaa\r\nbbb\r\nccc\r\nddd\x1b[2;1H\x1b[M"); // delete line at row 2
        assert_eq!(cell(&t, 0, 0).ch, 'a');
        assert_eq!(cell(&t, 0, 1).ch, 'c');
    }

    #[test]
    fn csi_delete_chars() {
        let mut t = term(5, 3);
        t.process(b"abcde\x1b[1;2H\x1b[2P"); // delete 2 chars at col 2
        assert_eq!(cell(&t, 0, 0).ch, 'a');
        assert_eq!(cell(&t, 1, 0).ch, 'd');
        assert_eq!(cell(&t, 2, 0).ch, 'e');
        assert_eq!(cell(&t, 3, 0).ch, ' ');
    }

    #[test]
    fn csi_erase_chars_x() {
        let mut t = term(5, 3);
        t.process(b"abcde\x1b[1;2H\x1b[2X");
        assert_eq!(cell(&t, 0, 0).ch, 'a');
        assert_eq!(cell(&t, 1, 0).ch, ' ');
        assert_eq!(cell(&t, 2, 0).ch, ' ');
        assert_eq!(cell(&t, 3, 0).ch, 'd');
    }

    #[test]
    fn sgr_reset() {
        let mut t = term(10, 5);
        t.process(b"\x1b[1m\x1b[31m"); // bold + red
        t.process(b"\x1b[0m"); // reset
        t.process(b"a");
        let c = cell(&t, 0, 0);
        assert!(!c.style.bold);
        assert_eq!(c.style.fg, Color::Default);
    }

    #[test]
    fn sgr_bold_underline_reverse() {
        let mut t = term(10, 5);
        t.process(b"\x1b[1;4;7ma");
        let c = cell(&t, 0, 0);
        assert!(c.style.bold);
        assert!(c.style.underline);
        assert!(c.style.reverse);
    }

    #[test]
    fn sgr_reset_attrs() {
        let mut t = term(10, 5);
        t.process(b"\x1b[1;4;7m\x1b[22;24;27ma");
        let c = cell(&t, 0, 0);
        assert!(!c.style.bold);
        assert!(!c.style.underline);
        assert!(!c.style.reverse);
    }

    #[test]
    fn sgr_ansi_fg_bg_colors() {
        let mut t = term(10, 5);
        t.process(b"\x1b[32;41ma"); // green fg, red bg
        let c = cell(&t, 0, 0);
        assert_eq!(c.style.fg, Color::Indexed(2));
        assert_eq!(c.style.bg, Color::Indexed(1));
    }

    #[test]
    fn sgr_bright_colors() {
        let mut t = term(10, 5);
        t.process(b"\x1b[92;103ma"); // bright green fg, bright yellow bg
        let c = cell(&t, 0, 0);
        assert_eq!(c.style.fg, Color::Indexed(10));
        assert_eq!(c.style.bg, Color::Indexed(11));
    }

    #[test]
    fn sgr_256_color() {
        let mut t = term(10, 5);
        t.process(b"\x1b[38;5;196m\x1b[48;5;21ma");
        let c = cell(&t, 0, 0);
        assert_eq!(c.style.fg, Color::Indexed(196));
        assert_eq!(c.style.bg, Color::Indexed(21));
    }

    #[test]
    fn sgr_rgb_color() {
        let mut t = term(10, 5);
        t.process(b"\x1b[38;2;255;128;0ma");
        let c = cell(&t, 0, 0);
        assert_eq!(c.style.fg, Color::Rgb(255, 128, 0));
    }

    #[test]
    fn sgr_default_colors() {
        let mut t = term(10, 5);
        t.process(b"\x1b[32;42m\x1b[39;49ma");
        let c = cell(&t, 0, 0);
        assert_eq!(c.style.fg, Color::Default);
        assert_eq!(c.style.bg, Color::Default);
    }

    #[test]
    fn scroll_region() {
        let mut t = term(3, 5);
        // Set scroll region to rows 2-4 (1-indexed).
        t.process(b"aaa\r\nbbb\r\nccc\r\nddd\r\neee");
        t.process(b"\x1b[2;4r"); // scroll region rows 2-4
        // cursor reset to (0,0)
        assert_eq!(t.cursor(), (0, 0));
    }

    #[test]
    fn esc_save_restore_cursor() {
        let mut t = term(20, 10);
        t.process(b"\x1b[5;10H\x1b7"); // move to (10,5) and save
        t.process(b"\x1b[H"); // move to origin
        t.process(b"\x1b8"); // restore
        assert_eq!(t.cursor(), (9, 4));
    }

    #[test]
    fn esc_ris_resets_all() {
        let mut t = term(5, 5);
        t.process(b"\x1b[3;3H\x1b[1mA");
        t.process(b"\x1bc"); // RIS
        assert_eq!(t.cursor(), (0, 0));
        assert!(!cell(&t, 2, 2).style.bold);
        assert_eq!(cell(&t, 2, 2).ch, ' ');
    }

    #[test]
    fn wrap_at_end_of_line() {
        let mut t = term(3, 3);
        t.process(b"abcd"); // 'a','b','c' fill row 0; 'd' wraps to row 1
        assert_eq!(cell(&t, 0, 1).ch, 'd');
        assert_eq!(t.cursor(), (1, 1));
    }

    #[test]
    fn cols_rows_accessors() {
        let t = term(80, 24);
        assert_eq!(t.cols(), 80);
        assert_eq!(t.rows(), 24);
    }
}
