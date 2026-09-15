//! Open-Meteo, no key: geocoding of the place name from settings, then the forecast
//! (current temperature and code, five days of highs and lows) as a [`WeatherReport`].

use alloc::string::String;
use alloc::vec::Vec;

use quire_ui::net::WeatherReport;

use super::jsonlite;
use super::url::percent_encode;

/// The geocoding request for a place name.
pub fn geocode_url(place: &str) -> String {
    alloc::format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&format=json", percent_encode(place.trim()))
}

/// The forecast request for a location (coordinates × 10 000, as the settings keep them).
pub fn forecast_url(lat: i32, lon: i32) -> String {
    alloc::format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=5",
        fixed4(lat),
        fixed4(lon)
    )
}

fn fixed4(v: i32) -> String {
    alloc::format!("{}{}.{:04}", if v < 0 { "-" } else { "" }, v.unsigned_abs() / 10_000, v.unsigned_abs() % 10_000)
}

/// The first geocoding result: (latitude × 10 000, longitude × 10 000, display name).
pub fn parse_geocode(json: &[u8]) -> Option<(i32, i32, String)> {
    let v = jsonlite::parse(json)?;
    let first = v.get("results")?.items().next()?;
    let lat = first.get("latitude")?.as_f64()?;
    let lon = first.get("longitude")?.as_f64()?;
    let mut name = first.get("name").and_then(|n| n.as_str()).unwrap_or_default();
    if let Some(c) = first.get("country_code").and_then(|c| c.as_str()) {
        if !name.is_empty() {
            name.push_str(", ");
            name.push_str(&c);
        }
    }
    Some(((lat * 10_000.0) as i32, (lon * 10_000.0) as i32, name))
}

/// The WMO weather code in words.
pub fn code_text(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 | 73 => "Snow",
        75 | 77 => "Heavy snow",
        80 | 81 => "Showers",
        82 => "Heavy showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, hail",
        _ => "Unknown",
    }
}

fn weekday_name(date: &str) -> String {
    // "YYYY-MM-DD" → Mon…Sun via days since the epoch (1970-01-01 was a Thursday).
    let y: i64 = date.get(0..4).and_then(|s| s.parse().ok()).unwrap_or(1970);
    let m: i64 = date.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    let d: i64 = date.get(8..10).and_then(|s| s.parse().ok()).unwrap_or(1);
    let yy = if m <= 2 { y - 1 } else { y };
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) - 719468;
    let wd = (days + 4).rem_euclid(7);
    String::from(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][wd as usize])
}

/// The forecast JSON as a report.
pub fn parse_forecast(json: &[u8]) -> Option<WeatherReport> {
    let v = jsonlite::parse(json)?;
    let cur = v.get("current")?;
    let temp = cur.get("temperature_2m")?.as_f64()?;
    let code = cur.get("weather_code").and_then(|c| c.as_i64()).unwrap_or(-1);
    let daily = v.get("daily")?;
    let dates: Vec<String> = daily.get("time")?.items().filter_map(|d| d.as_str()).collect();
    let highs: Vec<f64> = daily.get("temperature_2m_max")?.items().filter_map(|d| d.as_f64()).collect();
    let lows: Vec<f64> = daily.get("temperature_2m_min")?.items().filter_map(|d| d.as_f64()).collect();
    let days = dates.iter().zip(highs.iter().zip(lows.iter())).take(5).map(|(d, (h, l))| (weekday_name(d), round(*h), round(*l))).collect();
    Some((round(temp), String::from(code_text(code)), days))
}

fn round(v: f64) -> i16 {
    (if v >= 0.0 { v + 0.5 } else { v - 0.5 }) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(geocode_url("São Paulo"), "https://geocoding-api.open-meteo.com/v1/search?name=S%C3%A3o%20Paulo&count=1&format=json");
        assert!(forecast_url(485_000, -23_456).starts_with("https://api.open-meteo.com/v1/forecast?latitude=48.5000&longitude=-2.3456&"));
    }

    #[test]
    fn geocode() {
        let json = br#"{"results":[{"name":"Lisbon","latitude":38.71667,"longitude":-9.13333,"country_code":"PT"}]}"#;
        assert_eq!(parse_geocode(json), Some((387_166, -91_333, String::from("Lisbon, PT"))));
        assert_eq!(parse_geocode(br#"{"generationtime_ms":1.2}"#), None);
    }

    #[test]
    fn forecast() {
        let json = br#"{"current":{"temperature_2m":21.4,"weather_code":3},
          "daily":{"time":["2026-09-15","2026-09-16","2026-09-17"],"weather_code":[3,61,0],
          "temperature_2m_max":[24.6,19.2,22.0],"temperature_2m_min":[15.5,12.4,-0.6]}}"#;
        let (t, c, days) = parse_forecast(json).unwrap();
        assert_eq!((t, c.as_str()), (21, "Overcast"));
        assert_eq!(days, [("Tue".into(), 25, 16), ("Wed".into(), 19, 12), ("Thu".into(), 22, -1)]);
        assert_eq!(code_text(95), "Thunderstorm");
    }
}
