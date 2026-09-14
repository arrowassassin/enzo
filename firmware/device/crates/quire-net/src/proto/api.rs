//! JSON bodies of the read-only API: status, library and statistics.

use alloc::string::String;
use alloc::vec::Vec;
use quire_library::{Library, Session, Stats, Status};
use quire_ui::WifiState;

use super::json::Json;

/// What the main loop publishes for `/api/status`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusInfo {
    /// Battery percent.
    pub battery_percent: u8,
    /// On USB power.
    pub charging: bool,
    /// Firmware version.
    pub version: String,
    /// Build stamp.
    pub build: String,
    /// The current book's title, if one is open.
    pub book_title: Option<String>,
    /// The current book's progress percent.
    pub book_percent: u8,
    /// mDNS host name.
    pub hostname: String,
    /// Free heap bytes.
    pub heap_free: u32,
    /// Largest free heap block.
    pub heap_largest: u32,
    /// Seconds since boot.
    pub uptime: u32,
    /// Local time, seconds since 1970.
    pub local_now: u32,
    /// Whether the reading page is up (drives the idle timeout).
    pub reading: bool,
}

/// `GET /api/status`.
pub fn status_json(s: &StatusInfo, wifi: &WifiState, card_free: Option<u64>, card_total: u64, pin_set: bool) -> String {
    let mut j = Json::with_capacity(512);
    j.obj();
    j.key("battery").obj().kv_num("percent", s.battery_percent).kv_bool("charging", s.charging).end();
    match card_free {
        Some(f) => j.kv_u64("free", f),
        None => j.key("free").null(),
    };
    j.kv_u64("total", card_total).kv_str("version", &s.version).kv_str("build", &s.build).kv_str("hostname", &s.hostname);
    j.key("wifi").obj();
    match wifi {
        WifiState::Off => {
            j.kv_str("mode", "off");
        }
        WifiState::Connecting(ssid) => {
            j.kv_str("mode", "connecting").kv_str("ssid", ssid);
        }
        WifiState::Connected { ssid, ip, host, signal } => {
            j.kv_str("mode", "station").kv_str("ssid", ssid).kv_str("ip", ip).kv_str("host", host).kv_num("signal", *signal);
        }
        WifiState::Hotspot { ssid, ip, .. } => {
            j.kv_str("mode", "hotspot").kv_str("ssid", ssid).kv_str("ip", ip).kv_str("host", &s.hostname);
        }
        WifiState::Failed(ssid) => {
            j.kv_str("mode", "failed").kv_str("ssid", ssid);
        }
    }
    j.end();
    match &s.book_title {
        Some(t) => {
            j.key("book").obj().kv_str("title", t).kv_num("percent", s.book_percent).end();
        }
        None => {
            j.key("book").null();
        }
    }
    j.kv_bool("pin", pin_set).kv_num("uptime", s.uptime);
    j.key("heap").obj().kv_num("free", s.heap_free).kv_num("largest", s.heap_largest).end();
    j.end();
    j.finish()
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Unread => "unread",
        Status::Reading => "reading",
        Status::Finished => "finished",
        Status::Abandoned => "abandoned",
    }
}

/// `GET /api/library`.
pub fn library_json(lib: &Library) -> String {
    let mut j = Json::with_capacity(128 + lib.books.len() * 160);
    j.obj().key("books").arr();
    for b in lib.books.iter().filter(|b| !b.missing) {
        j.obj();
        j.kv_str("id", &alloc::format!("{}", b.id));
        j.kv_str("path", &b.path).kv_str("title", &b.title).kv_str("author", &b.author_line()).kv_u64("size", b.size);
        j.kv_num("percent", b.percent()).kv_str("status", status_name(b.status));
        j.kv_str("format", &alloc::format!("{:?}", b.format).to_ascii_lowercase());
        j.kv_str(
            "ingest",
            match b.ingest {
                quire_library::IngestState::Pending => "pending",
                quire_library::IngestState::Ready => "ready",
                quire_library::IngestState::Failed => "failed",
            },
        );
        j.kv_opt_str("error", b.error.as_deref()).kv_num("added", b.added).kv_num("last_opened", b.last_opened);
        j.end();
    }
    j.end();
    j.key("sources").arr();
    for s in &lib.sources {
        j.str(s);
    }
    j.end();
    j.key("collections").arr();
    for c in &lib.collections {
        j.obj().kv_num("id", c.id).kv_str("name", &c.name).end();
    }
    j.end();
    j.end();
    j.finish()
}

/// `GET /api/stats`; `sessions` are the most recent ones, newest first, with titles.
pub fn stats_json(stats: &Stats, today: u16, sessions: &[(Session, String)]) -> String {
    let mut j = Json::with_capacity(1024 + stats.days.len() * 48 + sessions.len() * 120);
    let (current, longest) = stats.streaks(today);
    j.obj();
    j.key("totals")
        .obj()
        .kv_num("secs", stats.secs)
        .kv_num("pages", stats.pages)
        .kv_num("sessions", stats.sessions)
        .kv_num("days", stats.days.len() as u32)
        .kv_u64("chars", stats.pace_chars)
        .end();
    j.key("streak").obj().kv_num("current", current).kv_num("longest", longest).end();
    j.key("goal")
        .obj()
        .kv_num("minutes", stats.goal_minutes)
        .kv_num("pages", stats.goal_page_count)
        .kv_bool("use_pages", stats.goal_pages)
        .kv_num("books", stats.goal_books)
        .end();
    j.kv_num("today", today).kv_num("night_sessions", stats.night_sessions);
    j.key("days").arr();
    for d in &stats.days {
        j.obj()
            .kv_num("day", d.day)
            .kv_num("secs", d.secs)
            .kv_num("pages", d.pages)
            .kv_num("chars", d.chars)
            .kv_num("sessions", d.sessions)
            .end();
    }
    j.end();
    j.key("hours").arr();
    for h in stats.hours {
        j.num(h);
    }
    j.end();
    j.key("weekdays").arr();
    for w in stats.weekdays {
        j.num(w);
    }
    j.end();
    j.key("sessions").arr();
    for (s, title) in sessions {
        j.obj();
        j.kv_str("book", &alloc::format!("{}", s.book))
            .kv_str("title", title)
            .kv_num("start", s.start)
            .kv_num("end", s.end)
            .kv_num("active", s.active)
            .kv_num("pages", s.pages)
            .kv_num("chars", s.chars);
        j.end();
    }
    j.end();
    j.end();
    j.finish()
}

/// Titles for sessions, from the library.
pub fn with_titles(lib: &Library, sessions: Vec<Session>) -> Vec<(Session, String)> {
    sessions
        .into_iter()
        .map(|s| {
            let t = lib.get(s.book).map(|b| b.title.clone()).unwrap_or_default();
            (s, t)
        })
        .collect()
}

/// The captive landing page (06 §2): one large link to the real browser.
pub fn captive_html(ip: &str, host: &str) -> String {
    alloc::format!(
        "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Quire</title>\
<style>body{{font-family:-apple-system,Segoe UI,Roboto,sans-serif;background:#f4f1ea;color:#1d1d1b;margin:0;padding:32px 20px;text-align:center}}\
a.b{{display:block;margin:28px auto;max-width:360px;background:#1d1d1b;color:#f4f1ea;text-decoration:none;font-size:22px;font-weight:600;padding:20px;border-radius:14px}}\
p{{color:#6b6860}}code{{font-size:18px}}</style></head><body><h1>Quire</h1><p>You are connected to the reader's hotspot.</p>\
<a class=b href=\"http://{ip}/\" target=\"_blank\" rel=\"noopener\">Open in your browser</a>\
<p>If nothing happens, open your browser and go to<br><code>http://{ip}</code> or <code>http://{host}.local</code></p></body></html>"
    )
}

/// The app icon.
pub const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#1d1d1b"/><rect x="16" y="12" width="32" height="40" rx="3" fill="#f4f1ea"/><path d="M22 22h20M22 29h20M22 36h14" stroke="#1d1d1b" stroke-width="3" stroke-linecap="round"/></svg>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status() {
        let s = StatusInfo {
            battery_percent: 77,
            version: "0.1.0".into(),
            hostname: "quire".into(),
            book_title: Some("Cranford".into()),
            ..Default::default()
        };
        let w = WifiState::Connected { ssid: "HomeNet".into(), ip: "192.168.1.24".into(), host: "quire".into(), signal: 3 };
        let j = status_json(&s, &w, Some(1 << 30), 1 << 31, false);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["battery"]["percent"], 77);
        assert_eq!(v["wifi"]["mode"], "station");
        assert_eq!(v["wifi"]["signal"], 3);
        assert_eq!(v["book"]["title"], "Cranford");
        assert_eq!(v["free"], 1u64 << 30);
        let j = status_json(&s, &WifiState::Off, None, 0, true);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(v["free"].is_null());
        assert_eq!(v["pin"], true);
    }

    #[test]
    fn library_and_stats() {
        let lib = Library::default();
        let v: serde_json::Value = serde_json::from_str(&library_json(&lib)).unwrap();
        assert!(v["books"].as_array().unwrap().is_empty());
        let stats = Stats::default();
        let v: serde_json::Value = serde_json::from_str(&stats_json(&stats, 20000, &[])).unwrap();
        assert_eq!(v["hours"].as_array().unwrap().len(), 24);
        assert_eq!(v["weekdays"].as_array().unwrap().len(), 7);
        assert_eq!(v["totals"]["secs"], 0);
    }

    #[test]
    fn captive() {
        let h = captive_html("192.168.4.1", "quire");
        assert!(h.contains("http://192.168.4.1/"));
        assert!(h.contains("quire.local"));
    }
}
