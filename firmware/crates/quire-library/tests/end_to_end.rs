//! A card from scratch: scan, ingest real books, open, lay out pages, resume, marks.
use quire_fs::host::HostFs;
use quire_fs::Fs;
use quire_layout::{Pos, Profile};
use quire_library::book::{page_image_ids, ImageStore};
use quire_library::{ingest_book, pages, scan, Book, IngestState, Library, Loc, Status};

fn card(name: &str) -> (HostFs, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("quire-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Books/classics")).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../quire-doc/fixtures/moby-dick.epub"), dir.join("Books/classics/moby-dick.epub"))
        .unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../quire-doc/fixtures/tracemonkey.pdf"), dir.join("Books/tracemonkey.pdf"))
        .unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../quire-doc/fixtures/middlemarch.txt"), dir.join("middlemarch.txt")).unwrap();
    std::fs::write(dir.join("Books/notes.docx"), b"not a book").unwrap();
    (HostFs::new(&dir), dir)
}

#[test]
fn scan_ingest_read_resume() {
    let (fs, dir) = card("full");
    let mut lib = Library::load(&fs);
    let report = scan(&fs, &mut lib, 1_000).unwrap();
    assert_eq!(report.added, 3, "{report:?}");
    assert_eq!(lib.pending().len(), 3);

    // Ingest everything.
    for id in lib.pending() {
        let mut last = (0, 0);
        ingest_book(&fs, &mut lib, id, &mut |d, t| last = (d, t)).unwrap_or_else(|e| panic!("ingest {id}: {e}"));
        assert_eq!(lib.get(id).unwrap().ingest, IngestState::Ready);
    }
    lib.save(&fs).unwrap();

    let moby = lib.by_path("/Books/classics/moby-dick.epub").unwrap().clone();
    assert_eq!(moby.title, "Moby-Dick");
    assert_eq!(moby.authors, ["Herman Melville"]);
    assert!(moby.has_cover);
    assert!(moby.chars > 1_000_000, "{}", moby.chars);
    assert!(moby.sections >= 140, "{}", moby.sections);

    // Open, resolve a TOC entry, lay out pages with the default profile.
    let book = Book::open(&fs, moby.id).unwrap();
    assert_eq!(book.total_chars(), moby.chars);
    let ch_index = book.toc.iter().position(|t| t.title.contains("Loomings")).expect("toc has Loomings");
    let loc = book.toc_target(ch_index).unwrap();
    assert_eq!(book.toc_index_at(&loc), Some(ch_index), "TOC lookup by chars");
    let (from, to, idx) = book.chapter_bounds(loc.chars);
    assert_eq!(idx, Some(ch_index));
    assert!(to > from && to - from > 5_000, "chapter 1 spans {} chars", to - from);
    let data = book.section(&fs, loc.section).unwrap();
    let text = quire_qtx::plain_text(&data);
    assert!(text.contains("Call me Ishmael"), "section {} starts: {}", loc.section, &text[..text.len().min(120)]);
    // Every section fits the RAM budget.
    for s in &book.sections {
        assert!(s.bytes as usize <= quire_library::cache::SECTION_BYTES + 1024, "section {} is {} bytes", s.source, s.bytes);
    }

    let profile = Profile::default();
    let geom = profile.geometry(quire_gfx::PANEL_W, quire_gfx::PANEL_H);
    let key = pages::profile_key(&profile, &geom);
    let starts = pages::get_or_build(&fs, &book, key, loc.section, &data, profile, geom);
    assert!(starts.len() > 3, "{} pages", starts.len());
    assert_eq!(pages::load(&fs, &book, key, loc.section).unwrap(), starts, "index cached");
    let pg = quire_layout::Paginator::new(&data, profile, geom);
    let page = pg.page_from(starts[2]).unwrap();
    assert!(!page.items.is_empty());
    let mut store = ImageStore::new();
    store.ensure(&fs, &book, &page_image_ids(&page));
    let mut frame = quire_gfx::Frame::panel();
    quire_layout::render_page(&page, &mut frame, &profile, &store);
    assert!(frame.ink_count() > 1000);

    // Resume: position → chars → position round trip lands on the same page.
    let chars = book.chars_at(loc.section, &data, starts[2]);
    let back = book.loc_at_chars(&fs, chars).unwrap();
    assert_eq!(back.section, loc.section);
    assert_eq!(pages::page_of(&starts, back.pos), 2);
    lib.set_loc(moby.id, Loc { section: loc.section, pos: starts[2], chars });
    lib.opened(moby.id, 2_000);
    lib.save(&fs).unwrap();
    assert!(quire_library::cache::load_thumb(&fs, moby.id).is_some(), "thumbnail without opening the book");
    let lib2 = Library::load(&fs);
    let e = lib2.get(moby.id).unwrap();
    assert_eq!(e.status, Status::Reading);
    assert_eq!(e.loc.pos, starts[2]);
    assert_eq!(lib2.shelf()[0].id, moby.id);
    assert!(e.percent() < 5);

    // Idle-time index fill makes page counts available.
    let mut todo = pages::missing(&fs, &book, key);
    assert_eq!(todo.len() + 1, book.sections.len(), "one section already indexed");
    let mut steps = 0;
    while pages::build_next(&fs, &book, key, profile, geom, &mut todo).is_some() {
        steps += 1;
        assert!(steps < 10_000);
    }
    let counts = pages::page_counts(&fs, &book, key).unwrap();
    assert_eq!(pages::page_number(&book, &counts, 900, 0, 0), 1);
    assert_eq!(pages::total_pages(&book, &counts, 900), counts.iter().map(|c| *c as u32).sum::<u32>());
    assert_eq!(counts.len(), book.sections.len());
    let total_pages: u32 = counts.iter().map(|c| *c as u32).sum();
    assert!(total_pages > 800 && total_pages < 4000, "{total_pages} pages");

    // The PDF and the TXT ingested too; the TXT at the card root was picked up.
    let pdf = lib.by_path("/Books/tracemonkey.pdf").unwrap().clone();
    assert!(pdf.title.starts_with("Trace"), "{}", pdf.title);
    let txt = lib.by_path("/middlemarch.txt").unwrap();
    assert!(txt.chars > 1000);

    // Rescan: nothing new; delete a file: marked missing, cache kept; put it back: returned.
    let report = scan(&fs, &mut lib, 3_000).unwrap();
    assert_eq!(report.added, 0);
    std::fs::rename(dir.join("Books/tracemonkey.pdf"), dir.join("tm.pdf")).unwrap();
    let report = scan(&fs, &mut lib, 3_100).unwrap();
    assert_eq!((report.added, report.missing), (0, 0), "moved file keeps its id: {report:?}");
    assert_eq!(lib.get(pdf.id).unwrap().path, "/tm.pdf");
    std::fs::remove_file(dir.join("tm.pdf")).unwrap();
    let report = scan(&fs, &mut lib, 3_200).unwrap();
    assert_eq!(report.missing, 1);
    assert!(fs.exists(&quire_library::book_dir(pdf.id)));
    quire_library::forget_book(&fs, &mut lib, pdf.id);
    assert!(!fs.exists(&quire_library::book_dir(pdf.id)));
    assert!(lib.get(pdf.id).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn long_chapters_are_split_at_paragraphs() {
    let dir = std::env::temp_dir().join(format!("quire-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // One 300 KB "chapter" of short paragraphs, as a text file.
    let mut txt = String::new();
    for i in 0..6000 {
        txt.push_str(&format!("Paragraph number {i} of the long chapter, with enough words to matter.\n\n"));
    }
    std::fs::write(dir.join("long.txt"), &txt).unwrap();
    let fs = HostFs::new(&dir);
    let mut lib = Library::load(&fs);
    lib.sources = vec!["/".into()];
    scan(&fs, &mut lib, 1).unwrap();
    let id = lib.pending()[0];
    ingest_book(&fs, &mut lib, id, &mut |_, _| {}).unwrap();
    let book = Book::open(&fs, id).unwrap();
    assert!(book.sections.len() >= 5, "{} sections", book.sections.len());
    for (i, s) in book.sections.iter().enumerate() {
        assert!(s.bytes as usize <= quire_library::cache::SECTION_BYTES, "section {i}: {}", s.bytes);
        let data = book.section(&fs, i as u16).unwrap();
        // Each section starts on a block boundary and the text is continuous.
        let first = quire_layout::para::blocks(&data).next().unwrap();
        assert_eq!(first.off, 0);
    }
    let all: String = (0..book.section_count()).map(|i| quire_qtx::plain_text(&book.section(&fs, i).unwrap())).collect();
    assert!(all.contains("Paragraph number 5999"));
    assert_eq!(all.matches("Paragraph number").count(), 6000);
    // Global char offsets are monotonic across sections.
    let mid = book.total_chars() / 2;
    let loc = book.loc_at_chars(&fs, mid).unwrap();
    assert!(loc.section > 0 && loc.chars <= mid);
    assert_eq!(book.section_at_chars(loc.chars).0, loc.section);
    // The position is exact to the word, so a resume never lands on the wrong page.
    assert!(loc.pos != Pos::START);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn debug_split() {
    let dir = std::env::temp_dir().join(format!("quire-dbg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut txt = String::new();
    for i in 0..6000 {
        txt.push_str(&format!("Paragraph number {i} of the long chapter, with enough words to matter.\n\n"));
    }
    std::fs::write(dir.join("long.txt"), &txt).unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../quire-doc/fixtures/middlemarch.txt"), dir.join("middlemarch.txt")).unwrap();
    let fs = HostFs::new(&dir);
    let mut lib = Library::load(&fs);
    lib.sources = vec!["/".into()];
    scan(&fs, &mut lib, 1).unwrap();
    for id in lib.pending() {
        ingest_book(&fs, &mut lib, id, &mut |_, _| {}).unwrap();
        let book = Book::open(&fs, id).unwrap();
        println!("{} sections={} chars={} toc={}", lib.get(id).unwrap().title, book.sections.len(), book.total_chars(), book.toc.len());
        let all: String = (0..book.section_count()).map(|i| quire_qtx::plain_text(&book.section(&fs, i).unwrap())).collect();
        for n in 0..6000 {
            if book.sections.len() > 1 && !all.contains(&format!("Paragraph number {n} ")) {
                println!("  MISSING {n}");
                let i = all.find(&format!("Paragraph number {} ", n - 1)).unwrap();
                println!("  context: {:?}", &all[i..i + 200]);
            }
        }
        for (i, s) in book.sections.iter().enumerate().take(4) {
            let data = book.section(&fs, i as u16).unwrap();
            let t = quire_qtx::plain_text(&data);
            let _ = i;
            println!(
                "  [{i}] src={} part={} bytes={} chars={} title={:?} start={:?} end={:?}",
                s.source,
                s.part,
                s.bytes,
                s.chars,
                s.title,
                &t[..t.len().min(80)],
                &t[t.len().saturating_sub(60)..]
            );
        }
    }
}
