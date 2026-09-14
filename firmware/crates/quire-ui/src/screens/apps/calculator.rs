//! 78 calculator: large keys, mono digits, a small expression evaluator.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{Frame, Ink, Rect, TextStyle};

use crate::text::{centered_baseline, draw_centered, draw_right};
use crate::theme::*;
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

const KEYS: [[&str; 4]; 5] = [["C", "(", ")", "÷"], ["7", "8", "9", "×"], ["4", "5", "6", "−"], ["1", "2", "3", "+"], ["0", ".", "⌫", "="]];

/// The calculator.
pub struct Calculator {
    expr: String,
    result: Option<String>,
    focus: (usize, usize),
}

impl Calculator {
    /// New.
    pub fn new() -> Self {
        Calculator { expr: String::new(), result: None, focus: (4, 3) }
    }
}

impl Default for Calculator {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluate `+ - × ÷` with parentheses and decimals; returns a formatted result.
pub fn evaluate(expr: &str) -> Result<String, &'static str> {
    let src: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut pos = 0;
    let v = parse_expr(&src, &mut pos, 0)?;
    if pos != src.len() {
        return Err("unexpected input");
    }
    if !v.is_finite() {
        return Err("not a number");
    }
    Ok(format_num(v))
}

fn parse_expr(s: &[char], pos: &mut usize, depth: u32) -> Result<f64, &'static str> {
    if depth > 32 {
        return Err("too deep");
    }
    let mut acc = parse_term(s, pos, depth)?;
    while *pos < s.len() {
        match s[*pos] {
            '+' => {
                *pos += 1;
                acc += parse_term(s, pos, depth)?;
            }
            '-' | '−' => {
                *pos += 1;
                acc -= parse_term(s, pos, depth)?;
            }
            _ => break,
        }
    }
    Ok(acc)
}

fn parse_term(s: &[char], pos: &mut usize, depth: u32) -> Result<f64, &'static str> {
    let mut acc = parse_factor(s, pos, depth)?;
    while *pos < s.len() {
        match s[*pos] {
            '*' | '×' => {
                *pos += 1;
                acc *= parse_factor(s, pos, depth)?;
            }
            '/' | '÷' => {
                *pos += 1;
                let d = parse_factor(s, pos, depth)?;
                if d == 0.0 {
                    return Err("division by zero");
                }
                acc /= d;
            }
            _ => break,
        }
    }
    Ok(acc)
}

fn parse_factor(s: &[char], pos: &mut usize, depth: u32) -> Result<f64, &'static str> {
    if *pos >= s.len() {
        return Err("incomplete");
    }
    match s[*pos] {
        '(' => {
            *pos += 1;
            let v = parse_expr(s, pos, depth + 1)?;
            if *pos < s.len() && s[*pos] == ')' {
                *pos += 1;
                Ok(v)
            } else {
                Err("missing )")
            }
        }
        '-' | '−' => {
            *pos += 1;
            Ok(-parse_factor(s, pos, depth + 1)?)
        }
        c if c.is_ascii_digit() || c == '.' => {
            let start = *pos;
            while *pos < s.len() && (s[*pos].is_ascii_digit() || s[*pos] == '.') {
                *pos += 1;
            }
            let t: String = s[start..*pos].iter().collect();
            parse_f64(&t).ok_or("bad number")
        }
        _ => Err("unexpected"),
    }
}

fn parse_f64(t: &str) -> Option<f64> {
    let (int, frac) = t.split_once('.').unwrap_or((t, ""));
    let mut v = 0f64;
    for c in int.chars() {
        v = v * 10.0 + c.to_digit(10)? as f64;
    }
    let mut scale = 0.1;
    for c in frac.chars() {
        v += c.to_digit(10)? as f64 * scale;
        scale /= 10.0;
    }
    Some(v)
}

/// Format with up to 8 decimals, trailing zeros trimmed.
pub fn format_num(v: f64) -> String {
    let neg = v < 0.0;
    let a = if neg { -v } else { v };
    let int = a as u64;
    let mut frac = ((a - int as f64) * 1e8 + 0.5) as u64;
    let mut int = int;
    if frac >= 100_000_000 {
        int += 1;
        frac -= 100_000_000;
    }
    let mut s = alloc::format!("{}{int}", if neg { "-" } else { "" });
    if frac > 0 {
        let mut f = alloc::format!("{frac:08}");
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
    }
    s
}

impl<E: Env> Screen<E> for Calculator {
    fn name(&self) -> &'static str {
        "78-calculator"
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Calculator", None);
        let w = f.width() as i32;
        let mono = quire_fonts::ui::mono_body();
        let poster = quire_fonts::ui::poster();
        let y = widgets::CONTENT_TOP;
        let display = Rect::new(widgets::INSET, y, (w - 2 * widgets::INSET) as u32, 96);
        f.stroke_rect(display, 2, Ink::Black);
        let shown = widgets::tail_fit(mono, if self.expr.is_empty() { "0" } else { &self.expr }, display.w as i32 - 24);
        draw_right(f, mono, display.right() - 12, y + 12 + mono.ascent(), &shown, TextStyle::INK);
        if let Some(r) = &self.result {
            let rf = if quire_gfx::measure_text(poster, r, TextStyle::INK) > display.w as i32 - 24 { mono } else { poster };
            draw_right(f, rf, display.right() - 12, display.bottom() - 12, r, TextStyle::INK);
        }
        let top = display.bottom() + 16;
        let kw = (w - 2 * widgets::INSET - 3 * 8) / 4;
        let kh = ((f.height() as i32 - RAIL_H - top - 4 * 8) / 5).min(72);
        for (r, rowk) in KEYS.iter().enumerate() {
            for (c, k) in rowk.iter().enumerate() {
                let rect = Rect::new(widgets::INSET + c as i32 * (kw + 8), top + r as i32 * (kh + 8), kw as u32, kh as u32);
                let focused = self.focus == (r, c);
                if focused {
                    f.fill_rect(rect, Ink::Black);
                }
                f.stroke_rect(rect, 2, Ink::Black);
                let font = quire_fonts::ui::list_title();
                draw_centered(
                    f,
                    font,
                    rect.x + kw / 2,
                    centered_baseline(font, rect.y, kh),
                    k,
                    TextStyle { inverted: focused, ..TextStyle::INK },
                );
            }
        }
        rail(f, ["", "Back", "Press", "="], None);
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        match ev.key {
            Key::Back if ev.kind == KeyKind::Press => Action::Pop,
            Key::Up => {
                self.focus.0 = (self.focus.0 + 4) % 5;
                Action::Redraw
            }
            Key::Down => {
                self.focus.0 = (self.focus.0 + 1) % 5;
                Action::Redraw
            }
            Key::Left => {
                self.focus.1 = (self.focus.1 + 3) % 4;
                Action::Redraw
            }
            Key::Right if ev.kind == KeyKind::Press => {
                self.focus.1 = (self.focus.1 + 1) % 4;
                Action::Redraw
            }
            Key::Right => {
                self.result = Some(evaluate(&self.expr).unwrap_or_else(String::from));
                Action::Redraw
            }
            Key::Confirm => {
                let k = KEYS[self.focus.0][self.focus.1];
                match k {
                    "C" => {
                        self.expr.clear();
                        self.result = None;
                    }
                    "⌫" => {
                        self.expr.pop();
                    }
                    "=" => self.result = Some(evaluate(&self.expr).unwrap_or_else(String::from)),
                    _ => {
                        if self.result.is_some() && k.chars().all(|c| c.is_ascii_digit()) {
                            self.expr.clear();
                            self.result = None;
                        }
                        if self.expr.len() < 40 {
                            self.expr.push_str(k);
                        }
                    }
                }
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates() {
        assert_eq!(evaluate("2+3×4").unwrap(), "14");
        assert_eq!(evaluate("(2+3)×4").unwrap(), "20");
        assert_eq!(evaluate("10÷4").unwrap(), "2.5");
        assert_eq!(evaluate("−3+1").unwrap(), "-2");
        assert!(evaluate("1÷0").is_err());
        assert!(evaluate("(1+2").is_err());
        assert_eq!(evaluate("0.1+0.2").unwrap(), "0.3");
    }
}
