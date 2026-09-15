//! EPUB 2 and 3: container → package document → spine, with NCX or nav TOC, metadata,
//! cover, and per-chapter XHTML converted through the HTML converter.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use quire_fs::{resolve, ReadAt};

use crate::html::{tokenize, ChapterHeading, Converter, ConverterInfo, Ev, ImageRef, ImageScan, Tok, Tokenizer};
use crate::image::{Fit, ImageKind};
use crate::inflate::ByteStream;
use crate::zip::{Entry, Zip};
use crate::{limits, DocError, Metadata, Sink, TocEntry};

/// Upper bound for a single chapter's XHTML.
const CHAPTER_LIMIT: usize = device_cap(3 * 1024 * 1024);
/// Largest entry read whole on the device (chapter, OPF, nav/NCX, container).
#[cfg(target_os = "none")]
const DEVICE_ENTRY_CAP: usize = 192 * 1024;

/// Clamp a host-side whole-entry limit to what the device can hold; identity on the host.
const fn device_cap(host: usize) -> usize {
    #[cfg(target_os = "none")]
    {
        if host > DEVICE_ENTRY_CAP {
            DEVICE_ENTRY_CAP
        } else {
            host
        }
    }
    #[cfg(not(target_os = "none"))]
    {
        host
    }
}

struct ManifestItem {
    id: String,
    href: String,
    media: String,
    properties: String,
}

/// Ingest an EPUB.
pub fn ingest<R: ReadAt>(file: &R, sink: &mut dyn Sink) -> Result<(), DocError> {
    let zip = Zip::open(file)?;
    if zip.find("META-INF/encryption.xml").is_some() || zip.find("META-INF/rights.xml").is_some() {
        // Font obfuscation also uses encryption.xml; only treat as DRM when content is encrypted.
        if let Some(e) = zip.find("META-INF/encryption.xml") {
            let enc = zip.read(e, device_cap(256 * 1024)).unwrap_or_default();
            let s = String::from_utf8_lossy(&enc);
            if s.contains("adept") || s.contains("Adobe") || s.contains("html") || s.contains("xhtml") {
                return Err(DocError::Drm);
            }
        }
    }
    let container = zip.read_name("META-INF/container.xml", device_cap(64 * 1024))?;
    let container = String::from_utf8_lossy(&container);
    let opf_path = attr_of(&container, "rootfile", "full-path").ok_or(DocError::Malformed("container.xml"))?;
    let opf = zip.read_name(&opf_path, device_cap(2 * 1024 * 1024))?;
    let opf = String::from_utf8_lossy(&opf).into_owned();
    let events = tokenize(&opf);

    // Metadata, manifest, spine.
    let mut meta = Metadata::default();
    let mut manifest: Vec<ManifestItem> = Vec::new();
    let mut spine: Vec<(String, bool)> = Vec::new(); // (idref, linear)
    let mut toc_id: Option<String> = None;
    let mut cover_id: Option<String> = None;
    let mut cur_text_target: Option<&str> = None;
    let mut creator_buf: Option<String> = None;
    for ev in &events {
        match ev {
            Ev::Open { name, attrs, .. } => {
                let n = name.to_ascii_lowercase();
                match n.as_str() {
                    "item" => {
                        let get =
                            |k: &str| attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default();
                        manifest.push(ManifestItem {
                            id: get("id"),
                            href: get("href"),
                            media: get("media-type"),
                            properties: get("properties"),
                        });
                    }
                    "itemref" => {
                        let idref = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("idref")).map(|(_, v)| v.clone()).unwrap_or_default();
                        let linear = !attrs.iter().any(|(a, v)| a.eq_ignore_ascii_case("linear") && v == "no");
                        spine.push((idref, linear));
                    }
                    "spine" => {
                        toc_id = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("toc")).map(|(_, v)| v.clone());
                    }
                    "meta" => {
                        let name_attr = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("name")).map(|(_, v)| v.as_str());
                        let content = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("content")).map(|(_, v)| v.clone());
                        if name_attr == Some("cover") {
                            cover_id = content;
                        }
                        let prop = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("property")).map(|(_, v)| v.as_str());
                        cur_text_target = match prop {
                            Some("belongs-to-collection") => Some("series"),
                            Some("group-position") => Some("series-index"),
                            Some("calibre:series") => None,
                            _ => None,
                        };
                        if name_attr == Some("calibre:series") {
                            meta.series = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("content")).map(|(_, v)| v.clone());
                        }
                        if name_attr == Some("calibre:series_index") {
                            meta.series_index =
                                attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("content")).and_then(|(_, v)| parse_index(v));
                        }
                    }
                    "title" => cur_text_target = Some("title"),
                    "creator" => {
                        cur_text_target = Some("creator");
                        creator_buf = Some(String::new());
                    }
                    "language" => cur_text_target = Some("language"),
                    "publisher" => cur_text_target = Some("publisher"),
                    "date" => cur_text_target = Some("date"),
                    "description" => cur_text_target = Some("description"),
                    "subject" => cur_text_target = Some("subject"),
                    "identifier" => cur_text_target = Some("identifier"),
                    _ => {}
                }
            }
            Ev::Close(name) => {
                let n = name.to_ascii_lowercase();
                if n == "creator" {
                    if let Some(c) = creator_buf.take() {
                        let c = crate::html::collapse_ws(&c);
                        if !c.is_empty() {
                            meta.authors.push(c);
                        }
                    }
                }
                if matches!(
                    n.as_str(),
                    "title" | "creator" | "language" | "publisher" | "date" | "description" | "subject" | "identifier" | "meta"
                ) {
                    cur_text_target = None;
                }
            }
            Ev::Text(t) => {
                let t = crate::html::collapse_ws(t);
                if t.is_empty() {
                    continue;
                }
                match cur_text_target {
                    Some("title") if meta.title.is_empty() => meta.title = t,
                    Some("creator") => {
                        if let Some(b) = creator_buf.as_mut() {
                            b.push_str(&t);
                        }
                    }
                    Some("language") if meta.language.is_empty() => meta.language = t,
                    Some("publisher") => meta.publisher = Some(t),
                    Some("date") => meta.year = t.get(..4).and_then(|y| y.parse().ok()),
                    Some("description") => meta.description = Some(t),
                    Some("subject") => meta.subjects.push(t),
                    Some("identifier") if meta.identifier.is_none() => meta.identifier = Some(t),
                    Some("series") => meta.series = Some(t),
                    Some("series-index") => meta.series_index = parse_index(&t),
                    _ => {}
                }
            }
        }
    }
    if meta.title.is_empty() {
        meta.title = "Untitled".into();
    }
    sink.metadata(&meta)?;

    let find_item = |id: &str| manifest.iter().find(|m| m.id == id);
    let opf_dir_base = opf_path.clone();

    // Cover: properties="cover-image", or <meta name="cover" content="id">, or a manifest id/href containing "cover".
    let cover_item = manifest
        .iter()
        .find(|m| m.properties.split_whitespace().any(|p| p == "cover-image"))
        .or_else(|| cover_id.as_deref().and_then(find_item))
        .or_else(|| {
            manifest.iter().find(|m| {
                m.media.starts_with("image/")
                    && (m.id.to_ascii_lowercase().contains("cover") || m.href.to_ascii_lowercase().contains("cover"))
            })
        });
    if let Some(ci) = cover_item {
        let path = resolve(&opf_dir_base, &ci.href);
        if let Some(e) = zip.find(&path) {
            // Decoded straight from the archive (never loaded whole), once for both fits.
            let kind = ImageKind::from_hint(&ci.media);
            let pair = zip.entry_reader(e).and_then(|r| crate::image::decode_multi(&r, kind, &Fit::cover_pair()));
            if let Ok(mut pair) = pair {
                if let (Some(thumb), Some(full)) = (pair.pop(), pair.pop()) {
                    sink.cover(&full, &thumb)?;
                }
            }
        }
    }

    // Chapters: spine items that are XHTML. The EPUB 3 nav document is skipped unless it
    // is the only content. Candidates are listed up front so links between chapters can
    // be written as `chapter#anchor` while converting.
    let nav_path: Option<String> =
        manifest.iter().find(|m| m.properties.split_whitespace().any(|p| p == "nav")).map(|m| resolve(&opf_dir_base, &m.href));
    let mut candidates: Vec<(usize, String)> = Vec::new(); // (spine position, path)
    for (i, (idref, _linear)) in spine.iter().enumerate() {
        let Some(item) = find_item(idref) else { continue };
        if !item.media.contains("html") && !item.media.contains("xml") {
            continue;
        }
        let path = resolve(&opf_dir_base, &item.href);
        if zip.find(&path).is_none() {
            continue;
        }
        candidates.push((i, path));
    }
    if candidates.len() > 1 {
        candidates.retain(|(_, p)| Some(p) != nav_path.as_ref());
    }

    // Table of contents (EPUB 3 nav, else NCX), keyed by path until the final chapter
    // indexes are known; it also supplies section titles for chapters without a heading.
    let mut raw_toc: Vec<RawToc> = Vec::new();
    if let Some(np) = nav_path.as_deref() {
        if let Some(e) = zip.find(np) {
            if let Ok(b) = zip.read(e, device_cap(1024 * 1024)) {
                raw_toc = parse_nav(&String::from_utf8_lossy(&b), np);
            }
        }
    }
    if raw_toc.is_empty() {
        let ncx = toc_id
            .as_deref()
            .and_then(find_item)
            .or_else(|| manifest.iter().find(|m| m.media.contains("dtbncx") || m.href.ends_with(".ncx")));
        if let Some(n) = ncx {
            let p = resolve(&opf_dir_base, &n.href);
            if let Some(e) = zip.find(&p) {
                if let Ok(b) = zip.read(e, device_cap(1024 * 1024)) {
                    raw_toc = parse_ncx(&String::from_utf8_lossy(&b), &p);
                }
            }
        }
    }

    // Chapter indexes: a candidate that turns out empty is skipped, so indexes assigned
    // so far are exact and later ones are provisional (the skip shifts them down by one;
    // links written earlier to chapters past an empty item are then off by one — rare).
    let mut chapter_paths: Vec<String> = Vec::new();
    let mut assigned: Vec<Option<u16>> = Vec::with_capacity(candidates.len());
    let mut chapter_index: u16 = 0;
    let total = spine.len().max(1) as u32;
    for (ci, (i, path)) in candidates.iter().enumerate() {
        let Some(entry) = zip.find(path) else {
            assigned.push(None);
            continue;
        };
        let skipped = ci as u16 - chapter_index;
        let resolver = |href: &str| -> Option<u16> {
            let full = resolve(path, href);
            let k = candidates.iter().position(|(_, p)| *p == full)?;
            if k < ci {
                assigned[k]
            } else {
                Some(k as u16 - skipped)
            }
        };
        let toc_title = raw_toc.iter().find(|e| e.path == *path && e.anchor.is_none()).map(|e| e.title.as_str());
        let ctx = ChapterCtx {
            zip: &zip,
            manifest: &manifest,
            opf: &opf_dir_base,
            path,
            index: chapter_index,
            toc_title,
            book_title: &meta.title,
        };
        let emitted = if entry.usize_ > CHAPTER_LIMIT as u64 {
            convert_streamed(&ctx, entry, &resolver, sink)?
        } else {
            match zip.read(entry, CHAPTER_LIMIT) {
                Ok(xhtml) => convert_whole(&ctx, &xhtml, &resolver, sink)?,
                Err(DocError::TooLarge(_)) => convert_streamed(&ctx, entry, &resolver, sink)?,
                Err(e) => return Err(e),
            }
        };
        if emitted {
            assigned.push(Some(chapter_index));
            chapter_paths.push(path.clone());
            chapter_index += 1;
        } else {
            assigned.push(None);
        }
        sink.progress(*i as u32 + 1, total);
    }
    if chapter_index == 0 {
        return Err(DocError::Malformed("epub has no readable chapters"));
    }

    let mut toc = map_toc(raw_toc, &chapter_paths);
    if toc.is_empty() {
        for (i, p) in chapter_paths.iter().enumerate() {
            toc.push(TocEntry { title: crate::title_from_name(p), chapter: i as u16, anchor: None, depth: 0 });
        }
    }
    sink.toc(&toc)?;
    Ok(())
}

/// What one chapter's conversion needs from the package.
struct ChapterCtx<'a, R: ReadAt> {
    zip: &'a Zip<R>,
    manifest: &'a [ManifestItem],
    opf: &'a str,
    path: &'a str,
    index: u16,
    toc_title: Option<&'a str>,
    book_title: &'a str,
}

/// The section title handed to `begin_chapter`, in priority: the chapter's own opening
/// heading ("1 · Loomings" when it has a number and a title, else whichever part
/// exists), the TOC entry that points at this file without an anchor, the XHTML
/// `<title>` when it is not just the book title, else none.
fn section_title(info: &ConverterInfo, toc_title: Option<&str>, book_title: &str) -> Option<String> {
    info.chapter_title
        .as_ref()
        .and_then(ChapterHeading::display)
        .or_else(|| toc_title.map(String::from))
        .or_else(|| info.title.as_deref().filter(|t| !t.eq_ignore_ascii_case(book_title)).map(String::from))
}

/// Decode and store the images a chapter references, before its text is converted, so
/// the converter writes final ids and sizes and the QTX never needs a patch pass.
fn store_images<R: ReadAt>(ctx: &ChapterCtx<'_, R>, srcs: Vec<String>, sink: &mut dyn Sink) -> Result<Vec<ImageRef>, DocError> {
    let mut out = Vec::new();
    for src in srcs {
        let ipath = resolve(ctx.path, &src);
        let Some(ie) = ctx.zip.find(&ipath) else { continue };
        let media = ctx.manifest.iter().find(|m| resolve(ctx.opf, &m.href) == ipath).map(|m| m.media.clone()).unwrap_or_default();
        let Ok(reader) = ctx.zip.entry_reader(ie) else { continue };
        // Decoded straight from the archive. On failure the placeholder stays: the
        // renderer draws a frame.
        if let Ok(bm) = crate::image::decode(&reader, ImageKind::from_hint(&media), Fit::inside(limits::IMAGE_W, limits::IMAGE_H)) {
            let id = sink.image(&bm)?;
            out.push(ImageRef { src, id, w: bm.w as u16, h: bm.h as u16 });
        }
    }
    Ok(out)
}

/// Convert a chapter held in memory. Peak: the XHTML bytes (the decoded text borrows
/// them when they are UTF-8) plus the QTX output.
fn convert_whole<R: ReadAt>(
    ctx: &ChapterCtx<'_, R>,
    xhtml: &[u8],
    resolver: &dyn Fn(&str) -> Option<u16>,
    sink: &mut dyn Sink,
) -> Result<bool, DocError> {
    let text = crate::txt::decode_text(xhtml);
    let images = store_images(ctx, crate::html::scan_images(&text), sink)?;
    let mut conv = Converter::new(ctx.index).with_resolver(resolver).with_images(images);
    conv.feed_str(&text);
    let (bytes, chars, info) = conv.finish();
    if !info.has_content() {
        return Ok(false);
    }
    let title = section_title(&info, ctx.toc_title, ctx.book_title);
    sink.begin_chapter(ctx.index, title.as_deref())?;
    sink.chapter_bytes(&bytes)?;
    sink.end_chapter(chars)?;
    Ok(true)
}

/// Convert a spine item too large to hold: two passes over the inflated entry in
/// [`WINDOW`]-sized slices (images first, then text), handing QTX to the sink as each
/// window is converted. Peak: one window plus the carry, plus one window of QTX.
fn convert_streamed<R: ReadAt>(
    ctx: &ChapterCtx<'_, R>,
    entry: &Entry,
    resolver: &dyn Fn(&str) -> Option<u16>,
    sink: &mut dyn Sink,
) -> Result<bool, DocError> {
    let mut scan = ImageScan::default();
    let mut win = Windows::new(ctx.zip.stream(entry)?);
    while let Some(text) = win.next()? {
        let mut tk = Tokenizer::window(&text, win.raw, win.partial());
        for t in tk.by_ref() {
            scan.event(&t);
        }
        win.push_back(tk.rest(), tk.raw_state());
    }
    let images = store_images(ctx, scan.srcs, sink)?;

    let mut conv = Converter::new(ctx.index).with_resolver(resolver).with_images(images);
    let mut win = Windows::new(ctx.zip.stream(entry)?);
    let mut begun = false;
    while let Some(text) = win.next()? {
        let mut tk = Tokenizer::window(&text, win.raw, win.partial());
        for t in tk.by_ref() {
            conv.event(t);
        }
        win.push_back(tk.rest(), tk.raw_state());
        if !begun {
            let info = conv.info();
            if !info.has_content() {
                continue; // nothing to show yet; the buffered bytes are only anchors
            }
            sink.begin_chapter(ctx.index, section_title(&info, ctx.toc_title, ctx.book_title).as_deref())?;
            begun = true;
        }
        let bytes = conv.flush_bytes();
        sink.chapter_bytes(&bytes)?;
    }
    let (bytes, chars, info) = conv.finish();
    if !begun {
        if !info.has_content() {
            return Ok(false);
        }
        sink.begin_chapter(ctx.index, section_title(&info, ctx.toc_title, ctx.book_title).as_deref())?;
    }
    sink.chapter_bytes(&bytes)?;
    sink.end_chapter(chars)?;
    Ok(true)
}

/// Bytes of source text fed to the tokenizer per window when streaming a chapter.
const WINDOW: usize = 32 * 1024;
/// Total inflated bytes a streamed chapter may reach before it is refused.
const STREAM_LIMIT: usize = 8 * CHAPTER_LIMIT;

/// Windows of decoded text over a zip entry stream. The tokenizer's unconsumed tail
/// (an open tag, comment, or the last word) is carried into the next window so nothing
/// is ever split; an incomplete UTF-8 or UTF-16 sequence at a chunk edge is carried as
/// bytes.
struct Windows<'z, R: ReadAt> {
    stream: ByteStream<&'z R>,
    carry: String,
    pending: Vec<u8>,
    raw: Option<&'static str>,
    utf16: Option<bool>, // Some(big endian) once a BOM was seen
    first: bool,
    eof: bool,
    forced: bool,
    total: usize,
}

impl<'z, R: ReadAt> Windows<'z, R> {
    fn new(stream: ByteStream<&'z R>) -> Self {
        Windows {
            stream,
            carry: String::new(),
            pending: Vec::new(),
            raw: None,
            utf16: None,
            first: true,
            eof: false,
            forced: false,
            total: 0,
        }
    }

    /// Whether the tokenizer should hold back an incomplete tail. A carry that keeps
    /// growing (an unterminated comment or attribute quote) is force-flushed.
    fn partial(&self) -> bool {
        !self.eof && !self.forced
    }

    fn push_back(&mut self, rest: &str, raw: Option<&'static str>) {
        self.carry.clear();
        self.carry.push_str(rest);
        self.raw = raw;
    }

    /// The next window (carry plus about [`WINDOW`] new bytes), or `None` at the end.
    fn next(&mut self) -> Result<Option<String>, DocError> {
        if self.eof {
            return Ok(None);
        }
        let mut raw = core::mem::take(&mut self.pending);
        while raw.len() < WINDOW {
            let c = self.stream.next_chunk()?;
            if c.is_empty() {
                self.eof = true;
                break;
            }
            raw.extend_from_slice(c);
        }
        self.total += raw.len();
        if self.total > STREAM_LIMIT {
            return Err(DocError::TooLarge("chapter"));
        }
        self.forced = self.carry.len() > 2 * WINDOW;
        let mut text = core::mem::take(&mut self.carry);
        if self.first {
            self.first = false;
            if raw.len() >= 2 && (raw[..2] == [0xFF, 0xFE] || raw[..2] == [0xFE, 0xFF]) {
                self.utf16 = Some(raw[0] == 0xFE);
                raw.drain(..2);
            }
        }
        if let Some(be) = self.utf16 {
            if raw.len() % 2 == 1 && !self.eof {
                self.pending = raw.split_off(raw.len() - 1);
            }
            let units = raw.as_chunks::<2>().0.iter().map(|c| if be { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) });
            text.extend(char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')));
            return Ok(Some(text));
        }
        if let Err(e) = core::str::from_utf8(&raw) {
            if e.error_len().is_none() && !self.eof {
                // An incomplete sequence at the edge: finish it with the next chunk.
                self.pending = raw.split_off(e.valid_up_to());
            }
        }
        text.push_str(&crate::txt::decode_text(&raw));
        Ok(Some(text))
    }
}

fn parse_index(s: &str) -> Option<u16> {
    let s = s.trim();
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let i: u16 = int.parse().ok()?;
    let f: u16 = frac.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(0) as u16;
    Some(i * 10 + f)
}

fn attr_of(xml: &str, tag: &str, attr: &str) -> Option<String> {
    for ev in tokenize(xml) {
        if let Ev::Open { name, attrs, .. } = ev {
            if name.eq_ignore_ascii_case(tag) {
                if let Some((_, v)) = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case(attr)) {
                    return Some(v.clone());
                }
            }
        }
    }
    None
}

/// A TOC entry before chapter indexes are known.
struct RawToc {
    title: String,
    /// Resolved path of the target file.
    path: String,
    anchor: Option<String>,
    depth: u8,
}

fn split_target(href: &str, base: &str) -> (String, Option<String>) {
    let (path, frag) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string())),
        None => (href, None),
    };
    let full = if path.is_empty() { base.to_string() } else { resolve(base, path) };
    (full, frag)
}

/// Map path-keyed entries onto the chapters that were actually written.
fn map_toc(raw: Vec<RawToc>, chapters: &[String]) -> Vec<TocEntry> {
    raw.into_iter()
        .filter_map(|e| {
            let ch = chapters.iter().position(|c| *c == e.path)? as u16;
            Some(TocEntry { title: e.title, chapter: ch, anchor: e.anchor, depth: e.depth })
        })
        .collect()
}

fn attr_str<'b>(attrs: &'b [(&str, alloc::borrow::Cow<'_, str>)], name: &str) -> Option<&'b str> {
    attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_ref())
}

fn parse_nav(xhtml: &str, base: &str) -> Vec<RawToc> {
    let mut out = Vec::new();
    let mut depth: i32 = -1;
    let mut in_nav = false;
    let mut cur: Option<(String, String)> = None; // (href, text)
    for ev in Tokenizer::new(xhtml) {
        match ev {
            Tok::Open { name, attrs, .. } => {
                let n = name.to_ascii_lowercase();
                if n == "nav" {
                    let t = attr_str(&attrs, "epub:type").unwrap_or("");
                    in_nav = t.contains("toc") || t.is_empty();
                    depth = -1;
                } else if in_nav && n == "ol" {
                    depth += 1;
                } else if in_nav && n == "a" {
                    let href = attr_str(&attrs, "href").unwrap_or("").to_string();
                    cur = Some((href, String::new()));
                }
            }
            Tok::Close(name) => {
                let n = name.to_ascii_lowercase();
                if n == "nav" {
                    in_nav = false;
                } else if in_nav && n == "ol" {
                    depth -= 1;
                } else if in_nav && n == "a" {
                    if let Some((href, text)) = cur.take() {
                        let title = crate::html::collapse_ws(&text);
                        if !title.is_empty() {
                            let (path, anchor) = split_target(&href, base);
                            out.push(RawToc { title, path, anchor, depth: depth.max(0) as u8 });
                        }
                    }
                }
            }
            Tok::Text(t) => {
                if let Some((_, text)) = cur.as_mut() {
                    text.push_str(&t);
                }
            }
        }
    }
    out
}

fn parse_ncx(xml: &str, base: &str) -> Vec<RawToc> {
    let mut out = Vec::new();
    let mut depth: i32 = -1;
    let mut label: Option<String> = None;
    let mut in_text = false;
    let mut pending_label: Option<String> = None;
    for ev in Tokenizer::new(xml) {
        match ev {
            Tok::Open { name, attrs, .. } => {
                let n = name.to_ascii_lowercase();
                match n.as_str() {
                    "navpoint" => {
                        depth += 1;
                        pending_label = None;
                    }
                    "text" => {
                        in_text = true;
                        label = Some(String::new());
                    }
                    "content" => {
                        let src = attr_str(&attrs, "src").unwrap_or("");
                        if let Some(l) = pending_label.take() {
                            let (path, anchor) = split_target(src, base);
                            out.push(RawToc { title: l, path, anchor, depth: depth.max(0) as u8 });
                        }
                    }
                    _ => {}
                }
            }
            Tok::Close(name) => {
                let n = name.to_ascii_lowercase();
                match n.as_str() {
                    "navpoint" => depth -= 1,
                    "text" => {
                        in_text = false;
                        if let Some(l) = label.take() {
                            let l = crate::html::collapse_ws(&l);
                            if !l.is_empty() {
                                pending_label = Some(l);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Tok::Text(t) => {
                if in_text {
                    if let Some(l) = label.as_mut() {
                        l.push_str(&t);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::memsink::MemSink;
    use alloc::string::String;
    use alloc::vec::Vec;
    use quire_qtx::{ParaKind, Reader, Token};

    fn ingest(bytes: &[u8]) -> MemSink {
        let mut sink = MemSink::default();
        super::ingest(&bytes, &mut sink).expect("ingest");
        sink
    }

    fn tokens(sink: &MemSink, chapter: usize) -> Vec<Token> {
        Reader::new(&sink.chapters[chapter].1).collect()
    }

    #[test]
    fn moby_dick_chapter_openings_and_section_titles() {
        let sink = ingest(include_bytes!("../fixtures/moby-dick.epub"));
        let ch1 = sink.chapters.iter().position(|c| quire_qtx::plain_text(&c.1).contains("Call me Ishmael")).expect("chapter 1");
        let toks = tokens(&sink, ch1);
        let first_block = toks.iter().find(|t| !matches!(t, Token::Anchor(_))).unwrap();
        assert_eq!(first_block, &Token::ChapterTitle { number: Some("1".into()), title: Some("Loomings".into()) });
        assert!(!toks.contains(&Token::Para(ParaKind::Heading(1))), "the opening is not also a heading");
        assert_eq!(sink.chapters[ch1].0.as_deref(), Some("1 · Loomings"));
        // Only the title page (whose own heading is the book title) may carry it.
        let book_titled = sink.chapters.iter().filter(|c| c.0.as_deref() == Some("Moby-Dick")).count();
        assert!(book_titled <= 1, "{book_titled} sections titled like the book");
        // The cover page (an image, no TOC entry, <title> = book title) is the only one without.
        let untitled = sink.chapters.iter().filter(|c| c.0.is_none()).count();
        assert!(untitled <= 1, "{untitled} untitled sections");
        // Every real chapter opens with a ChapterTitle; the TOC still maps to the right chapters.
        let openings = sink
            .chapters
            .iter()
            .filter(|c| matches!(Reader::new(&c.1).find(|t| !matches!(t, Token::Anchor(_))), Some(Token::ChapterTitle { .. })))
            .count();
        assert!(openings > 130, "{openings} openings");
        let loomings = sink.toc.iter().find(|t| t.title.contains("Loomings")).unwrap();
        assert_eq!(loomings.chapter as usize, ch1);
        assert_eq!(loomings.anchor, None);
        // The brief TOC page is a chapter; the nav document is not in this spine.
        assert!(sink.chapters.iter().all(|c| !c.1.is_empty()));
    }

    #[test]
    fn wasteland_verse_and_footnotes() {
        let sink = ingest(include_bytes!("../fixtures/wasteland.epub"));
        assert_eq!(sink.chapters.len(), 1, "nav is not a chapter");
        let toks = tokens(&sink, 0);
        let verse = toks.iter().filter(|t| matches!(t, Token::Para(ParaKind::Verse))).count();
        assert!(verse > 300, "verse lines: {verse}");
        let text = quire_qtx::plain_text(&sink.chapters[0].1);
        assert!(text.contains("April is the cruellest month, breeding\nLilacs out of the dead land, mixing\n"), "one paragraph per line");
        assert!(!text.contains("Hofgarten,10"), "line numbers dropped: {}", &text[..600]);
        assert!(toks.contains(&Token::Footnote("0#note-1".into())), "footnote target has the chapter#anchor form");
        assert!(toks.contains(&Token::Anchor("note-1".into())));
        assert_eq!(sink.chapters[0].0.as_deref(), Some("The Waste Land"));
    }

    #[test]
    fn childrens_literature_page_numbers_and_nav() {
        let sink = ingest(include_bytes!("../fixtures/childrens-literature.epub"));
        let all = sink.all_text();
        for line in all.lines() {
            let t = line.trim();
            assert!(!(t == "169" || t == "170" || t == "171"), "stray page number paragraph {t:?}");
        }
        assert!(all.contains("newly-fallen snow"), "hyphenated word across a page break is intact");
        assert!(!all.contains("THE CONTENTS"), "nav document is not a chapter");
        assert!(sink.chapters.iter().all(|c| c.0.as_deref() != Some("Children's Literature")));
        let s04 = sink.chapters.iter().find(|c| quire_qtx::plain_text(&c.1).contains("FAIRY STORIES")).unwrap();
        let first = Reader::new(&s04.1).find(|t| !matches!(t, Token::Anchor(_))).unwrap();
        assert!(matches!(&first, Token::ChapterTitle { number: Some(n), .. } if n == "IV"), "{first:?}");
        assert!(sink.toc.iter().any(|t| t.chapter == 1 && t.anchor.is_some()), "toc anchors map into the chapter");
    }

    fn synthetic_epub(chapters: &[(&str, &str)]) -> Vec<u8> {
        let mut b = crate::zip::testzip::Builder::new();
        b.add("mimetype", b"application/epub+zip", false);
        b.add("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#, true);
        let mut opf = String::from(
            r#"<package><metadata><dc:title>Synth</dc:title></metadata><manifest><item id="nav" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/>"#,
        );
        for (name, _) in chapters {
            opf.push_str(&alloc::format!(r#"<item id="{name}" href="{name}.xhtml" media-type="application/xhtml+xml"/>"#));
        }
        opf.push_str("</manifest><spine><itemref idref=\"nav\"/>");
        for (name, _) in chapters {
            opf.push_str(&alloc::format!(r#"<itemref idref="{name}"/>"#));
        }
        opf.push_str("</spine></package>");
        b.add("OPS/book.opf", opf.as_bytes(), true);
        let mut nav = String::from(r#"<html><body><h1>Contents</h1><nav epub:type="toc"><ol>"#);
        for (name, _) in chapters {
            nav.push_str(&alloc::format!(r#"<li><a href="{name}.xhtml">Entry {name}</a></li>"#));
        }
        nav.push_str("</ol></nav></body></html>");
        b.add("OPS/nav.xhtml", nav.as_bytes(), true);
        for (name, body) in chapters {
            b.add(&alloc::format!("OPS/{name}.xhtml"), alloc::format!("<html><body>{body}</body></html>").as_bytes(), true);
        }
        b.finish()
    }

    #[test]
    fn links_across_files_and_nav_skipped() {
        let epub = synthetic_epub(&[
            (
                "a",
                r##"<h1>Chapter One</h1><p id="top">See <a href="b.xhtml#x">there</a> and <a href="#top">here</a> and <a href="b.xhtml">file</a>.<a epub:type="noteref" href="notes.xhtml#n1">1</a></p>"##,
            ),
            ("empty", "<p>   </p>"),
            ("b", r##"<p id="x">Target <a href="a.xhtml#top">back</a></p>"##),
        ]);
        let sink = ingest(&epub);
        assert_eq!(sink.chapters.len(), 2, "nav and the empty item are skipped");
        let a = tokens(&sink, 0);
        let links: Vec<String> = a.iter().filter_map(|t| if let Token::Link(s) = t { Some(s.clone()) } else { None }).collect();
        // "b" is provisionally chapter 2 while "a" converts; the empty item shifts it to 1.
        assert_eq!(links, ["2#x", "0#top", "2"]);
        assert!(a.contains(&Token::Footnote("0#n1".into())));
        assert!(a.contains(&Token::Anchor("top".into())));
        let b = tokens(&sink, 1);
        assert!(b.contains(&Token::Link("0#top".into())), "{b:?}");
        assert_eq!(sink.chapters[0].0.as_deref(), Some("One"));
        assert_eq!(sink.chapters[1].0.as_deref(), Some("Entry b"), "TOC entry supplies the title");
        assert_eq!(sink.toc.iter().map(|t| t.chapter).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn nav_is_kept_when_it_is_the_only_content() {
        let epub = synthetic_epub(&[]);
        let sink = ingest(&epub);
        assert_eq!(sink.chapters.len(), 1);
    }

    #[test]
    fn oversized_chapter_is_streamed_in_windows() {
        let mut body = String::from("<h2>Chapter 3. Big</h2>");
        let mut n = 0;
        while body.len() < super::CHAPTER_LIMIT + 200 * 1024 {
            body.push_str(&alloc::format!("<p id=\"p{n}\">Paragraph {n} of the &ldquo;big&rdquo; chapter, with <em>emphasis</em> and a <a href=\"#p0\">link</a>.</p>\n"));
            n += 1;
        }
        let epub = synthetic_epub(&[("big", &body), ("small", "<p>Small.</p>")]);
        let sink = ingest(&epub);
        assert_eq!(sink.chapters.len(), 2);
        assert_eq!(sink.chapters[0].0.as_deref(), Some("3 · Big"));
        let text = quire_qtx::plain_text(&sink.chapters[0].1);
        assert!(text.starts_with("3 Big\n"));
        for i in [0, 1, n / 2, n - 1] {
            assert!(text.contains(&alloc::format!("Paragraph {i} of the “big” chapter, with emphasis and a link.\n")), "paragraph {i}");
        }
        assert_eq!(text.matches("Paragraph ").count(), n);
        assert!(sink.chapters[0].2 > 1_000_000);
        let toks = tokens(&sink, 0);
        assert!(toks.contains(&Token::Link("0#p0".into())));
        assert!(!toks.iter().any(|t| matches!(t, Token::Text(s) if s.contains('<') || s.contains("&"))), "no split tags or entities");
    }

    #[test]
    fn streamed_conversion_matches_whole_on_a_real_chapter() {
        // s04.xhtml is 338 KB: over the device's 192 KB limit, so it streams there.
        let data: &[u8] = include_bytes!("../fixtures/childrens-literature.epub");
        let zip = crate::zip::Zip::open(&data).unwrap();
        let entry = zip.find("EPUB/s04.xhtml").unwrap().clone();
        let resolver = |_: &str| -> Option<u16> { None };
        let ctx = super::ChapterCtx {
            zip: &zip,
            manifest: &[],
            opf: "EPUB/package.opf",
            path: "EPUB/s04.xhtml",
            index: 1,
            toc_title: None,
            book_title: "x",
        };
        let mut whole = MemSink::default();
        let xhtml = zip.read(&entry, 1 << 20).unwrap();
        assert!(super::convert_whole(&ctx, &xhtml, &resolver, &mut whole).unwrap());
        let mut streamed = MemSink::default();
        assert!(super::convert_streamed(&ctx, &entry, &resolver, &mut streamed).unwrap());
        assert_eq!(whole.chapters[0].0, streamed.chapters[0].0);
        assert_eq!(whole.chapters[0].2, streamed.chapters[0].2, "char counts");
        assert_eq!(quire_qtx::plain_text(&whole.chapters[0].1), quire_qtx::plain_text(&streamed.chapters[0].1));
        let kinds = |b: &[u8]| {
            Reader::new(b).filter(|t| matches!(t, Token::Para(_) | Token::ChapterTitle { .. } | Token::Anchor(_))).collect::<Vec<_>>()
        };
        assert_eq!(kinds(&whole.chapters[0].1), kinds(&streamed.chapters[0].1), "same blocks and anchors");
    }
}
