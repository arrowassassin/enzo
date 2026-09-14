//! Reading blocks (paragraphs, images, rules, chapter titles) out of a QTX stream.

use alloc::string::String;
use alloc::vec::Vec;
use quire_qtx::{ParaKind, Reader, Token};

/// A styled run of text inside a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The text.
    pub text: String,
    /// Style flags.
    pub style: u8,
    /// Inside a link.
    pub link: bool,
    /// A hard line break follows this run.
    pub break_after: bool,
}

/// A block-level element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Paragraph of runs.
    Para {
        /// Kind.
        kind: ParaKind,
        /// Runs in order.
        runs: Vec<Run>,
    },
    /// Block image.
    Image {
        /// Image id.
        id: u16,
        /// Stored width.
        w: u16,
        /// Stored height.
        h: u16,
    },
    /// Horizontal rule / section break.
    Rule,
    /// Chapter opening.
    Chapter {
        /// Number text.
        number: Option<String>,
        /// Title text.
        title: Option<String>,
    },
}

/// A block with its stream position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockAt {
    /// Byte offset of the block's first token.
    pub off: u32,
    /// Byte offset where the next block starts (or the stream length).
    pub next: u32,
    /// The block.
    pub block: Block,
}

fn is_block_start(t: &Token) -> bool {
    matches!(t, Token::Para(_) | Token::Image { .. } | Token::Rule | Token::ChapterTitle { .. })
}

/// Read the block starting at `off`. A stream that starts with inline tokens (no `Para`)
/// yields an implicit body paragraph.
pub fn read_block(qtx: &[u8], off: u32) -> Option<BlockAt> {
    let mut r = Reader::at(qtx, off);
    let first = r.next()?;
    let off = off.min(qtx.len() as u32);
    let mut style = 0u8;
    let mut link = false;
    let (kind, mut runs) = match first {
        Token::Para(k) => (k, Vec::new()),
        Token::Image { id, w, h } => return Some(BlockAt { off, next: r.offset(), block: Block::Image { id, w, h } }),
        Token::Rule => return Some(BlockAt { off, next: r.offset(), block: Block::Rule }),
        Token::ChapterTitle { number, title } => return Some(BlockAt { off, next: r.offset(), block: Block::Chapter { number, title } }),
        Token::End => {
            // Empty block; skip to the next real block.
            let next = r.offset();
            return if r.at_end() { None } else { read_block(qtx, next) };
        }
        Token::Text(s) => (ParaKind::Body, alloc::vec![Run { text: s, style: 0, link: false, break_after: false }]),
        Token::Style(f) => {
            style = f;
            (ParaKind::Body, Vec::new())
        }
        Token::Anchor(_) | Token::LinkEnd => (ParaKind::Body, Vec::new()),
        Token::Link(_) => {
            link = true;
            (ParaKind::Body, Vec::new())
        }
        Token::Footnote(id) => (ParaKind::Body, alloc::vec![footnote_run(&id)]),
        Token::Break => (ParaKind::Body, Vec::new()),
    };
    let next;
    loop {
        let here = r.offset();
        let Some(t) = r.next() else {
            next = here;
            break;
        };
        if is_block_start(&t) {
            next = here;
            break;
        }
        match t {
            Token::Text(s) => runs.push(Run { text: s, style, link, break_after: false }),
            Token::Style(f) => style = f,
            Token::Link(_) => link = true,
            Token::LinkEnd => link = false,
            Token::Footnote(id) => runs.push(footnote_run(&id)),
            Token::Break => {
                if let Some(last) = runs.last_mut() {
                    last.break_after = true;
                } else {
                    runs.push(Run { text: String::new(), style, link, break_after: true });
                }
            }
            Token::End => return Some(BlockAt { off, next: r.offset(), block: Block::Para { kind, runs } }),
            Token::Anchor(_) => {}
            _ => unreachable!(),
        }
    }
    Some(BlockAt { off, next, block: Block::Para { kind, runs } })
}

fn footnote_run(id: &str) -> Run {
    let digits: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    let text = if digits.is_empty() { String::from("†") } else { digits };
    Run { text, style: quire_qtx::style::SUP, link: true, break_after: false }
}

/// Iterate every block in a stream.
pub fn blocks(qtx: &[u8]) -> impl Iterator<Item = BlockAt> + '_ {
    let mut off = 0u32;
    core::iter::from_fn(move || {
        if off as usize >= qtx.len() {
            return None;
        }
        let b = read_block(qtx, off)?;
        off = b.next.max(b.off + 1);
        Some(b)
    })
}

/// Number of text characters in a block.
pub fn block_chars(b: &Block) -> u32 {
    match b {
        Block::Para { runs, .. } => runs.iter().map(|r| r.text.chars().count() as u32).sum(),
        Block::Chapter { number, title } => {
            number.as_ref().map_or(0, |s| s.chars().count() as u32) + title.as_ref().map_or(0, |s| s.chars().count() as u32)
        }
        _ => 0,
    }
}
