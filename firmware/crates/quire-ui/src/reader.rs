//! The open book: sections, page indexes, rendering, position, session tracking.
//! Everything the reading page, compass, contents, go-to, skim and sleep screens need.
//!
//! The reader keeps one section of text (10–40 KB), its page starts, the laid-out current
//! page (~5 KB) and the current page's location and facts. It never holds a frame: the
//! `Ui` owns the only frame, and overlays draw over the page already in it.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{Frame, Ink, Rect};
use quire_layout::{Geometry, Page, Paginator, Pos, Profile};
use quire_library::book::{page_image_ids, ImageStore};
use quire_library::marks::Marks;
use quire_library::{pages, Book, BookEntry, BookId, Library, Loc, SessionTracker, Stats, Status};

use crate::settings::Settings;
use crate::spine::{self, SpineModel};
use crate::theme::{MARGIN, SPINE_W};

/// Estimated characters per page when a section's index is not built yet.
const CHARS_PER_PAGE_DEFAULT: u32 = 900;

/// The reader.
pub struct Reader {
    /// The book.
    pub book: Book,
    /// Library id.
    pub id: BookId,
    /// Typography in use.
    pub profile: Profile,
    geom: Geometry,
    key: u32,
    /// Current section index (read-only outside the reader: use `goto`).
    pub section: u16,
    data: Vec<u8>,
    starts: Vec<Pos>,
    /// Current page within the section (read-only outside the reader: use `goto`).
    pub page: usize,
    /// Pages per section for this profile (`u16::MAX` = not built yet).
    counts: Vec<u16>,
    /// Bumped whenever a page count changes (the Spine drawn on the page depends on it).
    counts_gen: u16,
    /// Whether the book's page total has been recorded in the library for this profile.
    total_recorded: bool,
    images: ImageStore,
    /// Marks.
    pub marks: Marks,
    tracker: SessionTracker,
    turns_since_gc: u8,
    /// Whether the spine is shown.
    show_spine: bool,
    show_head: bool,
    /// Frame dimensions.
    w: u32,
    h: u32,
    /// The current page, laid out on demand.
    cached_page: Option<Page>,
    /// Location of the current page (computed once per page change).
    cur: Loc,
    /// The position the reader navigated to last; a typography change keeps the page
    /// starting there (and does not move it), so repeated changes do not drift.
    anchor: Loc,
    /// Facts about the current page (computed once per page change).
    info: PageInfo,
    /// TOC entry of the current page.
    toc_idx: Option<usize>,
    /// The next section's text and page starts, loaded in idle time near the boundary.
    next: Option<(u16, Vec<u8>, Vec<Pos>)>,
    /// Sections whose page index is still to build for this profile.
    todo: Vec<u16>,
    /// Where to return after following a note or link (Back pops it).
    returns: Vec<Loc>,
}

/// A rendered page's bookkeeping.
#[derive(Clone, Debug, Default)]
pub struct PageInfo {
    /// Chapter title shown in the running head.
    pub chapter: String,
    /// Global character offset at the page start.
    pub chars: u32,
    /// Percent × 10.
    pub permille: u32,
    /// Whether this is the last page of the book.
    pub last: bool,
    /// Whether the page starts a chapter.
    pub chapter_start: bool,
}

/// Everything that determines what `Reader::render` paints: two equal keys mean the same
/// pixels, so a frame already holding the page can be drawn over instead of re-rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderKey {
    section: u16,
    page: u32,
    key: u32,
    w: u32,
    h: u32,
    counts_gen: u16,
    flags: u8,
}

impl Reader {
    /// Open a book from the library at its saved position.
    pub fn open<F: Fs>(fs: &F, lib: &Library, settings: &Settings, id: BookId, now: u32) -> Result<Reader, quire_library::LibError> {
        let entry = lib.get(id).ok_or(quire_library::LibError::NotFound)?;
        let book = Book::open(fs, id)?;
        let (w, h) = (quire_gfx::PANEL_W, quire_gfx::PANEL_H);
        let profile = settings.profile;
        let geom = Self::geometry(&profile, w, h, settings.spine, settings.running_head);
        let key = pages::profile_key(&profile, &geom);
        let marks = Marks::load(fs, &book.dir);
        let loc = entry.loc;
        let mut r = Reader {
            book,
            id,
            profile,
            geom,
            key,
            section: 0,
            data: Vec::new(),
            starts: Vec::new(),
            page: 0,
            counts: Vec::new(),
            counts_gen: 0,
            total_recorded: false,
            images: ImageStore::new(),
            marks,
            tracker: SessionTracker::start(id, now, loc.chars, false),
            turns_since_gc: 0,
            show_spine: settings.spine,
            show_head: settings.running_head,
            w,
            h,
            cached_page: None,
            cur: Loc::default(),
            anchor: Loc::default(),
            info: PageInfo::default(),
            toc_idx: None,
            next: None,
            todo: Vec::new(),
            returns: Vec::new(),
        };
        r.load_counts(fs);
        r.goto(fs, loc);
        Ok(r)
    }

    fn load_counts<F: Fs>(&mut self, fs: &F) {
        self.counts = pages::counts(fs, &self.book, self.key);
        self.todo = self.counts.iter().enumerate().filter(|(_, n)| **n == u16::MAX).map(|(i, _)| i as u16).collect();
        self.counts_gen = self.counts_gen.wrapping_add(1);
        self.total_recorded = false;
    }

    /// The text geometry for a profile on a frame, leaving room for the Spine and head.
    pub fn geometry(profile: &Profile, w: u32, h: u32, spine: bool, head: bool) -> Geometry {
        let mut g = profile.geometry(w, h);
        // The Spine sits inside the right margin: keep the text block clear of it.
        if spine && g.text.right() > w as i32 - MARGIN - SPINE_W - 6 {
            let shrink = g.text.right() - (w as i32 - MARGIN - SPINE_W - 6);
            g.text.w = g.text.w.saturating_sub(shrink as u32);
        }
        if head {
            let head_h = 30;
            if g.text.y < MARGIN + head_h {
                let d = MARGIN + head_h - g.text.y;
                g.text.y += d;
                g.text.h = g.text.h.saturating_sub(d as u32);
            }
        }
        g
    }

    /// Apply new typography (font size etc.): rebuild the current section's page index for
    /// the new profile and keep the position.
    pub fn set_profile<F: Fs>(&mut self, fs: &F, settings: &Settings) {
        let chars = self.cur.chars;
        self.profile = settings.profile;
        self.show_spine = settings.spine;
        self.show_head = settings.running_head;
        self.geom = Self::geometry(&self.profile, self.w, self.h, self.show_spine, self.show_head);
        let old_key = self.key;
        self.key = pages::profile_key(&self.profile, &self.geom);
        if old_key != self.key {
            self.load_counts(fs);
            pages::purge_except(fs, &self.book, &[self.key, old_key]);
        }
        self.next = None;
        self.cached_page = None;
        // The section text is already in RAM: the new page begins at the anchor (the
        // position last navigated to), which the rebuilt index keeps as a page boundary.
        let anchor = self.anchor;
        let pos = if anchor.section == self.section {
            anchor.pos
        } else {
            let within = chars.saturating_sub(self.book.chars_before_section(self.section));
            if self.data.is_empty() {
                Pos::START
            } else {
                quire_layout::pos_at_chars(&self.data, within)
            }
        };
        self.starts = if self.book.section_count() == 0 {
            alloc::vec![Pos::START]
        } else {
            pages::get_or_build_pinned(fs, &self.book, self.key, self.section, &self.data, self.profile, self.geom, &mut self.counts, pos)
        };
        self.todo.retain(|s| *s != self.section);
        self.counts_gen = self.counts_gen.wrapping_add(1);
        self.page = pages::page_of(&self.starts, pos);
        self.sync();
        self.anchor = anchor;
    }

    /// Set the frame size (orientation change).
    pub fn set_size<F: Fs>(&mut self, fs: &F, settings: &Settings, w: u32, h: u32) {
        self.w = w;
        self.h = h;
        self.set_profile(fs, settings);
    }

    fn load_section<F: Fs>(&mut self, fs: &F, section: u16) {
        let section = section.min(self.book.section_count().saturating_sub(1));
        if section == self.section && !self.data.is_empty() {
            return;
        }
        self.section = section;
        match self.next.take() {
            Some((s, data, starts)) if s == section => {
                self.data = data;
                self.starts = starts;
            }
            _ => {
                self.data = self.book.section(fs, section).unwrap_or_default();
                self.starts =
                    pages::get_or_build_counted(fs, &self.book, self.key, section, &self.data, self.profile, self.geom, &mut self.counts);
                self.counts_gen = self.counts_gen.wrapping_add(1);
            }
        }
        self.todo.retain(|s| *s != section);
        self.cached_page = None;
    }

    /// Recompute the cached location and page facts after the section or page changed.
    fn sync(&mut self) {
        self.cached_page = None;
        let pos = self.starts.get(self.page).copied().unwrap_or(Pos::START);
        let chars = self.book.chars_at(self.section, &self.data, pos);
        self.cur = Loc { section: self.section, pos, chars };
        self.toc_idx = self.book.toc_index_at_chars(chars);
        let chapter = self
            .toc_idx
            .and_then(|i| self.book.toc.get(i))
            .map(|e| e.title.clone())
            .or_else(|| self.book.section_title(self.section).map(String::from))
            .unwrap_or_default();
        let total = self.book.total_chars().max(1);
        self.info = PageInfo {
            chapter,
            chars,
            permille: ((chars as u64 * 1000) / total as u64).min(1000) as u32,
            last: self.at_end(),
            chapter_start: self.page == 0 && self.book.sections.get(self.section as usize).map(|s| s.part == 0).unwrap_or(false),
        };
    }

    /// Go to a location.
    pub fn goto<F: Fs>(&mut self, fs: &F, loc: Loc) {
        self.load_section(fs, loc.section);
        self.page = pages::page_of(&self.starts, loc.pos);
        self.sync();
        self.anchor = self.cur;
    }

    /// Go to a global character offset.
    pub fn goto_chars<F: Fs>(&mut self, fs: &F, chars: u32) {
        if let Ok(loc) = self.book.loc_at_chars(fs, chars) {
            self.goto(fs, loc);
        }
    }

    /// Go to a section start.
    pub fn goto_section<F: Fs>(&mut self, fs: &F, section: u16) {
        self.goto(fs, Loc { section, pos: Pos::START, chars: self.book.chars_before_section(section) });
    }

    /// The current location.
    pub fn loc(&self) -> Loc {
        self.cur
    }

    /// Global characters at the current page.
    pub fn chars_now(&self) -> u32 {
        self.cur.chars
    }

    /// Whether at the last page of the book.
    pub fn at_end(&self) -> bool {
        self.section + 1 >= self.book.section_count() && self.page + 1 >= self.starts.len()
    }
    /// Whether at the first page.
    pub fn at_start(&self) -> bool {
        self.section == 0 && self.page == 0
    }

    /// Turn to the next page; false at the end of the book.
    pub fn next_page<F: Fs>(&mut self, fs: &F, now: u32) -> bool {
        if self.page + 1 < self.starts.len() {
            self.page += 1;
        } else if self.section + 1 < self.book.section_count() {
            let s = self.section + 1;
            self.load_section(fs, s);
            self.page = 0;
        } else {
            return false;
        }
        self.turned(now);
        true
    }

    /// Turn to the previous page; false at the start.
    pub fn prev_page<F: Fs>(&mut self, fs: &F, now: u32) -> bool {
        if self.page > 0 {
            self.page -= 1;
        } else if self.section > 0 {
            let s = self.section - 1;
            self.load_section(fs, s);
            self.page = self.starts.len().saturating_sub(1);
        } else {
            return false;
        }
        self.turned(now);
        true
    }

    /// Next chapter start (the next section whose part is 0).
    pub fn next_chapter<F: Fs>(&mut self, fs: &F, now: u32) -> bool {
        let mut s = self.section + 1;
        while (s as usize) < self.book.sections.len() && self.book.sections[s as usize].part != 0 {
            s += 1;
        }
        if (s as usize) >= self.book.sections.len() {
            return false;
        }
        self.goto_section(fs, s);
        self.turned(now);
        true
    }

    /// Previous chapter start (or the start of this one when past its first page).
    pub fn prev_chapter<F: Fs>(&mut self, fs: &F, now: u32) -> bool {
        let cur_first = self.chapter_first_section(self.section);
        if self.section != cur_first || self.page > 0 {
            self.goto_section(fs, cur_first);
        } else if cur_first > 0 {
            let s = self.chapter_first_section(cur_first - 1);
            self.goto_section(fs, s);
        } else {
            return false;
        }
        self.turned(now);
        true
    }

    fn chapter_first_section(&self, section: u16) -> u16 {
        let mut s = section;
        while s > 0 && self.book.sections.get(s as usize).map(|x| x.part != 0).unwrap_or(false) {
            s -= 1;
        }
        s
    }

    fn turned(&mut self, now: u32) {
        self.sync();
        self.anchor = self.cur;
        self.turns_since_gc = self.turns_since_gc.saturating_add(1);
        self.tracker.page(now, self.cur.chars);
    }

    /// Whether the next draw should be a full refresh (every N turns, or an image page).
    pub fn take_gc(&mut self, every: u8) -> bool {
        let every = every.max(1);
        if self.turns_since_gc >= every {
            self.turns_since_gc = 0;
            return true;
        }
        false
    }
    /// Force a full refresh on the next draw.
    pub fn force_gc(&mut self) {
        self.turns_since_gc = u8::MAX;
    }

    /// Lay out the current page (cached until the page changes).
    pub fn page(&mut self) -> Option<&Page> {
        if self.cached_page.is_none() {
            let start = *self.starts.get(self.page)?;
            let page = Paginator::new(&self.data, self.profile, self.geom).page_from(start)?;
            self.cached_page = Some(page);
        }
        self.cached_page.as_ref()
    }

    /// Facts about the current page.
    pub fn info(&mut self) -> PageInfo {
        self.info.clone()
    }

    /// The chapter title for the running head (TOC entry, else the section's title).
    pub fn chapter_title(&self) -> String {
        self.info.chapter.clone()
    }

    /// The chapter number as printed in the TOC title ("Chapter 12", "XII. …", "3 The
    /// Whale"), else the 1-based position among TOC entries of the same depth.
    pub fn chapter_number(&self) -> Option<u32> {
        let i = self.toc_idx?;
        let entry = self.book.toc.get(i)?;
        if let Some(n) = parse_chapter_number(&entry.title) {
            return Some(n);
        }
        let depth = entry.depth;
        Some(self.book.toc.iter().take(i + 1).filter(|e| e.depth == depth).count() as u32)
    }

    /// The TOC index of the current chapter, if any.
    pub fn toc_index(&self) -> Option<usize> {
        self.toc_idx
    }

    /// Number of chapters at the current chapter's depth (the top level when outside any).
    pub fn chapter_count(&self) -> u32 {
        let depth = self
            .toc_idx
            .and_then(|i| self.book.toc.get(i))
            .map(|e| e.depth)
            .or_else(|| self.book.toc.iter().map(|e| e.depth).min())
            .unwrap_or(0);
        self.book.toc.iter().filter(|e| e.depth == depth).count() as u32
    }

    /// Pages in a section (known or estimated).
    pub fn section_pages(&self, s: u16) -> u32 {
        match self.counts.get(s as usize) {
            Some(&n) if n != u16::MAX => n as u32,
            _ => (self.book.sections.get(s as usize).map(|x| x.chars).unwrap_or(0) / CHARS_PER_PAGE_DEFAULT).max(1),
        }
    }

    /// Total pages (known or estimated).
    pub fn total_pages(&self) -> u32 {
        (0..self.book.section_count()).map(|s| self.section_pages(s)).sum::<u32>().max(1)
    }

    /// Current page number in the book, 1-based.
    pub fn page_number(&self) -> u32 {
        (0..self.section).map(|s| self.section_pages(s)).sum::<u32>() + self.page as u32 + 1
    }

    /// Whether every section's page count is known.
    pub fn index_complete(&self) -> bool {
        !self.counts.contains(&u16::MAX)
    }

    /// Idle work: first prefetch the next section when the reader is within two pages of
    /// the boundary (so the turn across it touches no card), else build one more section
    /// index. False when there is nothing left to do. When the index completes, the book's
    /// page total is recorded in the library entry.
    pub fn index_step<F: Fs>(&mut self, fs: &F, lib: &mut Library) -> bool {
        let did = self.prefetch_next(fs)
            || pages::build_next_counted(fs, &self.book, self.key, self.profile, self.geom, &mut self.todo, &mut self.counts).is_some();
        if did {
            self.counts_gen = self.counts_gen.wrapping_add(1);
        }
        if !self.total_recorded && self.index_complete() {
            self.total_recorded = true;
            lib.set_pages_total(self.id, self.key, self.total_pages());
        }
        did
    }

    /// Load the next section's text and page starts ahead of time when the reader is near
    /// the end of this one; drops them again once the reader moves away. Returns whether
    /// anything was loaded.
    pub fn prefetch_next<F: Fs>(&mut self, fs: &F) -> bool {
        let ns = self.section + 1;
        let near = self.page + 2 >= self.starts.len();
        if !near || ns >= self.book.section_count() {
            self.next = None;
            return false;
        }
        if self.next.as_ref().is_some_and(|(s, _, _)| *s == ns) {
            return false;
        }
        let data = self.book.section(fs, ns).unwrap_or_default();
        let starts = pages::get_or_build_counted(fs, &self.book, self.key, ns, &data, self.profile, self.geom, &mut self.counts);
        self.todo.retain(|s| *s != ns);
        self.next = Some((ns, data, starts));
        true
    }

    /// Idle hook kept for platforms that called the old frame pre-render: it now prefetches
    /// the next section (see [`Reader::prefetch_next`]); no frame is rendered.
    pub fn prerender<F: Fs>(&mut self, fs: &F, _settings: &Settings) {
        self.prefetch_next(fs);
    }

    /// The Spine model for the current position.
    pub fn spine_model(&self) -> SpineModel {
        let total = self.total_pages();
        let mut chapters = Vec::new();
        let mut acc = 0u32;
        for (i, s) in self.book.sections.iter().enumerate() {
            if s.part == 0 && i > 0 {
                chapters.push(acc);
            }
            acc += self.section_pages(i as u16);
        }
        SpineModel { total, chapters, current: self.page_number().saturating_sub(1) }
    }

    /// Pages left in the current chapter.
    pub fn pages_left_in_chapter(&self) -> u32 {
        let first = self.chapter_first_section(self.section);
        let mut s = first;
        let mut total = 0u32;
        while (s as usize) < self.book.sections.len() && (s == first || self.book.sections[s as usize].part != 0) {
            total += self.section_pages(s);
            s += 1;
        }
        let before: u32 = (first..self.section).map(|x| self.section_pages(x)).sum::<u32>() + self.page as u32;
        total.saturating_sub(before + 1)
    }

    /// Characters left in the chapter (TOC chapter when there is one).
    pub fn chars_left_in_chapter(&self) -> u32 {
        let chars = self.cur.chars;
        let (_, to, _) = self.book.chapter_bounds(chars);
        to.saturating_sub(chars)
    }

    /// What the next `render` would paint, for callers that keep the last frame.
    pub fn render_key(&self, settings: &Settings) -> RenderKey {
        let head = self.show_head && settings.running_head;
        let spine = self.show_spine && settings.spine;
        RenderKey {
            section: self.section,
            page: self.page as u32,
            key: self.key,
            w: self.w,
            h: self.h,
            counts_gen: self.counts_gen,
            flags: head as u8 | ((spine as u8) << 1),
        }
    }

    /// Render the current page into a frame (text block, Spine, running head).
    pub fn render<F: Fs>(&mut self, fs: &F, f: &mut Frame, settings: &Settings) -> Option<PageInfo> {
        f.clear(Ink::White);
        let ids = page_image_ids(self.page()?);
        if !ids.is_empty() {
            self.images.ensure(fs, &self.book, &ids);
        }
        let page = self.cached_page.as_ref()?;
        quire_layout::render_page(page, f, &self.profile, &self.images);
        if self.show_head && settings.running_head {
            // The head shares the text measure: its right edge is the text block's.
            let baseline = MARGIN + 18;
            crate::widgets::reading_head(f, &self.book.meta.title, &self.info.chapter, self.geom.text.x, self.geom.text.right(), baseline);
        }
        if self.show_spine && settings.spine {
            let top = if self.show_head { MARGIN + 30 } else { MARGIN };
            let r = spine::strip_rect(self.w, self.h, top, MARGIN);
            spine::draw(f, r, &self.spine_model(), Ink::Black);
        }
        Some(self.info.clone())
    }

    /// The Spine rectangle as drawn.
    pub fn spine_rect(&self) -> Rect {
        let top = if self.show_head { MARGIN + 30 } else { MARGIN };
        spine::strip_rect(self.w, self.h, top, MARGIN)
    }

    /// The text block.
    pub fn text_rect(&self) -> Rect {
        self.geom.text
    }

    /// Bookmark this page (toggle); returns whether it is now set.
    pub fn toggle_bookmark<F: Fs>(&mut self, fs: &F, now: u32) -> bool {
        let loc = self.cur;
        let excerpt = self.page_excerpt(80);
        let set = self.marks.toggle_bookmark(loc.section, loc.pos, loc.chars, &excerpt, now);
        let _ = self.marks.save(fs, &self.book.dir);
        set
    }
    /// Whether this page is bookmarked.
    pub fn bookmarked(&self) -> bool {
        self.marks.has_bookmark(self.cur.section, self.cur.pos)
    }

    /// The first words of the page.
    pub fn page_excerpt(&mut self, max_chars: usize) -> String {
        let Some(page) = self.page() else { return String::new() };
        let mut s = String::new();
        for item in &page.items {
            if let quire_layout::DrawItem::Text { text, .. } = item {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(text);
                if s.chars().count() >= max_chars {
                    break;
                }
            }
        }
        if s.chars().count() > max_chars {
            let cut: String = s.chars().take(max_chars).collect();
            return alloc::format!("{}…", cut.trim_end());
        }
        s
    }

    /// The words on the page with their rectangles (for the word cursor).
    pub fn page_words(&mut self) -> Vec<(String, Rect)> {
        let font_h = self.profile.size as i32;
        let Some(page) = self.page() else { return Vec::new() };
        let mut out = Vec::new();
        for item in &page.items {
            if let quire_layout::DrawItem::Text { text, x, y, font, .. } = item {
                // Measures are in quarter pixels; the pen stays in quarters so rounding
                // never drifts along the line.
                let mut pen_q = *x * 4;
                let space_q = quire_gfx::text::measure_text_q(font, " ");
                for word in text.split(' ') {
                    let w_q = quire_gfx::text::measure_text_q(font, word);
                    let clean: String = word.trim_matches(|c: char| !c.is_alphanumeric()).into();
                    if !clean.is_empty() {
                        let x0 = pen_q / 4;
                        let x1 = (pen_q + w_q + 3) / 4;
                        out.push((
                            clean,
                            Rect::new(x0, y - font.ascent(), (x1 - x0).max(1) as u32, (font.ascent() + font.below()).max(font_h) as u32),
                        ));
                    }
                    pen_q += w_q + space_q;
                }
            }
        }
        out
    }

    /// Remember the current location before following a link or note.
    pub fn push_return(&mut self) {
        if self.returns.len() < 8 {
            self.returns.push(self.cur);
        }
    }
    /// Go back to where a link or note was followed from; false when there is nowhere to go.
    pub fn pop_return<F: Fs>(&mut self, fs: &F) -> bool {
        match self.returns.pop() {
            Some(loc) => {
                self.goto(fs, loc);
                self.force_gc();
                true
            }
            None => false,
        }
    }
    /// Whether a return location is pending.
    pub fn has_return(&self) -> bool {
        !self.returns.is_empty()
    }

    /// Save the position into the library entry.
    pub fn save_position(&self, lib: &mut Library) {
        lib.set_loc(self.id, self.cur);
    }

    /// The last page was shown: mark the book finished today. Returns true the first time.
    pub fn reached_end(&self, lib: &mut Library, today: u16) -> bool {
        if lib.get(self.id).map(|e| e.status == Status::Finished).unwrap_or(true) {
            return false;
        }
        lib.reached_end(self.id, today)
    }

    /// End the session and record it (called on close and before sleep).
    pub fn flush_session<F: Fs>(&mut self, fs: &F, lib: &mut Library, stats: &mut Stats, now: u32) {
        let chars = self.cur.chars;
        let t = core::mem::replace(&mut self.tracker, SessionTracker::start(self.id, now, chars, false));
        let s = t.finish(now);
        // Recording folds the session into the book's totals in the index; those only
        // reach the card when the index is marked changed.
        let before = lib.get(self.id).map(|e| e.stats.sessions);
        let _ = stats.record(fs, lib, &s);
        if lib.get(self.id).map(|e| e.stats.sessions) != before {
            lib.touch();
        }
    }

    /// Close: save position and session.
    pub fn close<F: Fs>(&mut self, fs: &F, lib: &mut Library, stats: &mut Stats, now: u32) {
        self.save_position(lib);
        self.flush_session(fs, lib, stats, now);
    }

    /// Seconds read in this session so far.
    pub fn session_secs(&self, now: u32) -> u32 {
        self.tracker.active_now(now)
    }

    /// The library entry.
    pub fn entry<'a>(&self, lib: &'a Library) -> Option<&'a BookEntry> {
        lib.get(self.id)
    }

    /// Time left in the chapter and the book, in seconds, from pace.
    pub fn time_left(&self, lib: &Library, stats: &Stats) -> (u32, u32) {
        let Some(e) = lib.get(self.id) else { return (0, 0) };
        let chars = self.cur.chars;
        let book_left = self.book.total_chars().saturating_sub(chars);
        (stats.secs_for_chars(e, self.chars_left_in_chapter()), stats.secs_for_chars(e, book_left))
    }
}

/// The chapter number printed at the start of a TOC title: a decimal or an upper-case
/// roman numeral as the first word, or as the second after a word such as "Chapter".
pub fn parse_chapter_number(title: &str) -> Option<u32> {
    for (i, word) in title.split_whitespace().take(2).enumerate() {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric());
        if w.is_empty() {
            continue;
        }
        if w.chars().all(|c| c.is_ascii_digit()) {
            return w.parse::<u32>().ok().filter(|n| *n > 0);
        }
        if let Some(n) = roman(w) {
            return Some(n);
        }
        if i > 0 {
            break;
        }
    }
    None
}

/// Value of a well-formed upper-case roman numeral.
fn roman(w: &str) -> Option<u32> {
    if w.is_empty() || w.len() > 9 {
        return None;
    }
    let val = |c: char| -> Option<u32> {
        Some(match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        })
    };
    let mut total = 0u32;
    let mut prev = 0u32;
    for c in w.chars().rev() {
        let v = val(c)?;
        if v < prev {
            total = total.checked_sub(v)?;
        } else {
            total += v;
        }
        prev = v;
    }
    // Reject forms such as "IIII" or "VX" by checking the canonical spelling.
    let mut n = total;
    let mut canon = String::new();
    for (v, s) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while n >= v {
            canon.push_str(s);
            n -= v;
        }
    }
    (canon == w && total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::parse_chapter_number;

    #[test]
    fn chapter_numbers_are_parsed_from_titles() {
        assert_eq!(parse_chapter_number("Chapter 1. Loomings."), Some(1));
        assert_eq!(parse_chapter_number("CHAPTER XII"), Some(12));
        assert_eq!(parse_chapter_number("3 The Whale"), Some(3));
        assert_eq!(parse_chapter_number("XLIV. The Chart"), Some(44));
        assert_eq!(parse_chapter_number("Part I"), Some(1));
        assert_eq!(parse_chapter_number("Epilogue"), None);
        assert_eq!(parse_chapter_number("In which nothing happens"), None);
        assert_eq!(parse_chapter_number("Mix and match"), None);
        assert_eq!(parse_chapter_number("Chapter One"), None);
        assert_eq!(parse_chapter_number("A Night at the Opera"), None);
        assert_eq!(parse_chapter_number("Chapter 0"), None);
    }
}
