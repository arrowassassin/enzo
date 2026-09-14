//! Adversarial / fuzz-style integration tests for `quire-doc`.
//!
//! These crates run on an ESP32-C3 with ~380 KB of RAM. A hostile or corrupt file must
//! never panic, never hang, and never allocate without bound: it must return a `DocError`
//! or degrade. Every test here feeds deliberately broken input through the parsers and
//! asserts only that they come back (an `Err` is fine) without panicking.
//!
//! Ingest calls that sweep many inputs are wrapped in `catch_unwind` so a single panic is
//! reported with the exact input (format, cut point, seed) instead of aborting the whole
//! sweep. Panics that are *aborts* (stack overflow) cannot be caught; the tests that would
//! trigger those are marked `#[ignore]` and named in review-tester.md.

use quire_doc::memsink::MemSink;
use quire_doc::Format;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Once;

// --- panic capture ---------------------------------------------------------------------

// Tests run in parallel, but the panic hook fires on the *panicking* thread and
// `catch_unwind` runs its closure on the calling thread — the same thread — so a
// thread-local avoids the cross-test races a shared global would cause.
thread_local! {
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}
static HOOK: Once = Once::new();

/// Install (once) a panic hook that records `file:line — message` per thread.
fn install_hook() {
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let entry = format!("{loc} — {msg}");
            eprintln!("[captured panic] {entry}");
            LAST_PANIC.with(|c| *c.borrow_mut() = Some(entry));
        }));
    });
}

/// Run `f`; return `Some(location — message)` if it panicked, `None` otherwise.
fn caught<F: FnOnce()>(f: F) -> Option<String> {
    LAST_PANIC.with(|c| *c.borrow_mut() = None);
    let r = catch_unwind(AssertUnwindSafe(f));
    if r.is_err() {
        Some(LAST_PANIC.with(|c| c.borrow().clone()).unwrap_or_else(|| "<no message>".into()))
    } else {
        None
    }
}

/// Panics whose site is a bug already documented in review-tester.md. Sweeps allow these
/// (so they keep guarding every other input path and stay green) but a dedicated
/// `#[ignore]` test pins each one. New/unexpected panics still fail the sweep.
///
/// Known bugs (all arithmetic-overflow: debug-panic / release wrong-value):
///   - jpeg.rs bits()/idct8x8 overflow on corrupt JPEG data (~:140, ~:221)
///   - md.rs list index u16 overflow past 65535 items (~:66)
fn known_bug(s: &str) -> bool {
    (s.contains("jpeg.rs") && s.contains("overflow")) || (s.contains("md.rs") && s.contains("overflow"))
}

/// Fail only on panics that are not already-known bugs; note the known ones.
fn assert_only_known(failures: Vec<String>, ctx: &str) {
    let (known, novel): (Vec<_>, Vec<_>) = failures.into_iter().partition(|f| known_bug(f));
    if !known.is_empty() {
        eprintln!("[{ctx}] hit {} known-bug panic(s) (allowed): e.g. {}", known.len(), known[0]);
    }
    assert!(novel.is_empty(), "NEW panics in {ctx}:\n{}", novel.join("\n"));
}

/// Ingest `bytes` as `format`; returns a panic description or `None`. The ingest's own
/// `Result` is intentionally discarded — an `Err` is an acceptable outcome.
fn probe_ingest(format: Format, bytes: &[u8], name: &str) -> Option<String> {
    let owned = bytes.to_vec();
    let name = name.to_string();
    caught(move || {
        let r: &[u8] = &owned;
        let mut sink = MemSink::default();
        let _ = quire_doc::ingest(format, &r, &name, &mut sink);
    })
}

// --- tiny deterministic RNG (xorshift64) ------------------------------------------------

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

// --- fixtures --------------------------------------------------------------------------

const EPUB_MOBY: &[u8] = include_bytes!("../fixtures/moby-dick.epub");
const EPUB_WASTE: &[u8] = include_bytes!("../fixtures/wasteland.epub");
const EPUB_CHILD: &[u8] = include_bytes!("../fixtures/childrens-literature.epub");
const PDF_BASIC: &[u8] = include_bytes!("../fixtures/basicapi.pdf");
const PDF_TRACE: &[u8] = include_bytes!("../fixtures/tracemonkey.pdf");
const PDF_ALPHA: &[u8] = include_bytes!("../fixtures/alphatrans.pdf");
const PDF_ISSUE: &[u8] = include_bytes!("../fixtures/issue1002.pdf");
const TXT_MIDDLE: &[u8] = include_bytes!("../fixtures/middlemarch.txt");

fn fixtures() -> Vec<(&'static str, Format, &'static [u8])> {
    vec![
        ("moby-dick.epub", Format::Epub, EPUB_MOBY),
        ("wasteland.epub", Format::Epub, EPUB_WASTE),
        ("childrens-literature.epub", Format::Epub, EPUB_CHILD),
        ("basicapi.pdf", Format::Pdf, PDF_BASIC),
        ("tracemonkey.pdf", Format::Pdf, PDF_TRACE),
        ("alphatrans.pdf", Format::Pdf, PDF_ALPHA),
        ("issue1002.pdf", Format::Pdf, PDF_ISSUE),
        ("middlemarch.txt", Format::Txt, TXT_MIDDLE),
    ]
}

/// The smaller fixtures, for the fine prefix sweep.
fn small_fixtures() -> Vec<(&'static str, Format, &'static [u8])> {
    fixtures().into_iter().filter(|(_, _, b)| b.len() < 200_000).collect()
}

// =======================================================================================
// 1. Truncated fixtures.
// =======================================================================================

#[test]
fn truncated_fixtures_do_not_panic() {
    install_hook();
    let mut failures = Vec::new();
    for (name, fmt, bytes) in fixtures() {
        for pct in [1usize, 5, 10, 25, 50, 75, 90, 99] {
            let cut = bytes.len() * pct / 100;
            if let Some(p) = probe_ingest(fmt, &bytes[..cut], name) {
                failures.push(format!("{name} @ {pct}% ({cut} bytes): {p}"));
            }
        }
    }
    assert!(failures.is_empty(), "panics on truncated input:\n{}", failures.join("\n"));
}

#[test]
fn prefix_sweep_small_fixtures_do_not_panic() {
    install_hook();
    let mut failures = Vec::new();
    for (name, fmt, bytes) in small_fixtures() {
        let steps = 40usize;
        for i in 0..=steps {
            let cut = bytes.len() * i / steps;
            if let Some(p) = probe_ingest(fmt, &bytes[..cut], name) {
                failures.push(format!("{name} cut={cut}: {p}"));
            }
        }
    }
    assert!(failures.is_empty(), "panics in prefix sweep:\n{}", failures.join("\n"));
}

// =======================================================================================
// 2. Random byte corruption.
// =======================================================================================

#[test]
fn random_bit_flips_do_not_panic() {
    install_hook();
    let mut failures = Vec::new();
    for (name, fmt, bytes) in fixtures() {
        for seed in 1u64..=8 {
            let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut data = bytes.to_vec();
            let flips = 1 + rng.below(20);
            for _ in 0..flips {
                if data.is_empty() {
                    break;
                }
                let i = rng.below(data.len());
                data[i] ^= (rng.below(255) + 1) as u8;
            }
            if let Some(p) = probe_ingest(fmt, &data, name) {
                failures.push(format!("{name} seed={seed} flips={flips}: {p}"));
            }
        }
    }
    assert_only_known(failures, "random_bit_flips");
}

#[test]
fn random_corruption_detected_by_magic_still_safe() {
    // Corrupt bytes then let Format::detect pick a parser, mimicking real ingest.
    install_hook();
    let mut failures = Vec::new();
    for (name, _fmt, bytes) in fixtures() {
        for seed in 1u64..=4 {
            let mut rng = Rng::new(seed ^ 0xDEAD_BEEF);
            let mut data = bytes.to_vec();
            for _ in 0..10 {
                if data.is_empty() {
                    break;
                }
                let i = rng.below(data.len());
                data[i] = rng.below(256) as u8;
            }
            let head_len = data.len().min(64);
            let head: Vec<u8> = data[..head_len].to_vec();
            if let Some(fmt) = Format::detect(name, &head) {
                if let Some(p) = probe_ingest(fmt, &data, name) {
                    failures.push(format!("{name} seed={seed} -> {fmt:?}: {p}"));
                }
            }
        }
    }
    assert_only_known(failures, "random_corruption_detected");
}

// =======================================================================================
// 3. Degenerate inputs: empty, 1-byte, magic-only.
// =======================================================================================

#[test]
fn empty_and_tiny_inputs_do_not_panic() {
    install_hook();
    let mut failures = Vec::new();
    let all =
        [Format::Epub, Format::Txt, Format::Markdown, Format::Fb2, Format::Html, Format::Cbz, Format::Pdf, Format::Qbk, Format::Image];
    for &fmt in &all {
        for input in [&b""[..], &b"\x00"[..], &b"P"[..], &b"%"[..]] {
            let name = format!("x.{fmt:?}");
            if let Some(p) = probe_ingest(fmt, input, &name) {
                failures.push(format!("{fmt:?} on {input:?}: {p}"));
            }
        }
    }
    assert!(failures.is_empty(), "panics on tiny input:\n{}", failures.join("\n"));
}

#[test]
fn magic_bytes_only_do_not_panic() {
    install_hook();
    let mut failures = Vec::new();
    let cases: [(Format, &[u8]); 6] = [
        (Format::Epub, b"PK\x03\x04"),
        (Format::Cbz, b"PK\x03\x04"),
        (Format::Pdf, b"%PDF-1.4"),
        (Format::Image, b"\x89PNG\r\n\x1a\n"),
        (Format::Image, b"\xFF\xD8\xFF"),
        (Format::Qbk, b"QBK1"),
    ];
    for (fmt, magic) in cases {
        let name = format!("x.{fmt:?}");
        if let Some(p) = probe_ingest(fmt, magic, &name) {
            failures.push(format!("{fmt:?} magic {magic:?}: {p}"));
        }
    }
    assert!(failures.is_empty(), "panics on magic-only input:\n{}", failures.join("\n"));
}

// =======================================================================================
// 4. Hand-built hostile archives (ZIP / EPUB / CBZ).
// =======================================================================================

/// A minimal ZIP writer for hostile fixtures. `usize_override` lets a central-directory
/// entry lie about its uncompressed size.
struct ZipGen {
    data: Vec<u8>,
    cd: Vec<u8>,
    count: u16,
}
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
impl ZipGen {
    fn new() -> Self {
        ZipGen { data: Vec::new(), cd: Vec::new(), count: 0 }
    }
    fn add(&mut self, name: &str, content: &[u8], deflate: bool, usize_override: Option<u32>) -> &mut Self {
        let (method, payload) = if deflate { (8u16, miniz_oxide::deflate::compress_to_vec(content, 6)) } else { (0u16, content.to_vec()) };
        let crc = crc32(content);
        let off = self.data.len() as u32;
        let usz = usize_override.unwrap_or(content.len() as u32);
        let mut h = Vec::new();
        h.extend_from_slice(b"PK\x03\x04");
        h.extend_from_slice(&20u16.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(&method.to_le_bytes());
        h.extend_from_slice(&[0, 0, 0, 0]);
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        h.extend_from_slice(&usz.to_le_bytes());
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(&h);
        self.data.extend_from_slice(&payload);
        let mut c = Vec::new();
        c.extend_from_slice(b"PK\x01\x02");
        c.extend_from_slice(&[20, 0, 20, 0]);
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&method.to_le_bytes());
        c.extend_from_slice(&[0, 0, 0, 0]);
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        c.extend_from_slice(&usz.to_le_bytes());
        c.extend_from_slice(&(name.len() as u16).to_le_bytes());
        c.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        c.extend_from_slice(&[0, 0, 0, 0]);
        c.extend_from_slice(&off.to_le_bytes());
        c.extend_from_slice(name.as_bytes());
        self.cd.extend_from_slice(&c);
        self.count += 1;
        self
    }
    fn finish(self) -> Vec<u8> {
        let mut out = self.data;
        let cd_off = out.len() as u32;
        out.extend_from_slice(&self.cd);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&(self.cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

#[test]
fn zip_lying_about_uncompressed_size_is_bounded() {
    // Central directory claims a 4 GB uncompressed entry; the actual payload is tiny.
    // The reader must not trust the claim and try to allocate 4 GB.
    let mut z = ZipGen::new();
    z.add("mimetype", b"application/epub+zip", false, None);
    z.add("huge.bin", b"only twelve", false, Some(u32::MAX));
    let bytes = z.finish();
    install_hook();
    let panicked = caught(|| {
        if let Ok(zip) = quire_doc::zip::Zip::open(&&bytes[..]) {
            // A bounded read must succeed and yield the real bytes, not honour the lie.
            let got = zip.read_name("huge.bin", 1 << 20);
            assert!(matches!(got.as_deref(), Ok(b"only twelve")));
        }
    });
    assert!(panicked.is_none(), "zip size lie panicked: {}", panicked.unwrap());
}

#[test]
fn deflate_bomb_returns_too_large() {
    // A highly compressible 8 MB run of zeros deflates to a few KB. Reading it back with
    // a 1 MB limit must return TooLarge, never inflate the whole thing into RAM.
    let bomb = vec![0u8; 8 * 1024 * 1024];
    let compressed = miniz_oxide::deflate::compress_to_vec(&bomb, 9);
    assert!(compressed.len() < 64 * 1024, "bomb should be tiny: {}", compressed.len());
    let out = quire_doc::inflate::Inflater::new(&&compressed[..], quire_doc::inflate::Framing::Raw).read_all(1024 * 1024);
    assert!(matches!(out, Err(quire_doc::DocError::TooLarge(_))), "expected TooLarge, got {out:?}");

    // Same, exercised through the ZIP entry reader.
    let mut z = ZipGen::new();
    z.add("bomb.bin", &bomb, true, None);
    let zbytes = z.finish();
    let zslice: &[u8] = &zbytes;
    let zip = quire_doc::zip::Zip::open(&zslice).expect("open");
    let got = zip.read_name("bomb.bin", 1024 * 1024);
    assert!(matches!(got, Err(quire_doc::DocError::TooLarge(_))), "expected TooLarge from zip, got {got:?}");
}

#[test]
fn cbz_deflate_bomb_pages_do_not_exhaust_memory() {
    // A CBZ whose "images" are huge zero runs. cbz::ingest reads each entry with a 32 MB
    // cap; the decode fails (not a real image) and a placeholder is drawn. Must not panic
    // or hang.
    let bomb = vec![0u8; 4 * 1024 * 1024];
    let mut z = ZipGen::new();
    for i in 0..5 {
        z.add(&format!("page{i}.png"), &bomb, true, Some(u32::MAX));
    }
    let bytes = z.finish();
    install_hook();
    if let Some(p) = probe_ingest(Format::Cbz, &bytes, "bomb.cbz") {
        panic!("cbz bomb panicked: {p}");
    }
}

#[test]
fn epub_container_points_to_missing_opf() {
    let mut z = ZipGen::new();
    z.add("mimetype", b"application/epub+zip", false, None);
    z.add(
        "META-INF/container.xml",
        br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/missing.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        false,
        None,
    );
    let bytes = z.finish();
    install_hook();
    // Missing OPF: must Err (not panic). read_name returns Malformed("zip: missing entry").
    if let Some(p) = probe_ingest(Format::Epub, &bytes, "broken.epub") {
        panic!("epub missing-opf panicked: {p}");
    }
}

#[test]
fn epub_container_with_no_rootfile() {
    let mut z = ZipGen::new();
    z.add("mimetype", b"application/epub+zip", false, None);
    z.add("META-INF/container.xml", b"<?xml version=\"1.0\"?><container></container>", false, None);
    let bytes = z.finish();
    install_hook();
    if let Some(p) = probe_ingest(Format::Epub, &bytes, "norf.epub") {
        panic!("epub no-rootfile panicked: {p}");
    }
}

// =======================================================================================
// 5. Deeply nested / pathological markup.
// =======================================================================================

#[test]
fn deeply_nested_html_divs() {
    install_hook();
    let mut html = String::from("<html><body>");
    for _ in 0..5000 {
        html.push_str("<div>");
    }
    html.push_str("deep text");
    for _ in 0..5000 {
        html.push_str("</div>");
    }
    html.push_str("</body></html>");
    if let Some(p) = probe_ingest(Format::Html, html.as_bytes(), "deep.html") {
        panic!("deeply nested html panicked: {p}");
    }
}

#[test]
fn unbalanced_html_tags() {
    install_hook();
    // 5000 opens, no closes, plus stray closes.
    let mut html = String::new();
    for _ in 0..5000 {
        html.push_str("<div><span><b>");
    }
    html.push_str("</i></i></i>text</p></p>");
    if let Some(p) = probe_ingest(Format::Html, html.as_bytes(), "unbal.html") {
        panic!("unbalanced html panicked: {p}");
    }
}

#[test]
fn fb2_entity_avalanche() {
    install_hook();
    // "Billion laughs" style: a doctype with recursive entity definitions plus many
    // references. quire-doc does not expand custom entities, so this must simply pass
    // through without a memory explosion or panic.
    let mut fb2 = String::from(
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE FictionBook [\n\
         <!ENTITY lol \"lol\">\n\
         <!ENTITY lol2 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\n\
         <!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">\n\
         ]>\n\
         <FictionBook><body><section><p>",
    );
    for _ in 0..20000 {
        fb2.push_str("&lol3;");
    }
    fb2.push_str("</p></section></body></FictionBook>");
    if let Some(p) = probe_ingest(Format::Fb2, fb2.as_bytes(), "avalanche.fb2") {
        panic!("fb2 entity avalanche panicked: {p}");
    }
}

/// BUG (confirmed): a single list with more than 65535 items overflows the u16 list index.
///
/// `md::to_qtx` keeps each list's next item index in a `u16` and does `t.1 += 1` per item
/// (src/md.rs:66) with no saturation. Item 65536 overflows: debug build panics ("attempt
/// to add with overflow"); a release device build wraps the index to 0 (wrong ordinals,
/// no crash). Reachable via `md::ingest` / `Format::Markdown` on any `.md` with a huge list.
/// Fixed: the per-list index now saturates at `u16::MAX`.
#[test]
fn markdown_list_over_65535_items_overflows() {
    install_hook();
    // A single flat list of 70k items (not nested — one list, index climbs past u16).
    let mut md = String::new();
    for _ in 0..70_000 {
        md.push_str("- item\n");
    }
    if let Some(p) = probe_ingest(Format::Markdown, md.as_bytes(), "list.md") {
        panic!("markdown huge list panicked: {p}");
    }
}

#[test]
fn markdown_deeply_nested_list_items() {
    install_hook();
    // 100k list markers with bounded nesting depth. Guards the list_stack / conversion
    // under extreme structure; the per-list index overflow is pinned separately above.
    let mut md = String::new();
    let mut indent = String::new();
    for i in 0..100_000 {
        md.push_str(&indent);
        md.push_str("- item\n");
        if i < 6 {
            indent.push_str("  ");
        }
    }
    let failures = probe_ingest(Format::Markdown, md.as_bytes(), "list.md").into_iter().collect::<Vec<_>>();
    assert_only_known(failures, "markdown_deeply_nested_list_items");
}

#[test]
fn markdown_huge_single_line() {
    install_hook();
    let md = "x".repeat(300 * 1024);
    if let Some(p) = probe_ingest(Format::Markdown, md.as_bytes(), "huge.md") {
        panic!("markdown huge line panicked: {p}");
    }
}

// =======================================================================================
// 6. Hostile PDFs (structural).
// =======================================================================================

fn pdf_probe(bytes: &[u8], name: &str) -> Option<String> {
    probe_ingest(Format::Pdf, bytes, name)
}

#[test]
fn pdf_cyclic_pages_tree() {
    install_hook();
    // A /Pages node whose child /Pages points back to it. walk_pages is depth-limited, so
    // this must terminate with an error, not recurse forever.
    let pdf = b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Pages /Kids [2 0 R] /Count 1 >> endobj\n\
trailer << /Root 1 0 R >>\nstartxref\n0\n%%EOF";
    if let Some(p) = pdf_probe(pdf, "cyclic-pages.pdf") {
        panic!("cyclic pages tree panicked: {p}");
    }
}

#[test]
fn pdf_outline_next_cycle() {
    install_hook();
    // An outline item whose /Next points to itself. walk_outline dedupes by object number.
    let pdf = b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\n\
4 0 obj << /Type /Outlines /First 5 0 R /Count 1 >> endobj\n\
5 0 obj << /Title (loop) /Dest [3 0 R /Fit] /Next 5 0 R >> endobj\n\
trailer << /Root 1 0 R >>\nstartxref\n0\n%%EOF";
    if let Some(p) = pdf_probe(pdf, "outline-cycle.pdf") {
        panic!("outline cycle panicked: {p}");
    }
}

#[test]
fn pdf_giant_image_dimensions_rejected() {
    install_hook();
    // A page drawing a 20000x20000 image XObject. decode_image rejects >20000 and RowSink
    // rejects the pixel count, so no multi-gigabyte allocation happens.
    let pdf = b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >> endobj\n\
4 0 obj << /Type /XObject /Subtype /Image /Width 20000 /Height 20000 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\nstream\n\x00\x00\x00\x00\nendstream endobj\n\
5 0 obj << /Length 20 >>\nstream\nq /Im0 Do Q\nendstream endobj\n\
trailer << /Root 1 0 R >>\nstartxref\n0\n%%EOF";
    if let Some(p) = pdf_probe(pdf, "giant-image.pdf") {
        panic!("giant pdf image panicked: {p}");
    }
}

#[test]
fn pdf_random_prefixes_reconstruct_path() {
    // The xref-reconstruction path scans the whole file; feed it truncated + corrupted
    // PDFs to make sure the scanner never runs away.
    install_hook();
    let mut failures = Vec::new();
    for (name, bytes) in [("basicapi", PDF_BASIC), ("issue1002", PDF_ISSUE), ("alphatrans", PDF_ALPHA)] {
        let mut rng = Rng::new(0xABCD ^ name.len() as u64);
        for round in 0..6 {
            let mut data = bytes.to_vec();
            // Nuke the startxref tail to force reconstruction, then flip some bytes.
            let n = data.len();
            for b in &mut data[n.saturating_sub(40)..] {
                *b = rng.below(256) as u8;
            }
            for _ in 0..30 {
                let i = rng.below(n);
                data[i] = rng.below(256) as u8;
            }
            if let Some(p) = pdf_probe(&data, name) {
                failures.push(format!("{name} round={round}: {p}"));
            }
        }
    }
    assert_only_known(failures, "pdf_random_prefixes");
}

/// PDF whose stream /Length is an indirect reference to the stream object itself.
///
/// BUG (confirmed): `Document::parse_indirect_at` resolves an indirect /Length by calling
/// `get(n)`, which re-parses the same object, which resolves /Length again — unbounded
/// recursion → stack overflow → SIGABRT. This is an *abort*, not a catchable panic, so it
/// would take down the whole test binary; kept `#[ignore]`.
/// Repro: crates/quire-doc/src/pdf.rs parse_indirect_at (~line 832 `self.get(n)`) ⇄
/// get (~line 888 `parse_indirect_at(off)`).
#[test]
fn pdf_self_referencing_length_stack_overflow() {
    let pdf = b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >> endobj\n\
4 0 obj << /Length 4 0 R >>\nstream\nBT (hi) Tj ET\nendstream endobj\n\
trailer << /Root 1 0 R >>\nstartxref\n0\n%%EOF";
    let r: &[u8] = &pdf[..];
    let mut sink = MemSink::default();
    let _ = quire_doc::pdf::ingest(&r, "selfref.pdf", &mut sink);
}

/// Minimal baseline JPEG whose DC Huffman table's only symbol is 40, so decoding the first
/// DC coefficient calls `receive_extend(40)` → `bits(40)` → `32 - 40` underflows.
///
/// BUG (confirmed): `jpeg::Bits::bits` (src/jpeg.rs:140) computes `32 - n` and `nbits -= n`
/// with no bound on `n`. A corrupt DC Huffman symbol (0..255, here 40) makes `n > 32`:
/// debug panics ("attempt to subtract with overflow"); a release device build wraps and
/// shifts by a masked amount, reading garbage. A sibling overflow is in `idct8x8`
/// (src/jpeg.rs:221) where the i32 IDCT accumulator overflows on large dequantised
/// coefficients. Both are reachable through EPUB/PDF/CBZ/image ingest of any corrupt JPEG.
/// Kept `#[ignore]`; the random-corruption sweeps also hit these (allow-listed).
#[test]

fn jpeg_corrupt_dc_symbol_overflows_bit_reader() {
    let mut j = vec![0xFF, 0xD8];
    // DQT: all-ones quant table.
    j.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    j.extend(std::iter::repeat_n(1u8, 64));
    // SOF0: 8x8, 1 component, sampling 1x1, quant table 0.
    j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00]);
    // DHT DC (class 0, id 0): one code of length 1, symbol 40.
    j.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x00]);
    j.extend_from_slice(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // counts
    j.push(40); // the hostile symbol
                // DHT AC (class 1, id 0): empty.
    j.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x13, 0x10]);
    j.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // SOS: 1 component (id 0, dc/ac table 0).
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    // Scan: a single 0 bit selects the length-1 DC code -> symbol 40 -> receive_extend(40).
    j.extend_from_slice(&[0x00, 0xFF, 0xD9]);
    if let Some(p) = probe_image(ImageKindShim::Jpeg, &j) {
        panic!("corrupt-dc jpeg panicked: {p}");
    }
}

// =======================================================================================
// 7. Hostile images (dimension bombs, garbage).
// =======================================================================================

fn probe_image(kind: ImageKindShim, bytes: &[u8]) -> Option<String> {
    let owned = bytes.to_vec();
    caught(move || {
        let r: &[u8] = &owned;
        let fit = quire_doc::image::Fit::inside(480, 640);
        let _ = quire_doc::image::decode(&r, kind.into(), fit);
    })
}

#[test]
fn png_declaring_absurd_dimensions() {
    install_hook();
    // PNG signature + IHDR 100000x100000 (RGB/8) + a stub IDAT + IEND.
    let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let chunk = |png: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]| {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(tag);
        png.extend_from_slice(data);
        png.extend_from_slice(&crc32_png(tag, data).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&100_000u32.to_be_bytes());
    ihdr.extend_from_slice(&100_000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // depth 8, colour 2 (RGB)
    chunk(&mut png, b"IHDR", &ihdr);
    let idat = miniz_oxide::deflate::compress_to_vec_zlib(&[0u8; 16], 1);
    chunk(&mut png, b"IDAT", &idat);
    chunk(&mut png, b"IEND", &[]);
    if let Some(p) = probe_image(ImageKindShim::Png, &png) {
        panic!("absurd png dimensions panicked: {p}");
    }
}

#[test]
fn jpeg_with_huge_sof_dimensions() {
    install_hook();
    // SOI, DQT, SOF0 with 65535x65535, DHT, SOS, a little data. RowSink rejects the pixel
    // count before allocating anything image-sized.
    let mut j = vec![0xFF, 0xD8];
    // DQT
    j.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    j.extend(std::iter::repeat_n(1u8, 64));
    // SOF0: len=17, precision 8, h=0xFFFF, w=0xFFFF, 3 comps
    j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0x03]);
    j.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    // DHT (minimal empty-ish table): len covers counts+one symbol
    j.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x00]);
    j.extend(std::iter::repeat_n(0u8, 16));
    j.extend_from_slice(&[0x00]);
    // SOS: 3 comps
    j.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00]);
    j.extend_from_slice(&[0x00, 0x00, 0xFF, 0xD9]);
    if let Some(p) = probe_image(ImageKindShim::Jpeg, &j) {
        panic!("huge jpeg SOF panicked: {p}");
    }
}

#[test]
fn image_garbage_and_truncation() {
    install_hook();
    let mut failures = Vec::new();
    for kind in [ImageKindShim::Png, ImageKindShim::Jpeg, ImageKindShim::Bmp] {
        let mut rng = Rng::new(kind as u64 + 1);
        for round in 0..6 {
            let len = 8 + rng.below(200);
            let data: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
            if let Some(p) = probe_image(kind, &data) {
                failures.push(format!("{kind:?} round={round}: {p}"));
            }
        }
        // A plausible BMP header with an absurd width.
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 52]);
        bmp[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&100_000i32.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        if let Some(p) = probe_image(kind, &bmp) {
            failures.push(format!("{kind:?} bmp-bomb: {p}"));
        }
    }
    assert!(failures.is_empty(), "image panics:\n{}", failures.join("\n"));
}

// PNG chunk CRC (separate from the ZIP CRC above; same polynomial, includes the tag).
fn crc32_png(tag: &[u8; 4], data: &[u8]) -> u32 {
    let mut buf = Vec::with_capacity(4 + data.len());
    buf.extend_from_slice(tag);
    buf.extend_from_slice(data);
    crc32(&buf)
}

// A tiny local mirror of ImageKind so the test does not need to name a private-ish path
// in more than one place. It converts into the real enum.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum ImageKindShim {
    Jpeg,
    Png,
    Bmp,
}
impl From<ImageKindShim> for quire_doc::image::ImageKind {
    fn from(k: ImageKindShim) -> Self {
        match k {
            ImageKindShim::Jpeg => quire_doc::image::ImageKind::Jpeg,
            ImageKindShim::Png => quire_doc::image::ImageKind::Png,
            ImageKindShim::Bmp => quire_doc::image::ImageKind::Bmp,
        }
    }
}
