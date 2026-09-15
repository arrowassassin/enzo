//! Sleep-image packs: a `pack.json` and `NN.pbm` / `NN.pbm.z` images under
//! `<sleep folder>/packs/<id>/` on the card (the format is `sleep-packs/README.md` in the
//! repository). Every image keeps a clean rectangle, the clock slot, where the live time
//! is drawn; a minute tick repaints only that rectangle.
//!
//! The JSON is scanned by hand (the crate carries no serde_json): only the fields the
//! sleep screen needs are kept, everything else is skipped whatever its shape, and any
//! malformed input yields `None` rather than a panic. Images stream from the card into
//! the frame a row at a time; a compressed image inflates through a 32 KB window, so no
//! second full-frame buffer is ever held (about 48 KB of heap at the peak).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;
use quire_fs::{Fs, ReadAt};
use quire_gfx::{draw_text, measure_text, BlitMode, Font, Frame, Ink, Rect, TextStyle};

/// The folder under the sleep folder that holds the packs.
pub const PACKS_DIR: &str = "packs";
/// The manifest inside each pack folder.
pub const PACK_FILE: &str = "pack.json";
/// The `sleep_pack` setting value that rotates across every installed pack.
pub const ALL_PACKS: &str = "*";

/// Which face the time is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClockStyle {
    /// 56 px numerals.
    #[default]
    Hero,
    /// 44 px numerals.
    Poster,
    /// 18 px bold label with tracking 2.
    Label,
}

impl ClockStyle {
    /// The face.
    pub fn font(self) -> &'static Font {
        match self {
            ClockStyle::Hero => quire_fonts::ui::hero(),
            ClockStyle::Poster => quire_fonts::ui::poster(),
            ClockStyle::Label => quire_fonts::ui::label_bold(),
        }
    }
    /// Letter spacing.
    pub const fn tracking(self) -> i32 {
        match self {
            ClockStyle::Label => 2,
            _ => 0,
        }
    }
    /// The next smaller style, for a time that does not fit.
    const fn smaller(self) -> Option<ClockStyle> {
        match self {
            ClockStyle::Hero => Some(ClockStyle::Poster),
            ClockStyle::Poster => Some(ClockStyle::Label),
            ClockStyle::Label => None,
        }
    }
    fn parse(s: &str) -> Option<ClockStyle> {
        match s {
            "hero" => Some(ClockStyle::Hero),
            "poster" => Some(ClockStyle::Poster),
            "label" => Some(ClockStyle::Label),
            _ => None,
        }
    }
}

/// What the slot is painted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Surface {
    /// Ink text on paper.
    #[default]
    Paper,
    /// Paper text on ink.
    Ink,
}

impl Surface {
    fn parse(s: &str) -> Option<Surface> {
        match s {
            "paper" => Some(Surface::Paper),
            "ink" => Some(Surface::Ink),
            _ => None,
        }
    }
}

/// The clean rectangle of an image where the time goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSlot {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
    /// Face.
    pub style: ClockStyle,
    /// Surface colour.
    pub on: Surface,
}

impl ClockSlot {
    /// The slot as a rectangle.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w.max(0) as u32, self.h.max(0) as u32)
    }
}

/// A clock as a sleep screen paints it: the slot, and whether a 1 px rule frames it (the
/// plate a sleep screen puts over an image without a slot of its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Where the time goes.
    pub slot: ClockSlot,
    /// Draw the plate's rule around the slot.
    pub plate: bool,
}

/// One image of a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackImage {
    /// The plain PBM's file name within the pack folder.
    pub file: String,
    /// The zlib-compressed PBM's file name, when the manifest names one (`<file>.z` otherwise).
    pub z: Option<String>,
    /// Display title.
    pub title: String,
    /// Where the time goes; `None` for an image without a clean slot.
    pub clock: Option<ClockSlot>,
}

impl PackImage {
    /// The compressed file's name.
    pub fn z_file(&self) -> String {
        self.z.clone().unwrap_or_else(|| alloc::format!("{}.z", self.file))
    }
}

/// A pack's manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    /// The pack's id: its folder name on the card.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The pack's folder on the card (empty for a manifest parsed from bytes).
    pub dir: String,
    /// The images, in manifest order.
    pub images: Vec<PackImage>,
}

/// What the picker needs to know about an installed pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackInfo {
    /// Folder name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Number of images.
    pub images: usize,
}

/// The packs folder under the sleep folder.
pub fn packs_dir(folder: &str) -> String {
    quire_fs::join(folder, PACKS_DIR)
}

/// The folder of one pack.
pub fn pack_dir(folder: &str, id: &str) -> String {
    quire_fs::join(&packs_dir(folder), id)
}

/// A name that stays inside its folder: no separators, no `..`, not empty.
fn plain_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\']) && !s.bytes().any(|b| b < 0x20)
}

/// Every installed pack, by folder name. A folder without a readable manifest is skipped.
pub fn list_packs<F: Fs>(fs: &F, folder: &str) -> Vec<PackInfo> {
    let mut ids: Vec<String> = fs
        .read_dir(&packs_dir(folder))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.is_dir && plain_name(&e.name))
        .map(|e| e.name)
        .collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let pack = load_pack(fs, folder, &id)?;
            Some(PackInfo { id, name: pack.name, images: pack.images.len() })
        })
        .collect()
}

/// Read and parse one pack's manifest; `None` when it is missing, unreadable or has no
/// images. The pack's `id` is its folder name (what the settings refer to), whatever the
/// manifest says.
pub fn load_pack<F: Fs>(fs: &F, folder: &str, id: &str) -> Option<Pack> {
    if !plain_name(id) {
        return None;
    }
    let dir = pack_dir(folder, id);
    let bytes = fs.read_to_vec(&quire_fs::join(&dir, PACK_FILE)).ok()?;
    let mut pack = parse_pack(&bytes)?;
    if pack.images.is_empty() {
        return None;
    }
    pack.id = String::from(id);
    pack.dir = dir;
    Some(pack)
}

/// Parse a manifest. Unknown fields are skipped; an image whose `file` is missing or
/// would leave the pack folder is dropped; a `clock` with a bad style, surface or size is
/// dropped (the image then shows without a time). `None` for malformed JSON or a
/// manifest that is not an object.
pub fn parse_pack(json: &[u8]) -> Option<Pack> {
    let mut sc = Scanner { s: json, i: 0, depth: 0 };
    let mut pack = Pack { id: String::new(), name: String::new(), dir: String::new(), images: Vec::new() };
    sc.object(|sc, key| {
        match key {
            "id" => pack.id = sc.string_or_skip()?,
            "name" => pack.name = sc.string_or_skip()?,
            "images" => {
                sc.ws();
                if sc.peek() == Some(b'[') {
                    sc.array(|sc| {
                        if let Some(img) = parse_image(sc)? {
                            pack.images.push(img);
                        }
                        Some(())
                    })?;
                } else {
                    sc.skip_value()?;
                }
            }
            _ => sc.skip_value()?,
        }
        Some(())
    })?;
    sc.ws();
    if sc.i != json.len() {
        return None;
    }
    if pack.name.is_empty() {
        pack.name = pack.id.clone();
    }
    Some(pack)
}

/// One `images[]` entry; `Some(None)` for an entry to drop (not an object, no usable file).
fn parse_image(sc: &mut Scanner) -> Option<Option<PackImage>> {
    sc.ws();
    if sc.peek() != Some(b'{') {
        sc.skip_value()?;
        return Some(None);
    }
    let mut img = PackImage { file: String::new(), z: None, title: String::new(), clock: None };
    sc.object(|sc, key| {
        match key {
            "file" => img.file = sc.string_or_skip()?,
            "z" => img.z = Some(sc.string_or_skip()?).filter(|z| plain_name(z)),
            "title" => img.title = sc.string_or_skip()?,
            "clock" => img.clock = parse_clock(sc)?,
            _ => sc.skip_value()?,
        }
        Some(())
    })?;
    if !plain_name(&img.file) {
        return Some(None);
    }
    Some(Some(img))
}

/// A `clock` object; `Some(None)` when it is not usable.
fn parse_clock(sc: &mut Scanner) -> Option<Option<ClockSlot>> {
    sc.ws();
    if sc.peek() != Some(b'{') {
        sc.skip_value()?;
        return Some(None);
    }
    let (mut x, mut y, mut w, mut h) = (None, None, None, None);
    let (mut style, mut on) = (None, None);
    sc.object(|sc, key| {
        match key {
            "x" => x = sc.int_or_skip()?,
            "y" => y = sc.int_or_skip()?,
            "w" => w = sc.int_or_skip()?,
            "h" => h = sc.int_or_skip()?,
            "style" => style = ClockStyle::parse(&sc.string_or_skip()?),
            "on" => on = Surface::parse(&sc.string_or_skip()?),
            _ => sc.skip_value()?,
        }
        Some(())
    })?;
    let (Some(x), Some(y), Some(w), Some(h), Some(style), Some(on)) = (x, y, w, h, style, on) else {
        return Some(None);
    };
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
        return Some(None);
    }
    Some(Some(ClockSlot { x, y, w, h, style, on }))
}

/// Deepest nesting the scanner follows (the device stack is small).
const MAX_DEPTH: u32 = 16;

/// A cursor over JSON text. Every method returns `None` on malformed input.
struct Scanner<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    /// Consume `c` (after whitespace); false if something else is there.
    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= MAX_DEPTH).then_some(())
    }
    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// An object: `f` is called with each key and must consume the value.
    fn object(&mut self, mut f: impl FnMut(&mut Self, &str) -> Option<()>) -> Option<()> {
        if !self.eat(b'{') {
            return None;
        }
        self.enter()?;
        if self.eat(b'}') {
            self.leave();
            return Some(());
        }
        loop {
            self.ws();
            let key = self.string()?;
            if !self.eat(b':') {
                return None;
            }
            f(self, &key)?;
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                self.leave();
                return Some(());
            }
            return None;
        }
    }

    /// An array: `f` must consume each element.
    fn array(&mut self, mut f: impl FnMut(&mut Self) -> Option<()>) -> Option<()> {
        if !self.eat(b'[') {
            return None;
        }
        self.enter()?;
        if self.eat(b']') {
            self.leave();
            return Some(());
        }
        loop {
            f(self)?;
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                self.leave();
                return Some(());
            }
            return None;
        }
    }

    /// A string at the cursor (the opening quote included), unescaped.
    fn string(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut cp = self.hex4()? as u32;
                            if (0xD800..0xDC00).contains(&cp) {
                                // A surrogate pair: the low half must follow.
                                if self.s.get(self.i..self.i + 2) == Some(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()? as u32;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    } else {
                                        cp = 0xFFFD;
                                    }
                                } else {
                                    cp = 0xFFFD;
                                }
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return None,
                    }
                }
                0x00..=0x1F => return None,
                _ => out.push(c),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    fn hex4(&mut self) -> Option<u16> {
        let mut v = 0u16;
        for _ in 0..4 {
            let c = self.peek()?;
            self.i += 1;
            let d = (c as char).to_digit(16)? as u16;
            v = (v << 4) | d;
        }
        Some(v)
    }

    /// A value expected to be a string: the string, or `None` after skipping anything else.
    fn string_or_skip(&mut self) -> Option<String> {
        self.ws();
        if self.peek() == Some(b'"') {
            self.string()
        } else {
            self.skip_value()?;
            Some(String::new())
        }
    }

    /// A value expected to be an integer: `Some(Some(n))` for one that fits an `i32` (a
    /// fraction or exponent is skipped, the integer part kept), `Some(None)` after
    /// skipping anything else.
    fn int_or_skip(&mut self) -> Option<Option<i32>> {
        self.ws();
        match self.peek() {
            Some(b'-' | b'0'..=b'9') => {
                let neg = self.eat(b'-');
                let start = self.i;
                let mut v: Option<i32> = Some(0);
                while let Some(d @ b'0'..=b'9') = self.peek() {
                    v = v.and_then(|v| v.checked_mul(10)).and_then(|v| v.checked_add((d - b'0') as i32));
                    self.i += 1;
                }
                if self.i == start {
                    return None;
                }
                self.skip_number_tail()?;
                Some(v.map(|v| if neg { -v } else { v }))
            }
            _ => {
                self.skip_value()?;
                Some(None)
            }
        }
    }

    /// The fraction and exponent of a number, if present.
    fn skip_number_tail(&mut self) -> Option<()> {
        if self.peek() == Some(b'.') {
            self.i += 1;
            let start = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            if self.i == start {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let start = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            if self.i == start {
                return None;
            }
        }
        Some(())
    }

    /// Skip any value.
    fn skip_value(&mut self) -> Option<()> {
        self.ws();
        match self.peek()? {
            b'{' => self.object(|sc, _| sc.skip_value()),
            b'[' => self.array(|sc| sc.skip_value()),
            b'"' => self.string().map(|_| ()),
            b'-' | b'0'..=b'9' => self.int_or_skip().map(|_| ()),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => None,
        }
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        if self.s.get(self.i..self.i + word.len()) == Some(word) {
            self.i += word.len();
            Some(())
        } else {
            None
        }
    }
}

/// Where a `bw × bh` image sits, centred on the frame.
fn centred(f: &Frame, bw: u32, bh: u32) -> (i32, i32) {
    ((f.width() as i32 - bw as i32) / 2, (f.height() as i32 - bh as i32) / 2)
}

/// Draw image `index` of `pack` into the frame (cleared to paper first, the image
/// centred): the plain PBM when the card has it, else the compressed one. False when
/// neither can be read; the frame then holds paper and whatever partial rows arrived.
pub fn draw_image_into<F: Fs>(fs: &F, pack: &Pack, index: usize, frame: &mut Frame) -> bool {
    let Some(img) = pack.images.get(index) else { return false };
    frame.clear(Ink::White);
    let plain = quire_fs::join(&pack.dir, &img.file);
    if fs.exists(&plain) {
        let (w, h) = (frame.width() as i32, frame.height() as i32);
        let place = move |bw: u32, bh: u32| ((w - bw as i32) / 2, (h - bh as i32) / 2);
        return quire_library::cache::load_pbm_into(fs, &plain, frame, place, BlitMode::Or).is_some();
    }
    inflate_pbm_into(fs, &quire_fs::join(&pack.dir, &img.z_file()), frame)
}

/// The inflater's output window: deflate matches reach back 32 KB.
const WINDOW: usize = 32 * 1024;
/// Compressed bytes read from the card per call.
const IN_CHUNK: usize = 4 * 1024;
/// The longest PBM header accepted.
const HEADER_MAX: usize = 64;

/// Stream a zlib-compressed P4 PBM from the card into the frame, centred, row by row.
pub fn inflate_pbm_into<F: Fs>(fs: &F, path: &str, frame: &mut Frame) -> bool {
    let Ok(file) = fs.open(path) else { return false };
    let total = file.len();
    let mut state: Box<DecompressorOxide> = Box::default();
    state.init();
    let mut inbuf = vec![0u8; IN_CHUNK];
    let mut window = vec![0u8; WINDOW];
    let mut sink = PbmSink::new();
    let (mut in_at, mut in_len, mut in_off, mut out_pos) = (0u64, 0usize, 0usize, 0usize);
    loop {
        if in_off == in_len {
            if in_at >= total {
                return false;
            }
            let Ok(n) = file.read_at(in_at, &mut inbuf) else { return false };
            if n == 0 {
                return false;
            }
            in_at += n as u64;
            in_len = n;
            in_off = 0;
        }
        let more = in_at < total;
        let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER | if more { inflate_flags::TINFL_FLAG_HAS_MORE_INPUT } else { 0 };
        let (status, consumed, written) = decompress(&mut state, &inbuf[in_off..in_len], &mut window, out_pos, flags);
        if consumed == 0 && written == 0 && status != TINFLStatus::Done {
            // No progress on the bytes it has: a stream this reader cannot follow.
            return false;
        }
        in_off += consumed;
        if !sink.push(&window[out_pos..out_pos + written], frame) {
            return false;
        }
        out_pos = (out_pos + written) & (WINDOW - 1);
        if sink.done() {
            return true;
        }
        match status {
            TINFLStatus::Done => return sink.done(),
            TINFLStatus::NeedsMoreInput | TINFLStatus::HasMoreOutput => {}
            _ => return false,
        }
    }
}

/// Where the inflated bytes go: the header is collected until it parses, then each
/// complete row is blitted into the frame.
struct PbmSink {
    header: Vec<u8>,
    /// Image size and placement once the header parsed.
    geom: Option<(u32, u32, i32, i32)>,
    row: Vec<u8>,
    fill: usize,
    rows_done: u32,
}

impl PbmSink {
    fn new() -> Self {
        PbmSink { header: Vec::with_capacity(HEADER_MAX), geom: None, row: Vec::new(), fill: 0, rows_done: 0 }
    }
    fn done(&self) -> bool {
        matches!(self.geom, Some((_, h, _, _)) if self.rows_done >= h)
    }
    /// Take inflated bytes; false when the header is not a P4 PBM.
    fn push(&mut self, mut bytes: &[u8], frame: &mut Frame) -> bool {
        if self.geom.is_none() {
            while !bytes.is_empty() && self.geom.is_none() {
                if self.header.len() >= HEADER_MAX {
                    return false;
                }
                self.header.push(bytes[0]);
                bytes = &bytes[1..];
                match parse_header(&self.header) {
                    Header::Incomplete => {}
                    Header::Bad => return false,
                    Header::Ok(w, h) => {
                        let (x, y) = centred(frame, w, h);
                        self.geom = Some((w, h, x, y));
                        self.row = vec![0u8; (w as usize).div_ceil(8)];
                    }
                }
            }
            if self.geom.is_none() {
                return true;
            }
        }
        let (w, h, x, y) = self.geom.unwrap_or_default();
        while !bytes.is_empty() && self.rows_done < h {
            let n = (self.row.len() - self.fill).min(bytes.len());
            self.row[self.fill..self.fill + n].copy_from_slice(&bytes[..n]);
            self.fill += n;
            bytes = &bytes[n..];
            if self.fill == self.row.len() {
                frame.blit_row(x, y + self.rows_done as i32, &self.row, w, BlitMode::Or);
                self.rows_done += 1;
                self.fill = 0;
            }
        }
        true
    }
}

enum Header {
    Incomplete,
    Bad,
    Ok(u32, u32),
}

/// Parse a P4 header from the bytes so far: `P4`, whitespace, width, whitespace, height,
/// one whitespace byte. `Incomplete` until the final byte is there.
fn parse_header(b: &[u8]) -> Header {
    if b.len() < 2 {
        return if b"P4".starts_with(b) { Header::Incomplete } else { Header::Bad };
    }
    if &b[..2] != b"P4" {
        return Header::Bad;
    }
    let mut i = 2;
    let mut nums = [0u32; 2];
    for n in nums.iter_mut() {
        // Whitespace (and comments) before the number.
        loop {
            match b.get(i) {
                None => return Header::Incomplete,
                Some(c) if c.is_ascii_whitespace() => i += 1,
                Some(b'#') => {
                    while let Some(c) = b.get(i) {
                        if *c == b'\n' {
                            break;
                        }
                        i += 1;
                    }
                }
                Some(_) => break,
            }
        }
        let start = i;
        while let Some(c @ b'0'..=b'9') = b.get(i) {
            *n = match n.checked_mul(10).and_then(|v| v.checked_add((c - b'0') as u32)) {
                Some(v) => v,
                None => return Header::Bad,
            };
            i += 1;
        }
        match b.get(i) {
            None => return Header::Incomplete,
            Some(c) if c.is_ascii_whitespace() && i > start => {}
            Some(_) => return Header::Bad,
        }
    }
    // The single whitespace byte after the height ends the header.
    i += 1;
    if i != b.len() {
        return Header::Bad;
    }
    let (w, h) = (nums[0], nums[1]);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Header::Bad;
    }
    Header::Ok(w, h)
}

/// The extent of the digits (and colon) above and below the baseline in `font`:
/// `(ascent, descent)`, both non-negative.
pub fn digit_extent(font: &Font) -> (i32, i32) {
    let (mut asc, mut desc) = (0i32, 0i32);
    for c in "0123456789:".chars() {
        if let Some(g) = font.glyph(c) {
            asc = asc.max(g.top as i32);
            desc = desc.max(g.bitmap.h as i32 - g.top as i32);
        }
    }
    (asc, desc)
}

/// Paint `time` in the slot: the rectangle is cleared to its surface, then the time is
/// centred in it, set in the slot's face (or a smaller one, then without an am/pm
/// suffix, when the text would not fit).
pub fn draw_clock(frame: &mut Frame, slot: &ClockSlot, time: &str) {
    let r = slot.rect();
    let (surface, inverted) = match slot.on {
        Surface::Paper => (Ink::White, false),
        Surface::Ink => (Ink::Black, true),
    };
    frame.fill_rect(r, surface);
    let mut text = time;
    let (font, style, tw) = loop {
        let mut st = slot.style;
        let fit = loop {
            let style = TextStyle { inverted, darker: false, tracking: st.tracking() };
            let tw = measure_text(st.font(), text, style);
            if tw <= r.w as i32 - 2 {
                break Some((st.font(), style, tw));
            }
            match st.smaller() {
                Some(s) => st = s,
                None => break None,
            }
        };
        if let Some(f) = fit {
            break f;
        }
        match text.split_once(' ') {
            Some((head, _)) => text = head,
            None => {
                // Nothing fits: draw the slot's face anyway, clipped by the frame.
                let style = TextStyle { inverted, darker: false, tracking: slot.style.tracking() };
                break (slot.style.font(), style, measure_text(slot.style.font(), text, style));
            }
        }
    };
    let (asc, desc) = digit_extent(font);
    let baseline = r.y + (r.h as i32 - (asc + desc)) / 2 + asc;
    draw_text(frame, font, r.x + (r.w as i32 - tw) / 2, baseline, text, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_fs::host::HostFs;

    const MOUNTAINS: &str = r#"{
  "id": "mountains",
  "name": "Mountains",
  "description": "Layered ridgelines",
  "licence": "CC0-1.0",
  "images": [
    { "file": "01.pbm", "z": "01.pbm.z", "bytes": 52283, "bytes_z": 13723, "title": "Dawn",
      "clock": { "x": 162, "y": 100, "w": 204, "h": 77, "style": "hero", "on": "paper" },
      "credit": "Generated by quire-sleep; CC0 1.0", "ink": 0.348 },
    { "file": "05.pbm", "z": "05.pbm.z", "title": "The Peak",
      "clock": { "x": 38, "y": 90, "w": 165, "h": 61, "style": "poster", "on": "ink" },
      "ink": -1.5e-2, "extra": { "nested": [1, 2, {"deep": null}], "flag": true } }
  ]
}"#;

    #[test]
    fn parses_the_real_manifest_shape() {
        let p = parse_pack(MOUNTAINS.as_bytes()).expect("parses");
        assert_eq!(p.id, "mountains");
        assert_eq!(p.name, "Mountains");
        assert_eq!(p.images.len(), 2);
        let a = &p.images[0];
        assert_eq!(a.file, "01.pbm");
        assert_eq!(a.z.as_deref(), Some("01.pbm.z"));
        assert_eq!(a.title, "Dawn");
        assert_eq!(a.clock, Some(ClockSlot { x: 162, y: 100, w: 204, h: 77, style: ClockStyle::Hero, on: Surface::Paper }));
        let b = &p.images[1];
        assert_eq!(b.clock.map(|c| (c.style, c.on)), Some((ClockStyle::Poster, Surface::Ink)));
        assert_eq!(b.z_file(), "05.pbm.z");
    }

    #[test]
    fn unknown_fields_and_odd_values_are_skipped() {
        let json = br#"{"images":[{"file":"a.pbm","title":7,"clock":"none","junk":[[[]]]},"not an object",{"nope":1},{"file":"../x.pbm"},{"file":"b.pbm","clock":{"x":1,"y":2,"w":3.9,"h":4,"style":"label","on":"ink","pad":false}}],"name":"Odd","later":{"a":"b\u00e9\ud83d\ude00\n"}}"#;
        let p = parse_pack(json).expect("parses");
        assert_eq!(p.name, "Odd");
        assert_eq!(p.id, "");
        assert_eq!(p.images.len(), 2, "{:?}", p.images);
        assert_eq!(p.images[0].file, "a.pbm");
        assert_eq!(p.images[0].title, "");
        assert_eq!(p.images[0].clock, None);
        assert_eq!(p.images[0].z_file(), "a.pbm.z");
        assert_eq!(p.images[1].clock, Some(ClockSlot { x: 1, y: 2, w: 3, h: 4, style: ClockStyle::Label, on: Surface::Ink }));
        // Numbers too big for an i32 are skipped where unknown and unusable in a slot.
        let p = parse_pack(br#"{"bytes":99999999999999999999,"images":[{"file":"a.pbm","clock":{"x":99999999999,"y":2,"w":3,"h":4,"style":"label","on":"ink"}}]}"#).unwrap();
        assert_eq!(p.images[0].clock, None);
    }

    #[test]
    fn name_falls_back_to_id() {
        let p = parse_pack(br#"{"id":"deco","images":[]}"#).unwrap();
        assert_eq!(p.name, "deco");
        assert!(p.images.is_empty());
    }

    #[test]
    fn clock_needs_every_field_and_a_size() {
        let p = parse_pack(br#"{"images":[{"file":"a.pbm","clock":{"x":1,"y":2,"w":3,"h":4,"style":"hero"}},{"file":"b.pbm","clock":{"x":1,"y":2,"w":0,"h":4,"style":"hero","on":"paper"}},{"file":"c.pbm","clock":{"x":1,"y":2,"w":3,"h":4,"style":"huge","on":"paper"}}]}"#).unwrap();
        assert!(p.images.iter().all(|i| i.clock.is_none()));
    }

    #[test]
    fn malformed_input_is_rejected_not_panicked() {
        let bad: &[&[u8]] = &[
            b"",
            b"   ",
            b"[]",
            b"{",
            b"{\"images\":[",
            b"{\"images\":[{\"file\":\"a.pbm\"}",
            b"{\"id\":\"x\",}",
            b"{\"id\":\"x\"}}",
            b"{\"id\":\"x\"} trailing",
            b"{\"id\":\"unterminated}",
            b"{\"id\":\"bad \\q escape\"}",
            b"{\"id\":\"\\u12\"}",
            b"{\"id\":\"ctrl\x01char\"}",
            b"{\"x\":1.}",
            b"{\"x\":-}",
            b"{\"x\":1e}",
            b"{\"x\":tru}",
            b"{\"x\":nul}",
            b"{id:1}",
            b"{\"a\":1 \"b\":2}",
            b"\xff\xfe{}",
        ];
        for b in bad {
            assert!(parse_pack(b).is_none(), "{:?} should be rejected", String::from_utf8_lossy(b));
        }
        // Nesting past the limit is refused rather than recursed into.
        let mut deep = String::from("{\"a\":");
        for _ in 0..40 {
            deep.push('[');
        }
        for _ in 0..40 {
            deep.push(']');
        }
        deep.push('}');
        assert!(parse_pack(deep.as_bytes()).is_none());
        // Every prefix of the good manifest is malformed too.
        for n in 0..MOUNTAINS.len() {
            let _ = parse_pack(&MOUNTAINS.as_bytes()[..n]);
        }
    }

    #[test]
    fn strings_unescape() {
        let p = parse_pack(br#"{"id":"a\/b\"c\\d\ttab\u0041\u00e9\ud83d\ude00\udc00x"}"#).unwrap();
        assert_eq!(p.id, "a/b\"c\\d\ttabAé😀\u{FFFD}x");
    }

    #[test]
    fn pbm_header_parses_incrementally() {
        let full = b"P4\n528 792\n";
        for n in 0..full.len() {
            assert!(matches!(parse_header(&full[..n]), Header::Incomplete), "{n}");
        }
        assert!(matches!(parse_header(full), Header::Ok(528, 792)));
        assert!(matches!(parse_header(b"P4 # c\n 12 34 "), Header::Ok(12, 34)));
        assert!(matches!(parse_header(b"P1\n"), Header::Bad));
        assert!(matches!(parse_header(b"P4\n0 1\n"), Header::Bad));
        assert!(matches!(parse_header(b"P4\nx"), Header::Bad));
        assert!(matches!(parse_header(b"P4\n5000 1\n"), Header::Bad));
    }

    fn pbm(w: u32, h: u32, byte: u8) -> Vec<u8> {
        let mut v = alloc::format!("P4\n{w} {h}\n").into_bytes();
        v.extend(core::iter::repeat_n(byte, (w as usize).div_ceil(8) * h as usize));
        v
    }

    #[test]
    fn packs_load_from_a_card_and_images_stream_in() {
        let dir = std::env::temp_dir().join(alloc::format!("quire-sleeppack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sleep/packs/tiny")).unwrap();
        std::fs::create_dir_all(dir.join("sleep/packs/broken")).unwrap();
        std::fs::create_dir_all(dir.join("sleep/packs/empty")).unwrap();
        std::fs::write(dir.join("sleep/packs/broken/pack.json"), b"{\"images\":[").unwrap();
        std::fs::write(dir.join("sleep/packs/empty/pack.json"), b"{\"images\":[]}").unwrap();
        std::fs::write(
            dir.join("sleep/packs/tiny/pack.json"),
            br#"{"id":"whatever","name":"Tiny","images":[{"file":"01.pbm","clock":{"x":2,"y":2,"w":40,"h":20,"style":"label","on":"paper"}},{"file":"02.pbm"},{"file":"03.pbm"}]}"#,
        )
        .unwrap();
        // 01: plain, all ink. 02: only the compressed form, striped rows. 03: missing.
        std::fs::write(dir.join("sleep/packs/tiny/01.pbm"), pbm(48, 40, 0xFF)).unwrap();
        let mut striped = b"P4\n48 40\n".to_vec();
        for r in 0..40u32 {
            striped.extend(core::iter::repeat_n(if r % 2 == 0 { 0xFF } else { 0x00 }, 6));
        }
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&striped, 6);
        std::fs::write(dir.join("sleep/packs/tiny/02.pbm.z"), &z).unwrap();
        let fs = HostFs::new(&dir);

        let packs = list_packs(&fs, "/sleep");
        assert_eq!(packs, vec![PackInfo { id: String::from("tiny"), name: String::from("Tiny"), images: 3 }]);
        assert!(load_pack(&fs, "/sleep", "broken").is_none());
        assert!(load_pack(&fs, "/sleep", "empty").is_none());
        assert!(load_pack(&fs, "/sleep", "../tiny").is_none());
        let pack = load_pack(&fs, "/sleep", "tiny").unwrap();
        assert_eq!(pack.id, "tiny", "the folder name is the id");
        assert_eq!(pack.dir, "/sleep/packs/tiny");

        let mut f = Frame::new(48, 40);
        assert!(draw_image_into(&fs, &pack, 0, &mut f));
        assert_eq!(f.ink_count(), 48 * 40);
        assert!(draw_image_into(&fs, &pack, 1, &mut f));
        assert_eq!(f.ink_count(), 48 * 20);
        assert!(f.get(0, 0) && !f.get(0, 1) && f.get(47, 38) && !f.get(47, 39));
        assert!(!draw_image_into(&fs, &pack, 2, &mut f));
        assert!(!draw_image_into(&fs, &pack, 3, &mut f));
        // A frame larger than the image centres it; a smaller one clips it.
        let mut big = Frame::new(60, 50);
        assert!(draw_image_into(&fs, &pack, 1, &mut big));
        assert_eq!(big.ink_count(), 48 * 20);
        assert!(!big.get(0, 5) && big.get(6, 5));
        let mut small = Frame::new(20, 10);
        assert!(draw_image_into(&fs, &pack, 1, &mut small));
        assert_eq!(small.ink_count(), 20 * 5);
        // Corrupt compressed data fails cleanly.
        std::fs::write(dir.join("sleep/packs/tiny/02.pbm.z"), &z[..z.len() / 2]).unwrap();
        assert!(!draw_image_into(&fs, &pack, 1, &mut f));
        std::fs::write(dir.join("sleep/packs/tiny/02.pbm.z"), b"not zlib at all").unwrap();
        assert!(!draw_image_into(&fs, &pack, 1, &mut f));
        let not_pbm = miniz_oxide::deflate::compress_to_vec_zlib(b"P6\n1 1\n255", 6);
        std::fs::write(dir.join("sleep/packs/tiny/02.pbm.z"), &not_pbm).unwrap();
        assert!(!draw_image_into(&fs, &pack, 1, &mut f));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_page_inflates_through_the_window() {
        // Larger than the 32 KB window and than one input chunk: every row must land.
        let (w, h) = (528u32, 792u32);
        let mut v = alloc::format!("P4\n{w} {h}\n").into_bytes();
        let stride = (w as usize).div_ceil(8);
        for r in 0..h as usize {
            for b in 0..stride {
                v.push(((r * 7 + b * 13) % 251) as u8);
            }
        }
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&v, 10);
        let dir = std::env::temp_dir().join(alloc::format!("quire-sleeppack-full-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.pbm.z"), &z).unwrap();
        let fs = HostFs::new(&dir);
        let mut f = Frame::panel();
        assert!(inflate_pbm_into(&fs, "/x.pbm.z", &mut f));
        let hdr = v.len() - stride * h as usize;
        assert_eq!(f.bits(), &v[hdr..]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clock_is_centred_in_its_slot() {
        let slot = ClockSlot { x: 162, y: 100, w: 204, h: 77, style: ClockStyle::Hero, on: Surface::Paper };
        let mut f = Frame::panel();
        f.clear(Ink::Black);
        draw_clock(&mut f, &slot, "21:47");
        // Ink only inside the slot's paper; the paper is the slot rectangle.
        let r = slot.rect();
        let mut min = (i32::MAX, i32::MAX);
        let mut max = (0, 0);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if f.get(x, y) {
                    min = (min.0.min(x), min.1.min(y));
                    max = (max.0.max(x), max.1.max(y));
                }
            }
        }
        let (asc, desc) = digit_extent(ClockStyle::Hero.font());
        assert!(asc > 30 && desc <= 2, "hero digits: {asc} above, {desc} below");
        let left = min.0 - r.x;
        let right = r.right() - 1 - max.0;
        assert!((left - right).abs() <= 4, "horizontal: {left} left, {right} right");
        let top = min.1 - r.y;
        let bottom = r.bottom() - 1 - max.1;
        assert!((top - bottom).abs() <= 2, "vertical: {top} top, {bottom} bottom");
        // Outside the slot the frame is untouched.
        assert!(f.get(r.x - 1, r.y) && f.get(r.right(), r.y) && f.get(r.x, r.y - 1) && f.get(r.x, r.bottom()));

        // Paper on ink: the slot is black with white digits.
        let inv = ClockSlot { on: Surface::Ink, ..slot };
        f.clear(Ink::White);
        draw_clock(&mut f, &inv, "21:47");
        assert!(f.get(r.x, r.y) && f.get(r.right() - 1, r.bottom() - 1));
        assert!(f.ink_count() < (r.w * r.h) as usize, "digits are cleared out of the ink");

        // A 12-hour time with its suffix falls back to a face that fits the slot.
        f.clear(Ink::White);
        draw_clock(&mut f, &slot, "12:47 pm");
        for y in r.y..r.bottom() {
            assert!(!f.get(r.x - 1, y) && !f.get(r.right(), y), "nothing spills sideways");
        }
        assert!(f.ink_count() > 100);
        // A slot narrower than any face still draws without panicking.
        let tiny = ClockSlot { x: 0, y: 0, w: 8, h: 8, style: ClockStyle::Label, on: Surface::Paper };
        draw_clock(&mut f, &tiny, "12:47 pm");
    }
}
