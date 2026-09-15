//! Reading statistics.
//!
//! `sessions.log` is append-only, fixed 32-byte records; `daily.bin` is the small
//! aggregate the Analytics screens read (per-day totals, hour and weekday histograms,
//! session-length buckets, records). The aggregate is rebuilt from the log when it is
//! missing or the log has grown behind its back.

use alloc::vec::Vec;
use quire_fs::{Fs, ReadAt, WriteFile};
use serde::{Deserialize, Serialize};

use crate::index::BookEntry;
use crate::time::{self, day_of, hour_of, weekday, DAY};
use crate::{BookId, LibError, LibResult, Library, STATS_DIR};

/// The session log.
pub const LOG_FILE: &str = "/.quire/stats/sessions.log";
/// The aggregate index.
pub const INDEX_FILE: &str = "/.quire/stats/daily.bin";
/// Log record size.
pub const RECORD: usize = 32;
/// A gap longer than this ends a session.
pub const IDLE_SECS: u32 = 300;
/// Per-page time cap.
pub const PAGE_CAP_SECS: u32 = 120;
/// Page times shorter than this do not count toward pace.
pub const PAGE_MIN_SECS: u32 = 5;
/// Days of per-day detail kept (a device keeps a year and a bit).
#[cfg(target_os = "none")]
const KEEP_DAYS: usize = 400;
/// Days of per-day detail kept.
#[cfg(not(target_os = "none"))]
const KEEP_DAYS: usize = 800;
/// Session length buckets of 5 minutes.
const LEN_BUCKETS: usize = 48;
/// Recent session lengths kept for the median ("typical session").
const RECENT_LENGTHS: usize = 32;
/// A day counts toward a streak only with this much reading.
pub const STREAK_MIN_SECS: u32 = 300;

/// One reading session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Book.
    pub book: BookId,
    /// Start (local seconds).
    pub start: u32,
    /// End.
    pub end: u32,
    /// Seconds actively reading (page-capped).
    pub active: u32,
    /// Pages turned.
    pub pages: u16,
    /// Characters advanced.
    pub chars: u32,
    /// Characters counted for pace.
    pub pace_chars: u32,
    /// Seconds counted for pace.
    pub pace_secs: u32,
    /// Bit 0: landscape.
    pub flags: u8,
}

impl Session {
    fn to_record(self) -> [u8; RECORD] {
        let mut r = [0u8; RECORD];
        r[0..8].copy_from_slice(&self.book.0.to_le_bytes());
        r[8..12].copy_from_slice(&self.start.to_le_bytes());
        r[12..16].copy_from_slice(&self.end.to_le_bytes());
        r[16..20].copy_from_slice(&self.active.to_le_bytes());
        r[20..22].copy_from_slice(&self.pages.to_le_bytes());
        r[22..26].copy_from_slice(&self.chars.to_le_bytes());
        r[26..29].copy_from_slice(&self.pace_chars.to_le_bytes()[..3]);
        r[29..31].copy_from_slice(&(self.pace_secs.min(u16::MAX as u32) as u16).to_le_bytes());
        r[31] = self.flags | 0x80;
        r
    }
    /// Decode a record, rejecting anything a real session could not have produced (a
    /// corrupt log must not be able to hang or overflow the aggregate).
    fn from_record(r: &[u8]) -> Option<Session> {
        if r.len() < RECORD || r[31] & 0x80 == 0 {
            return None;
        }
        let u32le = |i: usize| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
        let s = Session {
            book: BookId(u64::from_le_bytes(r[0..8].try_into().ok()?)),
            start: u32le(8),
            end: u32le(12),
            active: u32le(16),
            pages: u16::from_le_bytes([r[20], r[21]]),
            chars: u32le(22),
            pace_chars: u32::from_le_bytes([r[26], r[27], r[28], 0]),
            pace_secs: u16::from_le_bytes([r[29], r[30]]) as u32,
            flags: r[31] & 0x7f,
        };
        let span = s.end.checked_sub(s.start)?;
        // A session spans at most a day; active time cannot exceed the span (plus one page cap).
        if span > 2 * DAY || s.active > span + PAGE_CAP_SECS || s.start < 946_684_800 || s.start > 4_102_444_800 {
            return None;
        }
        Some(s)
    }
    /// Seconds actively reading (page-time capped).
    pub fn active_secs(&self) -> u32 {
        self.active
    }
    /// Wall-clock span in seconds.
    pub fn span_secs(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

/// Tracks the session in progress.
#[derive(Clone, Debug)]
pub struct SessionTracker {
    book: BookId,
    start: u32,
    last: u32,
    last_chars: u32,
    active: u32,
    pages: u16,
    chars: u32,
    pace_chars: u32,
    pace_secs: u32,
    flags: u8,
}

impl SessionTracker {
    /// Begin at `now` with the reader at `chars`.
    pub fn start(book: BookId, now: u32, chars: u32, landscape: bool) -> Self {
        SessionTracker {
            book,
            start: now,
            last: now,
            last_chars: chars,
            active: 0,
            pages: 0,
            chars: 0,
            pace_chars: 0,
            pace_secs: 0,
            flags: landscape as u8,
        }
    }
    /// The book being read.
    pub fn book(&self) -> BookId {
        self.book
    }
    /// A page was shown at `now`; the reader is now at `chars`.
    pub fn page(&mut self, now: u32, chars: u32) {
        let gap = now.saturating_sub(self.last);
        let delta = chars.saturating_sub(self.last_chars);
        if gap <= IDLE_SECS {
            self.active += gap.min(PAGE_CAP_SECS);
            if (PAGE_MIN_SECS..=PAGE_CAP_SECS).contains(&gap) && delta > 0 && delta < 20_000 {
                self.pace_chars += delta;
                self.pace_secs += gap;
            }
        }
        if chars > self.last_chars {
            self.chars += delta;
        }
        self.pages = self.pages.saturating_add(1);
        self.last = now;
        self.last_chars = chars;
    }
    /// Whether the reader has been idle past the limit.
    pub fn is_idle(&self, now: u32) -> bool {
        now.saturating_sub(self.last) > IDLE_SECS
    }
    /// Seconds active so far, including the current page.
    pub fn active_now(&self, now: u32) -> u32 {
        let gap = now.saturating_sub(self.last);
        self.active + if gap <= IDLE_SECS { gap.min(PAGE_CAP_SECS) } else { 0 }
    }
    /// End the session at `now`.
    pub fn finish(self, now: u32) -> Session {
        let gap = now.saturating_sub(self.last);
        let (end, active) = if gap <= IDLE_SECS { (now, self.active + gap.min(PAGE_CAP_SECS)) } else { (self.last, self.active) };
        Session {
            book: self.book,
            start: self.start,
            end: end.max(self.start),
            active,
            pages: self.pages,
            chars: self.chars,
            pace_chars: self.pace_chars,
            pace_secs: self.pace_secs,
            flags: self.flags,
        }
    }
}

/// One day's totals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DayStat {
    /// Day number.
    pub day: u16,
    /// Seconds read.
    pub secs: u32,
    /// Pages turned.
    pub pages: u16,
    /// Characters read.
    pub chars: u32,
    /// Sessions.
    pub sessions: u8,
}

/// The aggregate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    version: u16,
    /// Bytes of the log folded into this aggregate.
    pub log_len: u64,
    /// Per-day totals, ascending.
    pub days: Vec<DayStat>,
    /// Seconds by hour of day (session time is spread across the hours it spans).
    pub hours: [u32; 24],
    /// Seconds by weekday (0 = Monday).
    pub weekdays: [u32; 7],
    /// Sessions by length bucket (5 min each).
    pub lengths: Vec<u16>,
    /// Longest session (seconds, book, start).
    pub longest: (u32, BookId, u32),
    /// Total sessions.
    pub sessions: u32,
    /// Total seconds.
    pub secs: u32,
    /// Total pages.
    pub pages: u32,
    /// Pace totals.
    pub pace_chars: u64,
    /// Pace seconds.
    pub pace_secs: u64,
    /// Sessions started 23:00–04:00.
    pub night_sessions: u16,
    /// Daily goal in minutes.
    pub goal_minutes: u16,
    /// Daily goal in pages (used when `goal_pages` is set).
    pub goal_page_count: u16,
    /// Count the daily goal in pages instead of minutes.
    pub goal_pages: bool,
    /// Yearly goal in books.
    pub goal_books: u16,
    /// Recent session lengths in seconds (ring, newest last).
    pub recent_lengths: Vec<u16>,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            version: 1,
            log_len: 0,
            days: Vec::new(),
            hours: [0; 24],
            weekdays: [0; 7],
            lengths: alloc::vec![0; LEN_BUCKETS],
            longest: (0, BookId(0), 0),
            sessions: 0,
            secs: 0,
            pages: 0,
            pace_chars: 0,
            pace_secs: 0,
            night_sessions: 0,
            goal_minutes: 30,
            goal_page_count: 30,
            goal_pages: false,
            goal_books: 24,
            recent_lengths: Vec::new(),
        }
    }
}

/// A time range for the overview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    /// Today.
    Today,
    /// The last seven days.
    Week,
    /// This calendar month.
    Month,
    /// This calendar year.
    Year,
    /// Everything.
    All,
}

/// Totals for a range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Seconds.
    pub secs: u32,
    /// Pages.
    pub pages: u32,
    /// Sessions.
    pub sessions: u32,
    /// Days with reading.
    pub days: u32,
    /// Characters.
    pub chars: u64,
}

impl Totals {
    /// Pages per hour, or 0.
    pub fn pages_per_hour(&self) -> u32 {
        if self.secs < 60 {
            return 0;
        }
        (self.pages as u64 * 3600 / self.secs as u64) as u32
    }
}

/// An earned award.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Award {
    /// Name.
    pub name: &'static str,
    /// Day earned.
    pub day: u16,
}

impl Stats {
    /// Load the aggregate and fold in any log records it has not seen.
    pub fn load<F: Fs>(fs: &F) -> Stats {
        let mut s: Stats = fs.read_to_vec(INDEX_FILE).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
        if s.version != 1 || s.lengths.len() != LEN_BUCKETS {
            s = Stats::default();
        }
        let log_len = fs.open(LOG_FILE).map(|f| f.len()).unwrap_or(0);
        if log_len < s.log_len {
            // The log was truncated or replaced: rebuild from scratch, keeping goals.
            let (gm, gp, gpg, gb) = (s.goal_minutes, s.goal_page_count, s.goal_pages, s.goal_books);
            s = Stats { goal_minutes: gm, goal_page_count: gp, goal_pages: gpg, goal_books: gb, ..Stats::default() };
        }
        if log_len > s.log_len {
            let _ = s.fold_log(fs);
            let _ = s.save(fs);
        }
        s
    }

    /// Save the aggregate.
    pub fn save<F: Fs>(&self, fs: &F) -> LibResult<()> {
        if !fs.exists(STATS_DIR) {
            fs.mkdir_all(STATS_DIR)?;
        }
        let enc = postcard::to_allocvec(self).map_err(|_| LibError::Corrupt("stats encode"))?;
        Ok(fs.write_atomic(INDEX_FILE, &enc)?)
    }

    fn fold_log<F: Fs>(&mut self, fs: &F) -> LibResult<()> {
        let f = fs.open(LOG_FILE)?;
        let len = f.len();
        let mut pos = self.log_len - self.log_len % RECORD as u64;
        let mut buf = alloc::vec![0u8; RECORD * 64];
        while pos < len {
            let n = f.read_at(pos, &mut buf)?;
            if n < RECORD {
                break;
            }
            for r in buf[..n - n % RECORD].as_chunks::<RECORD>().0 {
                if let Some(s) = Session::from_record(r) {
                    self.apply(&s);
                }
            }
            pos += (n - n % RECORD) as u64;
        }
        self.log_len = len;
        Ok(())
    }

    /// Record a finished session: append to the log, fold it in, and update the book's
    /// totals in the library. Sessions under ten seconds are dropped.
    pub fn record<F: Fs>(&mut self, fs: &F, lib: &mut Library, s: &Session) -> LibResult<()> {
        if s.active < 10 && s.pages == 0 {
            return Ok(());
        }
        if !fs.exists(STATS_DIR) {
            fs.mkdir_all(STATS_DIR)?;
        }
        {
            let mut w = fs.append(LOG_FILE)?;
            w.write_all(&s.to_record())?;
            w.flush()?;
        }
        self.apply(s);
        self.log_len += RECORD as u64;
        self.save(fs)?;
        if let Some(b) = lib.get_mut(s.book) {
            b.stats.seconds = b.stats.seconds.saturating_add(s.active);
            b.stats.pages = b.stats.pages.saturating_add(s.pages as u32);
            b.stats.sessions = b.stats.sessions.saturating_add(1);
            if b.stats.started.is_none() {
                b.stats.started = Some(day_of(s.start));
            }
            if s.pace_secs >= PAGE_MIN_SECS && s.pace_chars > 0 {
                b.stats.recent.push((s.pace_chars, s.pace_secs));
                while b.stats.recent.len() > 7 {
                    b.stats.recent.remove(0);
                }
            }
        }
        Ok(())
    }

    fn apply(&mut self, s: &Session) {
        let day = day_of(s.start);
        let i = self.days.partition_point(|d| d.day < day);
        if self.days.get(i).map(|d| d.day != day).unwrap_or(true) {
            self.days.insert(i, DayStat { day, ..Default::default() });
            if self.days.len() > KEEP_DAYS {
                self.days.remove(0);
            }
        }
        if let Some(d) = self.days.iter_mut().find(|d| d.day == day) {
            d.secs = d.secs.saturating_add(s.active);
            d.pages = d.pages.saturating_add(s.pages);
            d.chars = d.chars.saturating_add(s.chars);
            d.sessions = d.sessions.saturating_add(1);
        }
        if self.recent_lengths.len() >= RECENT_LENGTHS {
            self.recent_lengths.remove(0);
        }
        self.recent_lengths.push(s.active.min(u16::MAX as u32) as u16);
        // Spread active seconds over the hours the session spanned (bounded: sessions are
        // validated to a day or two, so this loop runs at most ~50 times).
        let span = s.end.saturating_sub(s.start).max(1);
        let mut t = s.start;
        let mut left = s.active;
        let mut steps = 0;
        while t < s.end && left > 0 && steps < 64 {
            steps += 1;
            let next = ((t as u64 / 3600 + 1) * 3600).min(u32::MAX as u64) as u32;
            let seg = next.min(s.end).saturating_sub(t);
            let share = ((s.active as u64 * seg as u64) / span as u64) as u32;
            let share = share.min(left);
            self.hours[hour_of(t) as usize] = self.hours[hour_of(t) as usize].saturating_add(share);
            left -= share;
            t = next;
        }
        if left > 0 {
            self.hours[hour_of(s.start) as usize] = self.hours[hour_of(s.start) as usize].saturating_add(left);
        }
        let wd = weekday(day) as usize;
        self.weekdays[wd] = self.weekdays[wd].saturating_add(s.active);
        let bucket = ((s.active / 300) as usize).min(LEN_BUCKETS - 1);
        self.lengths[bucket] = self.lengths[bucket].saturating_add(1);
        if s.active > self.longest.0 {
            self.longest = (s.active, s.book, s.start);
        }
        self.sessions = self.sessions.saturating_add(1);
        self.secs = self.secs.saturating_add(s.active);
        self.pages = self.pages.saturating_add(s.pages as u32);
        self.pace_chars = self.pace_chars.saturating_add(s.pace_chars as u64);
        self.pace_secs = self.pace_secs.saturating_add(s.pace_secs as u64);
        let h = hour_of(s.start);
        if !(4..23).contains(&h) {
            self.night_sessions = self.night_sessions.saturating_add(1);
        }
    }

    /// The day record for a day.
    pub fn day(&self, day: u16) -> Option<&DayStat> {
        self.days.iter().find(|d| d.day == day)
    }

    /// The day range a tab covers, ending today.
    pub fn range_days(range: Range, today: u16) -> (u16, u16) {
        let (y, m, _) = time::civil(today);
        let from = match range {
            Range::Today => today,
            Range::Week => today.saturating_sub(6),
            Range::Month => time::from_civil(y, m, 1),
            Range::Year => time::from_civil(y, 1, 1),
            Range::All => 0,
        };
        (from, today)
    }

    /// Totals over a range ending today.
    pub fn totals(&self, range: Range, today: u16) -> Totals {
        let (from, to) = Self::range_days(range, today);
        let mut t = self.totals_between(from, to);
        if range == Range::All {
            // Days beyond the kept window still count in the running totals.
            t.secs = t.secs.max(self.secs);
            t.pages = t.pages.max(self.pages);
            t.sessions = t.sessions.max(self.sessions);
        }
        t
    }

    /// Totals over an inclusive day range.
    pub fn totals_between(&self, from: u16, to: u16) -> Totals {
        let mut t = Totals::default();
        for d in self.days.iter().filter(|d| d.day >= from && d.day <= to) {
            t.secs = t.secs.saturating_add(d.secs);
            t.pages = t.pages.saturating_add(d.pages as u32);
            t.sessions = t.sessions.saturating_add(d.sessions as u32);
            t.chars = t.chars.saturating_add(d.chars as u64);
            if d.secs > 0 {
                t.days += 1;
            }
        }
        t
    }

    /// Totals per calendar month of a year (the Year tab's ink line).
    pub fn month_totals(&self, y: u16) -> Vec<Totals> {
        (1..=12u8)
            .map(|m| {
                let a = time::from_civil(y, m, 1);
                self.totals_between(a, a + time::days_in_month(y, m) as u16 - 1)
            })
            .collect()
    }

    /// Per-day seconds over an inclusive range (the Week/Month ink lines).
    pub fn day_secs_between(&self, from: u16, to: u16) -> Vec<u32> {
        (from..=to).map(|d| self.day(d).map(|x| x.secs).unwrap_or(0)).collect()
    }

    /// Minutes read per hour of `day` — the overview's ink line — approximated from
    /// that day's sessions being unavailable here, so it uses the all-time hour profile
    /// scaled to the day's total. For exact per-day curves use [`Stats::day_curve`].
    pub fn hour_profile(&self) -> [u32; 24] {
        self.hours
    }

    /// Minutes per hour for one day, read from the log (bounded scan of the tail).
    pub fn day_curve<F: Fs>(&self, fs: &F, day: u16) -> [u16; 24] {
        let mut out = [0u16; 24];
        for s in self.sessions_between(fs, day as u32 * DAY, (day as u32 + 1) * DAY, 400) {
            let span = s.end.saturating_sub(s.start).max(1);
            let mut t = s.start;
            let mut steps = 0;
            while t < s.end && steps < 64 {
                steps += 1;
                let next = ((t as u64 / 3600 + 1) * 3600).min(u32::MAX as u64) as u32;
                let seg = next.min(s.end).saturating_sub(t);
                let share = (s.active as u64 * seg as u64 / span as u64) as u32;
                let h = hour_of(t) as usize;
                out[h] = out[h].saturating_add((share / 60) as u16);
                t = next;
            }
        }
        out
    }

    /// Whether a day counts toward a streak (at least [`STREAK_MIN_SECS`] of reading).
    fn counts_for_streak(d: &DayStat) -> bool {
        d.secs >= STREAK_MIN_SECS
    }

    /// Current and longest streaks of days with real reading, as of `today`.
    pub fn streaks(&self, today: u16) -> (u32, u32) {
        let mut longest = 0u32;
        let mut run = 0u32;
        let mut prev: Option<u16> = None;
        for d in self.days.iter().filter(|d| Self::counts_for_streak(d)) {
            run = match prev {
                Some(p) if d.day == p + 1 => run + 1,
                _ => 1,
            };
            longest = longest.max(run);
            prev = Some(d.day);
        }
        let current = match prev {
            Some(p) if p == today || p + 1 == today => run,
            _ => 0,
        };
        (current, longest)
    }

    /// Percent of the daily goal reached on a day (minutes or pages, per the goal kind).
    pub fn goal_fraction(&self, day: u16) -> u8 {
        let d = self.day(day);
        if self.goal_pages {
            let goal = (self.goal_page_count as u32).max(1);
            let pages = d.map(|d| d.pages as u32).unwrap_or(0);
            return ((pages as u64 * 100) / goal as u64).min(100) as u8;
        }
        let goal = (self.goal_minutes as u32 * 60).max(60);
        let secs = d.map(|d| d.secs).unwrap_or(0);
        ((secs as u64 * 100) / goal as u64).min(100) as u8
    }

    /// Favourite hour (most seconds), if any reading.
    pub fn favourite_hour(&self) -> Option<u8> {
        let (h, v) = self.hours.iter().enumerate().max_by_key(|(_, v)| **v)?;
        (*v > 0).then_some(h as u8)
    }
    /// Favourite weekday (0 = Monday).
    pub fn favourite_weekday(&self) -> Option<u8> {
        let (d, v) = self.weekdays.iter().enumerate().max_by_key(|(_, v)| **v)?;
        (*v > 0).then_some(d as u8)
    }
    /// Typical (median) session length in seconds, from the recent sessions.
    pub fn typical_session_secs(&self) -> u32 {
        if self.recent_lengths.is_empty() {
            // Only the buckets survive from before the ring existed.
            let total: u32 = self.lengths.iter().map(|n| *n as u32).sum();
            if total == 0 {
                return 0;
            }
            let mut acc = 0u32;
            for (i, n) in self.lengths.iter().enumerate() {
                acc += *n as u32;
                if acc * 2 >= total {
                    return (i as u32 * 300) + 150;
                }
            }
            return 0;
        }
        let mut v = self.recent_lengths.clone();
        v.sort_unstable();
        v[v.len() / 2] as u32
    }

    /// Global reading pace in characters per second × 1000 (0 when unknown).
    pub fn pace_milli_cps(&self) -> u32 {
        if self.pace_secs < 60 {
            return 0;
        }
        ((self.pace_chars * 1000) / self.pace_secs).min(u32::MAX as u64) as u32
    }

    /// Reading pace for a book in characters per second × 1000: its last seven sessions,
    /// else the global pace, else 900 characters a minute.
    pub fn pace_milli_cps_for(&self, book: &BookEntry) -> u32 {
        let (c, s): (u64, u64) = book.stats.recent.iter().fold((0, 0), |(c, s), (rc, rs)| (c + *rc as u64, s + *rs as u64));
        let milli_cps = if s >= 60 && c > 0 {
            (c * 1000 / s) as u32
        } else {
            let g = self.pace_milli_cps();
            if g > 0 {
                g
            } else {
                15_000
            }
        };
        milli_cps.max(500)
    }

    /// Seconds to read `chars` characters of a book at its pace.
    pub fn secs_for_chars(&self, book: &BookEntry, chars: u32) -> u32 {
        let milli_cps = self.pace_milli_cps_for(book);
        ((chars as u64 * 1000) / milli_cps as u64).min(u32::MAX as u64) as u32
    }

    /// Seconds left in a book from its pace.
    pub fn time_left_secs(&self, book: &BookEntry) -> u32 {
        self.secs_for_chars(book, book.chars_left())
    }

    /// Average seconds per day over the last seven days (at least ten minutes).
    pub fn daily_average_secs(&self, today: u16) -> u32 {
        let t = self.totals(Range::Week, today);
        (t.secs / 7).max(600)
    }

    /// The day a book would be finished, given seconds left and the daily average.
    pub fn finish_day(&self, today: u16, secs_left: u32) -> u16 {
        let avg = self.daily_average_secs(today);
        today.saturating_add(secs_left.div_ceil(avg).min(3650) as u16)
    }

    /// Per-day seconds for a calendar month.
    pub fn month(&self, y: u16, m: u8) -> Vec<(u8, u32)> {
        let first = time::from_civil(y, m, 1);
        let n = time::days_in_month(y, m);
        (0..n).map(|i| (i + 1, self.day(first + i as u16).map(|d| d.secs).unwrap_or(0))).collect()
    }

    /// Awards earned so far.
    pub fn awards(&self, lib: &Library) -> Vec<Award> {
        let mut out = Vec::new();
        if let Some(d) = lib.books.iter().filter_map(|b| b.stats.finished).min() {
            out.push(Award { name: "First book", day: d });
        }
        // 7-day streak: the day a run of seven was completed.
        let mut run = 0u32;
        let mut prev: Option<u16> = None;
        for d in self.days.iter().filter(|d| Self::counts_for_streak(d)) {
            run = match prev {
                Some(p) if d.day == p + 1 => run + 1,
                _ => 1,
            };
            prev = Some(d.day);
            if run == 7 {
                out.push(Award { name: "7-day streak", day: d.day });
                break;
            }
        }
        let mut acc = 0u32;
        for d in &self.days {
            acc = acc.saturating_add(d.secs);
            if acc >= 100 * 3600 {
                out.push(Award { name: "100 hours", day: d.day });
                break;
            }
        }
        if self.night_sessions >= 10 {
            if let Some(d) = self.days.last() {
                out.push(Award { name: "Night owl", day: d.day });
            }
        }
        if lib.books.iter().filter(|b| b.stats.finished.is_some()).count() >= 10 {
            if let Some(d) = lib.books.iter().filter_map(|b| b.stats.finished).max() {
                out.push(Award { name: "Ten books", day: d });
            }
        }
        out.sort_by_key(|a| a.day);
        out
    }

    /// Books finished in an inclusive day range.
    pub fn books_finished_between(lib: &Library, from: u16, to: u16) -> u32 {
        lib.books.iter().filter(|bk| bk.stats.finished.map(|d| d >= from && d <= to).unwrap_or(false)).count() as u32
    }

    /// Books finished in a calendar year.
    pub fn books_finished_in_year(lib: &Library, y: u16) -> u32 {
        let (a, b) = (time::from_civil(y, 1, 1), time::from_civil(y, 12, 31));
        lib.books.iter().filter(|bk| bk.stats.finished.map(|d| d >= a && d <= b).unwrap_or(false)).count() as u32
    }

    /// Books finished in a calendar month.
    pub fn books_finished_in_month(lib: &Library, y: u16, m: u8) -> u32 {
        let a = time::from_civil(y, m, 1);
        let b = a + time::days_in_month(y, m) as u16 - 1;
        lib.books.iter().filter(|bk| bk.stats.finished.map(|d| d >= a && d <= b).unwrap_or(false)).count() as u32
    }

    /// Longest streak within a year.
    pub fn longest_streak_in_year(&self, y: u16) -> u32 {
        let (a, b) = (time::from_civil(y, 1, 1), time::from_civil(y, 12, 31));
        let mut longest = 0u32;
        let mut run = 0u32;
        let mut prev: Option<u16> = None;
        for d in self.days.iter().filter(|d| Self::counts_for_streak(d) && d.day >= a && d.day <= b) {
            run = match prev {
                Some(p) if d.day == p + 1 => run + 1,
                _ => 1,
            };
            longest = longest.max(run);
            prev = Some(d.day);
        }
        longest
    }

    /// Sessions from the log whose start lies in `[from, to)`, newest first, bounded.
    pub fn sessions_between<F: Fs>(&self, fs: &F, from: u32, to: u32, limit: usize) -> Vec<Session> {
        let mut out = Vec::new();
        let Ok(f) = fs.open(LOG_FILE) else { return out };
        let len = f.len();
        let mut pos = len - len % RECORD as u64;
        let mut buf = alloc::vec![0u8; RECORD * 64];
        let mut scanned = 0usize;
        while pos > 0 && out.len() < limit && scanned < 4000 {
            let chunk = (pos.min(buf.len() as u64)) as usize;
            pos -= chunk as u64;
            let Ok(n) = f.read_at(pos, &mut buf[..chunk]) else { break };
            for r in buf[..n].as_chunks::<RECORD>().0.iter().rev() {
                scanned += 1;
                if let Some(s) = Session::from_record(r) {
                    if s.start < from {
                        return out;
                    }
                    if s.start < to {
                        out.push(s);
                        if out.len() >= limit {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    /// Favourite hour (most active seconds) over the sessions that started in the days
    /// `[from, to]`, read back from the log; `None` when nothing was read then.
    /// (Added for the year-in-review poster, which must not show all-time figures on an
    /// empty year; the aggregate keeps hours for all time only.)
    pub fn favourite_hour_between<F: Fs>(&self, fs: &F, from: u16, to: u16) -> Option<u8> {
        let mut hours = [0u32; 24];
        for s in self.sessions_between(fs, from as u32 * DAY, (to as u32 + 1) * DAY, 4000) {
            hours[hour_of(s.start) as usize % 24] += s.active_secs();
        }
        let (h, v) = hours.iter().enumerate().max_by_key(|(_, v)| **v)?;
        (*v > 0).then_some(h as u8)
    }

    /// Longest session (active seconds, book, start) among those started in the days
    /// `[from, to]`, from the log.
    pub fn longest_session_between<F: Fs>(&self, fs: &F, from: u16, to: u16) -> Option<(u32, BookId, u32)> {
        self.sessions_between(fs, from as u32 * DAY, (to as u32 + 1) * DAY, 4000)
            .into_iter()
            .map(|s| (s.active_secs(), s.book, s.start))
            .max_by_key(|s| s.0)
            .filter(|s| s.0 > 0)
    }

    /// The most recent sessions, newest first.
    pub fn recent<F: Fs>(&self, fs: &F, limit: usize) -> Vec<Session> {
        self.sessions_between(fs, 0, u32::MAX, limit)
    }

    /// A book's sessions in the last `days` days, newest first.
    pub fn book_sessions<F: Fs>(&self, fs: &F, book: BookId, today: u16, days: u16, limit: usize) -> Vec<Session> {
        let from = today.saturating_sub(days) as u32 * DAY;
        self.sessions_between(fs, from, u32::MAX, 2000).into_iter().filter(|s| s.book == book).take(limit).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{BookStats, IngestState, Loc, Status};
    use crate::BookEntry;
    use quire_fs::host::HostFs;

    fn day(y: u16, m: u8, d: u8, h: u32) -> u32 {
        time::from_civil(y, m, d) as u32 * DAY + h * 3600
    }

    #[test]
    fn tracker_caps_page_time_and_ignores_idle() {
        let t0 = 1_700_000_000u32;
        let mut t = SessionTracker::start(BookId(7), t0, 0, false);
        t.page(t0 + 30, 400); // 30 s, counts for pace
        t.page(t0 + 32, 800); // 2 s, too quick for pace
        t.page(t0 + 500, 1200); // 468 s gap: longer than the idle limit, ignored
        let s = t.finish(t0 + 510);
        assert_eq!(s.pages, 3);
        assert_eq!(s.active, 30 + 2 + 10);
        assert_eq!(s.pace_chars, 400);
        assert_eq!(s.pace_secs, 30);
        assert_eq!(s.chars, 1200);
        let r = s.to_record();
        assert_eq!(Session::from_record(&r), Some(s));
    }

    #[test]
    fn stats_fold_and_query() {
        let dir = std::env::temp_dir().join(alloc::format!("quire-stats-{}", std::process::id()));
        let fs = HostFs::new(&dir);
        let mut lib = Library::load(&fs);
        lib.upsert(BookEntry {
            id: BookId(1),
            path: "/Books/a.epub".into(),
            size: 1,
            format: quire_doc::Format::Epub,
            title: "A".into(),
            authors: Vec::new(),
            series: None,
            year: None,
            language: "en".into(),
            sections: 1,
            chars: 100_000,
            has_cover: false,
            ingest: IngestState::Ready,
            error: None,
            added: 0,
            last_opened: 0,
            loc: Loc { section: 0, pos: quire_layout::Pos::START, chars: 40_000 },
            status: Status::Reading,
            collections: Vec::new(),
            stats: BookStats::default(),
            missing: false,
            pages_total: None,
        });
        let mut st = Stats::load(&fs);
        // Three days in a row, 21:00 sessions, then a gap.
        for d in 10..13u8 {
            let start = day(2026, 9, d, 21);
            let s = Session {
                book: BookId(1),
                start,
                end: start + 1800,
                active: 1800,
                pages: 30,
                chars: 9000,
                pace_chars: 9000,
                pace_secs: 1500,
                flags: 0,
            };
            st.record(&fs, &mut lib, &s).unwrap();
        }
        let today = time::from_civil(2026, 9, 13);
        assert_eq!(st.streaks(today), (3, 3));
        assert_eq!(st.totals(Range::Week, today).secs, 5400);
        assert_eq!(st.totals(Range::Today, today).secs, 0);
        assert_eq!(st.totals(Range::Month, today).pages, 90);
        assert_eq!(st.favourite_hour(), Some(21));
        assert_eq!(st.favourite_weekday(), Some(weekday(time::from_civil(2026, 9, 12))));
        assert_eq!(st.typical_session_secs(), 1800);
        assert_eq!(st.month_totals(2026)[8].pages, 90);
        assert_eq!(st.totals_between(time::from_civil(2026, 9, 11), time::from_civil(2026, 9, 11)).secs, 1800);
        assert_eq!(st.totals(Range::All, today).pages_per_hour(), 60);
        // Pace: 6 chars/s → 60 000 chars left = 10 000 s.
        let b = lib.get(BookId(1)).unwrap();
        assert_eq!(b.stats.recent.len(), 3);
        assert_eq!(st.time_left_secs(b), 10_000);
        assert_eq!(st.finish_day(today, 10_000), today + 10_000u32.div_ceil(5400 / 7) as u16);
        // Reload folds nothing new; a fresh aggregate rebuilds from the log.
        let again = Stats::load(&fs);
        assert_eq!(again.sessions, 3);
        let _ = fs.remove(INDEX_FILE);
        let rebuilt = Stats::load(&fs);
        assert_eq!(rebuilt.days.len(), 3);
        assert_eq!(rebuilt.hours[21], 5400);
        let recent = rebuilt.recent(&fs, 2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].start, day(2026, 9, 12, 21));
        assert_eq!(rebuilt.day_curve(&fs, time::from_civil(2026, 9, 12))[21], 30);
        assert_eq!(rebuilt.book_sessions(&fs, BookId(1), today, 30, 10).len(), 3);
        assert!(rebuilt.awards(&lib).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
