//! Simple terminal grid: a 2-D array of characters the renderer reads each frame.

pub struct TermGrid {
    pub cols: usize,
    pub rows: usize,
    cells: Vec<char>,
    cursor_col: usize,
    cursor_row: usize,
}

impl TermGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![' '; cols * rows],
            cursor_col: 0,
            cursor_row: 0,
        }
    }

    pub fn put_char(&mut self, ch: char) {
        if self.cursor_col >= self.cols {
            self.newline();
        }
        let idx = self.cursor_row * self.cols + self.cursor_col;
        self.cells[idx] = ch;
        self.cursor_col += 1;
    }

    pub fn newline(&mut self) {
        self.cursor_col = 0;
        if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
        } else {
            self.scroll_up();
        }
    }

    fn scroll_up(&mut self) {
        self.cells.copy_within(self.cols.., 0);
        let start = (self.rows - 1) * self.cols;
        self.cells[start..].fill(' ');
    }

    pub fn cell(&self, col: usize, row: usize) -> char {
        self.cells[row * self.cols + col]
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_col, self.cursor_row)
    }
}
