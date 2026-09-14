//! EPUB 2 and 3: container → package document → spine, with NCX or nav TOC, metadata,
//! cover, and per-chapter XHTML converted through the HTML converter.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use quire_fs::{resolve, ReadAt};
use quire_qtx::{Reader, Token, Writer};

use crate::html::{tokenize, Ev};
use crate::image::{Fit, ImageKind};
use crate::zip::Zip;
use crate::{limits, DocError, Metadata, Sink, TocEntry};

/// Upper bound for a single chapter's XHTML.
const CHAPTER_LIMIT: usize = 3 * 1024 * 1024;

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
            let enc = zip.read(e, 256 * 1024).unwrap_or_default();
            let s = String::from_utf8_lossy(&enc);
            if s.contains("adept") || s.contains("Adobe") || s.contains("html") || s.contains("xhtml") {
                return Err(DocError::Drm);
            }
        }
    }
    let container = zip.read_name("META-INF/container.xml", 64 * 1024)?;
    let container = String::from_utf8_lossy(&container);
    let opf_path = attr_of(&container, "rootfile", "full-path").ok_or(DocError::Malformed("container.xml"))?;
    let opf = zip.read_name(&opf_path, 2 * 1024 * 1024)?;
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
                        let get = |k: &str| attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default();
                        manifest.push(ManifestItem { id: get("id"), href: get("href"), media: get("media-type"), properties: get("properties") });
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
                            meta.series_index = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("content")).and_then(|(_, v)| parse_index(v));
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
                if matches!(n.as_str(), "title" | "creator" | "language" | "publisher" | "date" | "description" | "subject" | "identifier" | "meta") {
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
        .or_else(|| manifest.iter().find(|m| m.media.starts_with("image/") && (m.id.to_ascii_lowercase().contains("cover") || m.href.to_ascii_lowercase().contains("cover"))));
    if let Some(ci) = cover_item {
        let path = resolve(&opf_dir_base, &ci.href);
        if let Some(e) = zip.find(&path) {
            if let Ok(bytes) = zip.read(e, 6 * 1024 * 1024) {
                let kind = ImageKind::from_hint(&ci.media);
                let full = crate::image::decode(&bytes, kind, Fit::fill(limits::COVER_W, limits::COVER_H));
                let thumb = crate::image::decode(&bytes, kind, Fit { fs: false, ..Fit::fill(limits::THUMB_W, limits::THUMB_H) });
                if let (Ok(full), Ok(thumb)) = (full, thumb) {
                    sink.cover(&full, &thumb)?;
                }
            }
        }
    }

    // Chapters: linear spine items that are XHTML.
    let mut chapter_paths: Vec<String> = Vec::new();
    let mut chapter_index: u16 = 0;
    let total = spine.len().max(1) as u32;
    let mut nav_path: Option<String> = manifest.iter().find(|m| m.properties.split_whitespace().any(|p| p == "nav")).map(|m| resolve(&opf_dir_base, &m.href));
    for (i, (idref, linear)) in spine.iter().enumerate() {
        let Some(item) = find_item(idref) else { continue };
        if !item.media.contains("html") && !item.media.contains("xml") {
            continue;
        }
        let path = resolve(&opf_dir_base, &item.href);
        if Some(&path) == nav_path.as_ref() && !linear {
            continue;
        }
        let Some(entry) = zip.find(&path) else { continue };
        let xhtml = zip.read(entry, CHAPTER_LIMIT)?;
        let text = crate::txt::decode_bytes(&xhtml);
        let (qtx, chars, info) = crate::html::to_qtx(&text);
        if !info.text_emitted && info.images.is_empty() {
            continue;
        }
        sink.begin_chapter(chapter_index, info.title.as_deref())?;
        // Resolve and store images, patching ids and sizes into the stream.
        let mut id_map: Vec<(u16, u16, u16, u16)> = Vec::new(); // (tmp id, real id, w, h)
        for (tmp, src) in &info.images {
            let ipath = resolve(&path, src);
            let Some(ie) = zip.find(&ipath) else { continue };
            let media = manifest.iter().find(|m| resolve(&opf_dir_base, &m.href) == ipath).map(|m| m.media.clone()).unwrap_or_default();
            let Ok(bytes) = zip.read(ie, 8 * 1024 * 1024) else { continue };
            match crate::image::decode(&bytes, ImageKind::from_hint(&media), Fit::inside(limits::IMAGE_W, limits::IMAGE_H)) {
                Ok(bm) => {
                    let real = sink.image(&bm)?;
                    id_map.push((*tmp, real, bm.w as u16, bm.h as u16));
                }
                Err(_) => {} // leave the placeholder: the renderer draws a frame
            }
        }
        let patched = patch_images(&qtx, &id_map);
        sink.chapter_bytes(&patched)?;
        sink.end_chapter(chars)?;
        chapter_paths.push(path);
        chapter_index += 1;
        sink.progress(i as u32 + 1, total);
    }
    if chapter_index == 0 {
        return Err(DocError::Malformed("epub has no readable chapters"));
    }

    // Table of contents: EPUB 3 nav, else NCX.
    let mut toc: Vec<TocEntry> = Vec::new();
    if let Some(np) = nav_path.take() {
        if let Some(e) = zip.find(&np) {
            if let Ok(b) = zip.read(e, 1024 * 1024) {
                toc = parse_nav(&String::from_utf8_lossy(&b), &np, &chapter_paths);
            }
        }
    }
    if toc.is_empty() {
        let ncx = toc_id.as_deref().and_then(find_item).or_else(|| manifest.iter().find(|m| m.media.contains("dtbncx") || m.href.ends_with(".ncx")));
        if let Some(n) = ncx {
            let p = resolve(&opf_dir_base, &n.href);
            if let Some(e) = zip.find(&p) {
                if let Ok(b) = zip.read(e, 1024 * 1024) {
                    toc = parse_ncx(&String::from_utf8_lossy(&b), &p, &chapter_paths);
                }
            }
        }
    }
    if toc.is_empty() {
        for (i, p) in chapter_paths.iter().enumerate() {
            toc.push(TocEntry { title: crate::title_from_name(p), chapter: i as u16, anchor: None, depth: 0 });
        }
    }
    sink.toc(&toc)?;
    Ok(())
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

/// Rewrite `Image` tokens with stored ids and sizes.
fn patch_images(qtx: &[u8], map: &[(u16, u16, u16, u16)]) -> Vec<u8> {
    if map.is_empty() {
        return qtx.to_vec();
    }
    let mut w = Writer::new();
    for t in Reader::new(qtx) {
        match t {
            Token::Image { id, .. } => match map.iter().find(|m| m.0 == id) {
                Some(&(_, real, iw, ih)) => w.push(&Token::Image { id: real, w: iw, h: ih }),
                None => w.push(&Token::Image { id, w: 0, h: 0 }),
            },
            other => w.push(&other),
        }
    }
    w.finish()
}

fn chapter_for(href: &str, base: &str, chapters: &[String]) -> Option<(u16, Option<String>)> {
    let (path, frag) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string())),
        None => (href, None),
    };
    let full = if path.is_empty() { base.to_string() } else { resolve(base, path) };
    chapters.iter().position(|c| *c == full).map(|i| (i as u16, frag))
}

fn parse_nav(xhtml: &str, base: &str, chapters: &[String]) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut depth: i32 = -1;
    let mut in_nav = false;
    let mut cur: Option<(String, String)> = None; // (href, text)
    for ev in tokenize(xhtml) {
        match ev {
            Ev::Open { name, attrs, .. } => {
                let n = name.to_ascii_lowercase();
                if n == "nav" {
                    let t = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("epub:type")).map(|(_, v)| v.as_str()).unwrap_or("");
                    in_nav = t.contains("toc") || t.is_empty();
                    depth = -1;
                } else if in_nav && n == "ol" {
                    depth += 1;
                } else if in_nav && n == "a" {
                    let href = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("href")).map(|(_, v)| v.clone()).unwrap_or_default();
                    cur = Some((href, String::new()));
                }
            }
            Ev::Close(name) => {
                let n = name.to_ascii_lowercase();
                if n == "nav" {
                    in_nav = false;
                } else if in_nav && n == "ol" {
                    depth -= 1;
                } else if in_nav && n == "a" {
                    if let Some((href, text)) = cur.take() {
                        if let Some((ch, anchor)) = chapter_for(&href, base, chapters) {
                            let title = crate::html::collapse_ws(&text);
                            if !title.is_empty() {
                                out.push(TocEntry { title, chapter: ch, anchor, depth: depth.max(0) as u8 });
                            }
                        }
                    }
                }
            }
            Ev::Text(t) => {
                if let Some((_, text)) = cur.as_mut() {
                    text.push_str(&t);
                }
            }
        }
    }
    out
}

fn parse_ncx(xml: &str, base: &str, chapters: &[String]) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut depth: i32 = -1;
    let mut label: Option<String> = None;
    let mut in_text = false;
    let mut pending_label: Option<String> = None;
    for ev in tokenize(xml) {
        match ev {
            Ev::Open { name, attrs, .. } => {
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
                        let src = attrs.iter().find(|(a, _)| a.eq_ignore_ascii_case("src")).map(|(_, v)| v.clone()).unwrap_or_default();
                        if let Some(l) = pending_label.take() {
                            if let Some((ch, anchor)) = chapter_for(&src, base, chapters) {
                                out.push(TocEntry { title: l, chapter: ch, anchor, depth: depth.max(0) as u8 });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ev::Close(name) => {
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
            Ev::Text(t) => {
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
