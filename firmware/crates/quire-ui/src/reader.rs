//! The open book: sections, page indexes, rendering, position, session tracking.
//! Everything the reading page, compass, contents, go-to, skim and sleep screens need.

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
    /// Current section index.
    pub section: u16,
    data: Vec<u8>,
    starts: Vec<Pos>,
    /// Current page within the section.
    pub page: usize,
    /// Pages per section, once known.
    counts: Vec<Option<u16>>,
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
    /// Pre-rendered next page, when idle time allowed it.
    next_frame: Option<(u16, usize, Frame)>,
    cached_page: Option<Page>,
    /// The current page as last rendered (overlays redraw over it without relayout).
    frame_cache: Option<(u16, usize, Frame)>,
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
            counts: alloc::vec![None; book.sections.len()],
            book,
            id,
            profile,
            geom,
            key,
            section: 0,
            data: Vec::new(),
            starts: Vec::new(),
            page: 0,
            images: ImageStore::new(),
            marks,
            tracker: SessionTracker::start(id, now, loc.chars, false),
            turns_since_gc: 0,
            show_spine: settings.spine,
            show_head: settings.running_head,
            w,
            h,
            next_frame: None,
            cached_page: None,
            frame_cache: None,
            todo: Vec::new(),
            returns: Vec::new(),
        };
        r.load_counts(fs);
        r.goto(fs, loc);
        Ok(r)
    }

    fn load_counts<F: Fs>(&mut self, fs: &F) {
        let c = pages::counts(fs, &self.book, self.key);
        self.todo = c.iter().enumerate().filter(|(_, n)| **n == u16::MAX).map(|(i, _)| i as u16).collect();
        for (i, n) in c.into_iter().enumerate() {
            if let Some(slot) = self.counts.get_mut(i) {
                *slot = (n != u16::MAX).then_some(n);
            }
        }
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

    /// Apply new typography (font size etc.): relayout the current section, keep the position.
    pub fn set_profile<F: Fs>(&mut self, fs: &F, settings: &Settings) {
        let chars = self.chars_now();
        self.profile = settings.profile;
        self.show_spine = settings.spine;
        self.show_head = settings.running_head;
        self.geom = Self::geometry(&self.profile, self.w, self.h, self.show_spine, self.show_head);
        let old_key = self.key;
        self.key = pages::profile_key(&self.profile, &self.geom);
        if old_key != self.key {
            self.counts = alloc::vec![None; self.book.sections.len()];
            self.load_counts(fs);
            pages::purge_except(fs, &self.book, &[self.key, old_key]);
        }
        self.next_frame = None;
        self.cached_page = None;
        self.frame_cache = None;
        let loc = self.book.loc_at_chars(fs, chars).unwrap_or_default();
        self.goto(fs, loc);
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
        self.data = self.book.section(fs, section).unwrap_or_default();
        self.starts = pages::get_or_build(fs, &self.book, self.key, section, &self.data, self.profile, self.geom);
        self.counts[section as usize] = Some(self.starts.len() as u16);
        self.todo.retain(|s| *s != section);
        self.cached_page = None;
        self.next_frame = None;
        self.frame_cache = None;
    }

    /// Go to a location.
    pub fn goto<F: Fs>(&mut self, fs: &F, loc: Loc) {
        self.load_section(fs, loc.section);
        self.page = pages::page_of(&self.starts, loc.pos);
        self.cached_page = None;
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
        let pos = self.starts.get(self.page).copied().unwrap_or(Pos::START);
        Loc { section: self.section, pos, chars: self.book.chars_at(self.section, &self.data, pos) }
    }

    /// Global characters at the current page.
    pub fn chars_now(&self) -> u32 {
        self.loc().chars
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
        self.cached_page = None;
        self.frame_cache = None;
        self.turns_since_gc = self.turns_since_gc.saturating_add(1);
        let chars = self.chars_now();
        self.tracker.page(now, chars);
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

    /// Lay out the current page.
    pub fn page(&mut self) -> Option<Page> {
        if let Some(p) = &self.cached_page {
            return Some(p.clone());
        }
        let start = *self.starts.get(self.page)?;
        let pg = Paginator::new(&self.data, self.profile, self.geom);
        let page = pg.page_from(start)?;
        self.cached_page = Some(page.clone());
        Some(page)
    }

    /// Facts about the current page.
    pub fn info(&mut self) -> PageInfo {
        let chars = self.chars_now();
        let total = self.book.total_chars().max(1);
        let title = self.chapter_title();
        PageInfo {
            chapter: title,
            chars,
            permille: ((chars as u64 * 1000) / total as u64).min(1000) as u32,
            last: self.at_end(),
            chapter_start: self.page == 0 && self.book.sections.get(self.section as usize).map(|s| s.part == 0).unwrap_or(false),
        }
    }

    /// The chapter title for the running head (TOC entry, else the section's title).
    pub fn chapter_title(&self) -> String {
        let loc = self.loc();
        if let Some(i) = self.book.toc_index_at(&loc) {
            if let Some(e) = self.book.toc.get(i) {
                return e.title.clone();
            }
        }
        self.book.section_title(self.section).map(String::from).unwrap_or_default()
    }

    /// Chapter number (1-based position among TOC entries of the same depth), if any.
    pub fn chapter_number(&self) -> Option<u32> {
        let loc = self.loc();
        let i = self.book.toc_index_at(&loc)?;
        let depth = self.book.toc.get(i)?.depth;
        Some(self.book.toc.iter().take(i + 1).filter(|e| e.depth == depth).count() as u32)
    }

    /// The TOC index of the current chapter, if any.
    pub fn toc_index(&self) -> Option<usize> {
        self.book.toc_index_at(&self.loc())
    }

    /// Number of chapters at the top level.
    pub fn chapter_count(&self) -> u32 {
        let d = self.book.toc.iter().map(|e| e.depth).min().unwrap_or(0);
        self.book.toc.iter().filter(|e| e.depth == d).count() as u32
    }

    /// Pages in a section (known or estimated).
    pub fn section_pages(&self, s: u16) -> u32 {
        match self.counts.get(s as usize).copied().flatten() {
            Some(n) => n as u32,
            None => (self.book.sections.get(s as usize).map(|x| x.chars).unwrap_or(0) / CHARS_PER_PAGE_DEFAULT).max(1),
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
        self.counts.iter().all(|c| c.is_some())
    }

    /// Build one more section index in idle time; false when done. When the index
    /// completes, the book's page total is recorded in the library entry.
    pub fn index_step<F: Fs>(&mut self, fs: &F, lib: &mut Library) -> bool {
        let Some(section) = pages::build_next(fs, &self.book, self.key, self.profile, self.geom, &mut self.todo) else {
            return false;
        };
        let n = pages::counts(fs, &self.book, self.key).get(section as usize).copied().unwrap_or(u16::MAX);
        if n != u16::MAX {
            self.counts[section as usize] = Some(n);
        }
        if self.index_complete() {
            lib.set_pages_total(self.id, self.key, self.total_pages());
        }
        true
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
        let chars = self.chars_now();
        let (_, to, _) = self.book.chapter_bounds(chars);
        to.saturating_sub(chars)
    }

    /// Render the current page into a frame (text block, Spine, running head). The result is
    /// cached so overlays can redraw the page without relayout.
    pub fn render<F: Fs>(&mut self, fs: &F, f: &mut Frame, settings: &Settings) -> Option<PageInfo> {
        if let Some((s, p, cached)) = &self.frame_cache {
            if *s == self.section && *p == self.page && cached.width() == f.width() && cached.height() == f.height() {
                f.copy_rect_from(cached, cached.bounds());
                return Some(self.info());
            }
        }
        let info = self.render_uncached(fs, f, settings)?;
        self.frame_cache = Some((self.section, self.page, f.clone()));
        Some(info)
    }

    fn render_uncached<F: Fs>(&mut self, fs: &F, f: &mut Frame, settings: &Settings) -> Option<PageInfo> {
        f.clear(Ink::White);
        let page = self.page()?;
        let ids = page_image_ids(&page);
        if !ids.is_empty() {
            let book = &self.book;
            self.images.ensure(fs, book, &ids);
        }
        quire_layout::render_page(&page, f, &self.profile, &self.images);
        let info = self.info();
        if self.show_head && settings.running_head {
            let baseline = MARGIN + 18;
            let right = if self.show_spine { self.w as i32 - MARGIN - SPINE_W - 6 } else { self.w as i32 - MARGIN };
            crate::widgets::reading_head(
                f,
                &self.book.meta.title,
                &info.chapter,
                self.geom.text.x,
                right.max(self.geom.text.right()),
                baseline,
            );
        }
        if self.show_spine && settings.spine {
            let top = if self.show_head { MARGIN + 30 } else { MARGIN };
            let r = spine::strip_rect(self.w, self.h, top, MARGIN);
            spine::draw(f, r, &self.spine_model(), Ink::Black);
        }
        Some(info)
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

    /// Pre-render the next page in idle time (a page turn then only blits).
    pub fn prerender<F: Fs>(&mut self, fs: &F, settings: &Settings) {
        if self.next_frame.as_ref().map(|(s, p, _)| *s == self.section && *p == self.page + 1).unwrap_or(false) {
            return;
        }
        if self.page + 1 >= self.starts.len() {
            return;
        }
        let saved = (self.section, self.page, self.cached_page.take());
        self.page += 1;
        let mut frame = Frame::new(self.w, self.h);
        self.render_uncached(fs, &mut frame, settings);
        self.page = saved.1;
        self.cached_page = saved.2;
        self.next_frame = Some((saved.0, saved.1 + 1, frame));
    }

    /// Take the pre-rendered frame if it matches the current page.
    pub fn take_prerendered(&mut self) -> Option<Frame> {
        match self.next_frame.take() {
            Some((s, p, fr)) if s == self.section && p == self.page => Some(fr),
            _ => None,
        }
    }

    /// Bookmark this page (toggle); returns whether it is now set.
    pub fn toggle_bookmark<F: Fs>(&mut self, fs: &F, now: u32) -> bool {
        let loc = self.loc();
        let excerpt = self.page_excerpt(80);
        let set = self.marks.toggle_bookmark(loc.section, loc.pos, loc.chars, &excerpt, now);
        let _ = self.marks.save(fs, &self.book.dir);
        set
    }
    /// Whether this page is bookmarked.
    pub fn bookmarked(&self) -> bool {
        let loc = self.loc();
        self.marks.has_bookmark(loc.section, loc.pos)
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
        let Some(page) = self.page() else { return Vec::new() };
        let font_h = self.profile.size as i32;
        let mut out = Vec::new();
        for item in &page.items {
            if let quire_layout::DrawItem::Text { text, x, y, font, .. } = item {
                let mut pen = *x;
                for word in text.split(' ') {
                    let w = quire_gfx::text::measure_text_q(font, word);
                    let clean: String = word.trim_matches(|c: char| !c.is_alphanumeric()).into();
                    if !clean.is_empty() {
                        out.push((
                            clean,
                            Rect::new(pen, y - font.ascent(), w.max(1) as u32, (font.ascent() + font.descent()).max(font_h) as u32),
                        ));
                    }
                    pen += w + quire_gfx::text::measure_text_q(font, " ");
                }
            }
        }
        out
    }

    /// Remember the current location before following a link or note.
    pub fn push_return(&mut self) {
        let loc = self.loc();
        if self.returns.len() < 8 {
            self.returns.push(loc);
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
        lib.set_loc(self.id, self.loc());
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
        let chars = self.chars_now();
        let t = core::mem::replace(&mut self.tracker, SessionTracker::start(self.id, now, chars, false));
        let s = t.finish(now);
        let _ = stats.record(fs, lib, &s);
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
        let chars = self.chars_now();
        let book_left = self.book.total_chars().saturating_sub(chars);
        (stats.secs_for_chars(e, self.chars_left_in_chapter()), stats.secs_for_chars(e, book_left))
    }
}
