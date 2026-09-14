//! Persisted settings (`/.quire/settings.bin`).

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_layout::Profile;
use serde::{Deserialize, Serialize};

/// Where settings live.
pub const SETTINGS_FILE: &str = "/.quire/settings.bin";

/// Which sleep screen to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SleepVariant {
    /// Full-bleed cover with a band.
    #[default]
    Cover,
    /// Finish-by poster.
    Poster,
    /// Quote of the day.
    Quote,
    /// Custom image from the card.
    Custom,
    /// The page itself, screened.
    QuickResume,
    /// Blank paper.
    Blank,
}

impl SleepVariant {
    /// All variants in picker order.
    pub const ALL: [SleepVariant; 6] = [
        SleepVariant::Cover,
        SleepVariant::Poster,
        SleepVariant::Quote,
        SleepVariant::Custom,
        SleepVariant::QuickResume,
        SleepVariant::Blank,
    ];
    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            SleepVariant::Cover => "Cover",
            SleepVariant::Poster => "Poster",
            SleepVariant::Quote => "Quote",
            SleepVariant::Custom => "Custom",
            SleepVariant::QuickResume => "Quick resume",
            SleepVariant::Blank => "Blank",
        }
    }
}

/// What the side keys do on the reading page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SideKeys {
    /// Previous / next page.
    #[default]
    Pages,
    /// Previous / next chapter.
    Chapters,
}

/// Rotation of custom sleep images.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageRotation {
    /// Always the chosen image.
    #[default]
    Fixed,
    /// A new one each sleep.
    EachSleep,
    /// A new one each day.
    Daily,
}

/// The settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Schema version.
    pub version: u16,
    /// First-run wizard completed.
    pub first_run_done: bool,
    /// UI language code.
    pub language: String,
    /// Reading typography.
    pub profile: Profile,
    /// Anti-alias text on the 2-bit path.
    pub antialias: bool,
    /// Inverted (dark) mode.
    pub inverted: bool,
    /// Large UI (×1.25 rows).
    pub large_ui: bool,
    /// Hide apps and games from Jump.
    pub simple_mode: bool,
    /// Sleep screen.
    pub sleep: SleepVariant,
    /// Cover sleep: show the title band.
    pub sleep_band: bool,
    /// Poster sleep: show streak instead of finish-by.
    pub sleep_streak: bool,
    /// Quote sleep: use quotes.txt from the card.
    pub sleep_quotes_card: bool,
    /// Custom image rotation.
    pub sleep_rotation: ImageRotation,
    /// Custom image folder.
    pub sleep_folder: String,
    /// Custom image chosen (file name).
    pub sleep_image: Option<String>,
    /// Quick resume moon glyph.
    pub sleep_moon: bool,
    /// Minutes of idling before sleep.
    pub sleep_after_min: u16,
    /// Minutes of sleep before deep power-off.
    pub power_off_after_min: u16,
    /// Lock keys when sleeping.
    pub lock_when_sleeping: bool,
    /// Power short press refreshes instead of sleeping.
    pub power_refreshes: bool,
    /// Night jobs hour (0–23) or off.
    pub night_jobs_hour: Option<u8>,
    /// Panel power-down when idle.
    pub panel_off: bool,
    /// Side keys behaviour.
    pub side_keys: SideKeys,
    /// Swap Up/Down.
    pub swap_side_keys: bool,
    /// Follow orientation with the IMU.
    pub orientation_follow: bool,
    /// Left-handed rotation.
    pub left_handed: bool,
    /// Tilt to turn pages.
    pub tilt_turn: bool,
    /// Shake to refresh.
    pub shake_refresh: bool,
    /// Tap to turn.
    pub tap_turn: bool,
    /// Full refresh every N pages.
    pub gc_every_pages: u8,
    /// Show the Spine.
    pub spine: bool,
    /// Library shows a grid (else list).
    pub library_grid: bool,
    /// Library sort tab index.
    pub library_tab: u8,
    /// Dictionary in use (file stem) if any.
    pub dictionary: Option<String>,
    /// Wi-Fi on at boot.
    pub wifi_at_boot: bool,
    /// Sync server URL (KOReader style).
    pub sync_url: String,
    /// Sync user.
    pub sync_user: String,
    /// Sync key.
    pub sync_key: String,
    /// OPDS catalogs: (name, url).
    pub opds: Vec<(String, String)>,
    /// Calibre wireless port.
    pub calibre_port: u16,
    /// Bookshop language filter.
    pub bookshop_language: String,
    /// Weather place name.
    pub weather_place: String,
    /// Weather latitude × 10 000.
    pub weather_lat: i32,
    /// Weather longitude × 10 000.
    pub weather_lon: i32,
    /// Daily goal is pages (else minutes).
    pub goal_pages: bool,
    /// News feed URLs.
    pub news_feeds: Vec<String>,
    /// Device name for mDNS.
    pub hostname: String,
    /// 24-hour clock.
    pub clock_24h: bool,
    /// Time zone offset minutes (informational; the RTC keeps local time).
    pub tz_minutes: i16,
    /// Show the running head on the reading page.
    pub running_head: bool,
    /// Publisher styles honoured.
    pub publisher_styles: bool,
    /// Publisher fonts honoured.
    pub publisher_fonts: bool,
    /// Saved words (dictionary).
    pub saved_words: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: 1,
            first_run_done: false,
            language: "en".into(),
            profile: Profile::default(),
            antialias: false,
            inverted: false,
            large_ui: false,
            simple_mode: false,
            sleep: SleepVariant::Cover,
            sleep_band: true,
            sleep_streak: false,
            sleep_quotes_card: false,
            sleep_rotation: ImageRotation::EachSleep,
            sleep_folder: "/sleep".into(),
            sleep_image: None,
            sleep_moon: true,
            sleep_after_min: 10,
            power_off_after_min: 72 * 60,
            lock_when_sleeping: false,
            power_refreshes: false,
            night_jobs_hour: None,
            panel_off: true,
            side_keys: SideKeys::Pages,
            swap_side_keys: false,
            orientation_follow: false,
            left_handed: false,
            tilt_turn: false,
            shake_refresh: false,
            tap_turn: false,
            gc_every_pages: 10,
            spine: true,
            library_grid: true,
            library_tab: 0,
            dictionary: None,
            wifi_at_boot: false,
            sync_url: String::new(),
            sync_user: String::new(),
            sync_key: String::new(),
            opds: Vec::new(),
            calibre_port: 9090,
            bookshop_language: "en".into(),
            weather_place: String::new(),
            weather_lat: 0,
            weather_lon: 0,
            goal_pages: false,
            news_feeds: Vec::new(),
            hostname: "quire".into(),
            clock_24h: true,
            tz_minutes: 0,
            running_head: true,
            publisher_styles: true,
            publisher_fonts: false,
            saved_words: Vec::new(),
        }
    }
}

impl Settings {
    /// Load, or defaults.
    pub fn load<F: Fs>(fs: &F) -> Settings {
        let mut s: Settings = fs.read_to_vec(SETTINGS_FILE).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
        if s.version != 1 {
            s = Settings::default();
        }
        s
    }
    /// Save if changed on disk (compares bytes to avoid card writes).
    pub fn save<F: Fs>(&self, fs: &F) -> Result<(), quire_fs::FsError> {
        let Ok(bytes) = postcard::to_allocvec(self) else { return Ok(()) };
        if let Ok(old) = fs.read_to_vec(SETTINGS_FILE) {
            if old == bytes {
                return Ok(());
            }
        }
        if !fs.exists(crate::ROOT) {
            fs.mkdir_all(crate::ROOT)?;
        }
        fs.write_atomic(SETTINGS_FILE, &bytes)
    }
    /// Row height for lists.
    pub fn row_h(&self) -> i32 {
        if self.large_ui {
            crate::theme::ROW_H_LARGE
        } else {
            crate::theme::ROW_H
        }
    }
}
