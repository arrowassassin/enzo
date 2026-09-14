//! Words, line breaking, hyphenation and justification for one paragraph.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::text::measure_text_q;
use quire_gfx::Font;
use quire_qtx::style;
use unicode_linebreak::{linebreaks, BreakOpportunity};

use crate::para::Run;
use crate::{Lang, Profile};

/// A word-sized piece of text: one break-opportunity segment, possibly split by style.
#[derive(Clone, Debug)]
pub struct Atom {
    /// Text without trailing whitespace.
    pub text: String,
    /// Style flags.
    pub style: u8,
    /// Font for `style`.
    pub font: &'static Font,
    /// Width in quarter pixels.
    pub width_q: i32,
    /// Width of the trailing space in quarter pixels (0 if glued to the next atom).
    pub space_q: i32,
    /// A mandatory break follows this atom.
    pub hard_break: bool,
    /// Index of the break-opportunity segment this atom belongs to.
    pub word: u32,
    /// Character offset inside the segment where this atom starts (after a hyphen split).
    pub part: u16,
    /// Inside a link.
    pub link: bool,
    /// Whether the atom may be hyphenated (alphabetic, long enough, not code).
    pub hyphenable: bool,
}

/// A laid-out line: atoms with x positions in quarter pixels.
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// Atoms and their x offsets (quarter px) from the text block's left edge.
    pub atoms: Vec<(i32, Atom)>,
    /// Position the line begins at.
    pub first: (u32, u16),
    /// Whether this is the paragraph's last line.
    pub last: bool,
    /// Natural (unjustified) width, quarter px.
    pub natural_q: i32,
}

/// Convert runs into atoms using UAX #14 break opportunities.
pub fn atomize(runs: &[Run], profile: &Profile, code: bool) -> Vec<Atom> {
    // Concatenate the paragraph text, remembering run boundaries.
    let mut text = String::new();
    let mut bounds: Vec<(usize, usize, usize)> = Vec::new(); // (start, end, run index)
    for (i, r) in runs.iter().enumerate() {
        let s = text.len();
        text.push_str(&r.text);
        if r.break_after {
            text.push('\n');
        }
        bounds.push((s, text.len(), i));
    }
    if text.is_empty() {
        return Vec::new();
    }
    let mut atoms = Vec::new();
    let mut seg_start = 0usize;
    let mut word = 0u32;
    let emit = |seg_start: usize, seg_end: usize, mandatory: bool, word: u32, atoms: &mut Vec<Atom>| {
        let seg = &text[seg_start..seg_end];
        // Trailing whitespace becomes the space after; the newline (if any) a hard break.
        let trimmed_end = seg.trim_end_matches([' ', '\n', '\t', '\u{A0}']).len();
        let had_space = trimmed_end < seg.len() && !code;
        let had_space = had_space || (code && seg[trimmed_end..].contains(' '));
        // Split the core by run boundaries so every atom has one style.
        let mut pieces: Vec<(usize, usize, usize)> = Vec::new();
        for &(bs, be, ri) in &bounds {
            let s = bs.max(seg_start);
            let e = be.min(seg_start + trimmed_end);
            if s < e {
                pieces.push((s, e, ri));
            }
        }
        if pieces.is_empty() {
            // Whitespace-only segment: attach its space to the previous atom.
            if let Some(last) = atoms.last_mut() {
                let a: &mut Atom = last;
                if had_space && a.space_q == 0 {
                    a.space_q = space_width_q(a.font);
                }
                if mandatory {
                    a.hard_break = true;
                }
            }
            return;
        }
        let n = pieces.len();
        for (k, (s, e, ri)) in pieces.into_iter().enumerate() {
            let run = &runs[ri];
            let st = if code { run.style | style::MONO } else { run.style };
            let font = profile.font(st);
            let t = &text[s..e];
            let last_piece = k + 1 == n;
            let width_q = measure_text_q(font, t);
            let hyphenable = is_hyphenable(t, code);
            atoms.push(Atom {
                text: String::from(t),
                style: st,
                font,
                width_q,
                space_q: if last_piece && had_space { space_width_q(font) } else { 0 },
                hard_break: last_piece && mandatory,
                word,
                part: 0,
                link: run.link,
                hyphenable,
            });
        }
    };
    for (idx, op) in linebreaks(&text) {
        let mandatory = matches!(op, BreakOpportunity::Mandatory);
        emit(seg_start, idx, mandatory, word, &mut atoms);
        seg_start = idx;
        word += 1;
    }
    if seg_start < text.len() {
        emit(seg_start, text.len(), false, word, &mut atoms);
    }
    if let Some(a) = atoms.last_mut() {
        a.space_q = 0;
        a.hard_break = false;
    }
    atoms
}

/// Longest word we will ever hand to the hyphenator. Beyond this a word is not prose
/// (URLs, chemical names, concatenations), and hypher's own inline buffer is 45 bytes.
const MAX_HYPHEN_BYTES: usize = 45;

/// Whether a word may be hyphenated: prose-looking, long enough to be worth splitting,
/// and short enough that the hyphenator is safe and cheap.
fn is_hyphenable(t: &str, code: bool) -> bool {
    if code || t.len() > MAX_HYPHEN_BYTES {
        return false;
    }
    let (_, core, _) = word_core(t);
    core.chars().count() >= 5
}

/// Split an atom into (leading punctuation, the alphabetic word, trailing punctuation):
/// `“shore,”` → (`“`, `shore`, `,”`). The hyphenator only ever sees the word.
fn word_core(t: &str) -> (&str, &str, &str) {
    let is_word = |c: char| c.is_alphabetic() || c == '\'' || c == '\u{2019}';
    let start = t.find(is_word).unwrap_or(t.len());
    let end = t.rfind(is_word).map(|i| i + t[i..].chars().next().map_or(1, char::len_utf8)).unwrap_or(start);
    let core = &t[start..end];
    if core.chars().all(is_word) {
        (&t[..start], core, &t[end..])
    } else {
        (t, "", "")
    }
}

fn space_width_q(font: &Font) -> i32 {
    font.glyph(' ').map(|g| g.advance_q as i32).unwrap_or((font.size() as i32) << 2 >> 2)
}

/// Options for breaking one paragraph.
#[derive(Clone, Copy, Debug)]
pub struct BreakOpts {
    /// Width of the first line, quarter px (drop cap and indents reduce it).
    pub first_width_q: i32,
    /// Width of other lines, quarter px.
    pub width_q: i32,
    /// Lines 2..=n that also use `first_width_q` (drop cap occupies three lines).
    pub narrow_lines: u32,
    /// Justify.
    pub justify: bool,
    /// Hyphenate.
    pub hyphenate: bool,
    /// Language.
    pub lang: Lang,
}

/// Break atoms into lines. Greedy fill with hyphenation as a last resort, then
/// justification by distributing the leftover across the inter-word spaces.
pub fn break_lines(atoms: Vec<Atom>, opts: BreakOpts) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut cur = Line::default();
    let mut cur_w = 0i32; // natural width so far incl. spaces before the next atom
    let mut pending_space = 0i32;
    let mut queue: Vec<Atom> = atoms;
    queue.reverse();
    let width_for = |line_no: usize| -> i32 {
        if (line_no as u32) < opts.narrow_lines.max(1) {
            opts.first_width_q
        } else {
            opts.width_q
        }
    };
    let mut line_no = 0usize;

    while let Some(atom) = queue.pop() {
        let width = width_for(line_no);
        let need = cur_w + pending_space + atom.width_q;
        if need <= width || cur.atoms.is_empty() && !atom.hyphenable {
            if cur.atoms.is_empty() {
                cur.first = (atom.word, atom.part);
            }
            let x = cur_w + pending_space;
            cur_w = x + atom.width_q;
            pending_space = atom.space_q;
            let hard = atom.hard_break;
            cur.atoms.push((x, atom));
            if hard {
                finish(&mut lines, &mut cur, cur_w, false, opts.justify, width, true);
                cur_w = 0;
                pending_space = 0;
                line_no += 1;
            }
            continue;
        }
        // Does not fit. Try hyphenating the atom to fill the line.
        if opts.hyphenate && atom.hyphenable {
            let avail = width - cur_w - pending_space;
            if let Some((head, tail)) = hyphen_split(&atom, avail, opts.lang) {
                if cur.atoms.is_empty() {
                    cur.first = (head.word, head.part);
                }
                let x = cur_w + pending_space;
                cur_w = x + head.width_q;
                cur.atoms.push((x, head));
                finish(&mut lines, &mut cur, cur_w, false, opts.justify, width, false);
                cur_w = 0;
                pending_space = 0;
                line_no += 1;
                queue.push(tail);
                continue;
            }
        }
        if cur.atoms.is_empty() {
            // Wider than the line and unhyphenatable: place it anyway (it will clip).
            cur.first = (atom.word, atom.part);
            cur_w = atom.width_q;
            pending_space = atom.space_q;
            let hard = atom.hard_break;
            cur.atoms.push((0, atom));
            if hard {
                finish(&mut lines, &mut cur, cur_w, false, opts.justify, width, true);
                cur_w = 0;
                pending_space = 0;
                line_no += 1;
            }
            continue;
        }
        // Close the line and retry the atom on the next one.
        finish(&mut lines, &mut cur, cur_w, false, opts.justify, width, false);
        cur_w = 0;
        pending_space = 0;
        line_no += 1;
        queue.push(atom);
    }
    if !cur.atoms.is_empty() {
        finish(&mut lines, &mut cur, cur_w, true, opts.justify, width_for(line_no), true);
    }
    if let Some(l) = lines.last_mut() {
        l.last = true;
    }
    lines
}

fn finish(lines: &mut Vec<Line>, cur: &mut Line, natural_q: i32, last: bool, justify: bool, width_q: i32, ragged: bool) {
    let mut line = core::mem::take(cur);
    line.natural_q = natural_q;
    line.last = last;
    if justify && !last && !ragged && line.atoms.len() > 1 {
        let gaps: Vec<usize> =
            line.atoms.iter().enumerate().filter(|(i, (_, a))| a.space_q > 0 && *i + 1 < line.atoms.len()).map(|(i, _)| i).collect();
        let extra = width_q - natural_q;
        if !gaps.is_empty() && extra > 0 {
            let per = extra / gaps.len() as i32;
            let space_ref = line.atoms[gaps[0]].1.space_q.max(1);
            // A line that would need more than six spaces' worth of stretch per gap stays
            // ragged (rivers read worse than one short line); hyphenation makes this rare.
            if per <= space_ref * 6 {
                let mut rem = extra - per * gaps.len() as i32;
                let mut shift = 0i32;
                let mut gi = 0usize;
                for i in 0..line.atoms.len() {
                    line.atoms[i].0 += shift;
                    if gi < gaps.len() && gaps[gi] == i {
                        shift += per + if rem > 0 { 1 } else { 0 };
                        if rem > 0 {
                            rem -= 1;
                        }
                        gi += 1;
                    }
                }
            }
        }
    }
    lines.push(line);
}

/// Split an atom at the longest hyphenation point whose `head-` fits in `avail_q`.
fn hyphen_split(atom: &Atom, avail_q: i32, lang: Lang) -> Option<(Atom, Atom)> {
    let lang = lang.hypher()?;
    let (prefix, word, suffix) = word_core(&atom.text);
    let hyphen_q = atom.font.glyph('-').map(|g| g.advance_q as i32).unwrap_or(0);
    let mut best: Option<usize> = None;
    let mut acc = 0usize;
    let syllables: Vec<&str> = hypher::hyphenate(word, lang).collect();
    if syllables.len() < 2 {
        return None;
    }
    let prefix_q = measure_text_q(atom.font, prefix);
    for s in &syllables[..syllables.len() - 1] {
        acc += s.len();
        let head = &word[..acc];
        let w = prefix_q + measure_text_q(atom.font, head) + hyphen_q;
        if w <= avail_q {
            best = Some(acc);
        } else {
            break;
        }
    }
    let cut = best?;
    let mut head_text = String::from(prefix);
    head_text.push_str(&word[..cut]);
    head_text.push('-');
    let mut tail_text = String::from(&word[cut..]);
    tail_text.push_str(suffix);
    let head = Atom {
        width_q: measure_text_q(atom.font, &head_text),
        space_q: 0,
        hard_break: false,
        hyphenable: false,
        text: head_text,
        ..atom.clone()
    };
    let tail = Atom {
        width_q: measure_text_q(atom.font, &tail_text),
        part: atom.part + (prefix.chars().count() + word[..cut].chars().count()) as u16,
        hyphenable: is_hyphenable(&tail_text, false),
        text: tail_text,
        ..atom.clone()
    };
    Some((head, tail))
}

/// Drop atoms that come before `(word, part)`, trimming a partially consumed word.
///
/// This is what makes a reading position survive a change of type size: the paragraph is
/// re-broken from exactly the word the reader was on, rather than from the nearest line
/// boundary of some other layout.
pub fn skip_to(atoms: Vec<Atom>, word: u32, part: u16) -> Vec<Atom> {
    let mut out = Vec::with_capacity(atoms.len());
    for mut a in atoms {
        if a.word < word {
            continue;
        }
        if a.word == word && part > 0 {
            let skip = part as usize;
            let byte = a.text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(a.text.len());
            if byte >= a.text.len() {
                continue;
            }
            a.text = String::from(&a.text[byte..]);
            a.part = part;
            a.width_q = measure_text_q(a.font, &a.text);
            a.hyphenable = is_hyphenable(&a.text, false);
        }
        out.push(a);
    }
    out
}
