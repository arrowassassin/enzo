//! JSON view of [`quire_ui::Settings`] for `GET`/`PUT /api/settings`: a `PUT` carries only
//! the keys that change, merged onto the current settings.

use alloc::string::String;
use quire_ui::Settings;
use serde_json::Value;

/// The settings as JSON, plus `pin_set`.
pub fn to_json(s: &Settings, pin_set: bool) -> String {
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Value::Object(o) = &mut v {
        o.insert(String::from("pin_set"), Value::Bool(pin_set));
    }
    serde_json::to_string(&v).unwrap_or_default()
}

/// The result of merging a request body.
pub struct Merged {
    /// The new settings.
    pub settings: Settings,
    /// A new PIN when the body carried `pin` (empty clears it).
    pub pin: Option<String>,
}

/// Merge `body` (a JSON object of changed keys) onto `current`. Unknown keys are ignored;
/// a value of the wrong type is an error.
pub fn merge(current: &Settings, body: &str) -> Result<Merged, String> {
    let patch: Value = serde_json::from_str(body).map_err(|e| alloc::format!("bad JSON: {e}"))?;
    let Value::Object(patch) = patch else { return Err(String::from("expected an object")) };
    let mut base = serde_json::to_value(current).map_err(|e| alloc::format!("{e}"))?;
    let Value::Object(map) = &mut base else { return Err(String::from("settings are not an object")) };
    let mut pin = None;
    for (k, v) in patch {
        if k == "pin" {
            let p = v.as_str().ok_or_else(|| String::from("pin must be a string"))?.trim();
            if !super::pin::valid_pin(p) {
                return Err(String::from("pin must be 4 to 8 digits"));
            }
            pin = Some(String::from(p));
            continue;
        }
        if k == "version" || k == "pin_set" {
            continue;
        }
        if map.contains_key(&k) {
            map.insert(k, v);
        }
    }
    let settings: Settings = serde_json::from_value(base).map_err(|e| alloc::format!("bad value: {e}"))?;
    Ok(Merged { settings, pin })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings::default();
        let j = to_json(&s, true);
        assert!(j.contains("\"pin_set\":true"));
        assert!(j.contains("\"hostname\":\"quire\""));
        let m = merge(&s, r#"{"hostname":"reader","sleep_after_min":5,"unknown":1,"pin":"2468"}"#).unwrap();
        assert_eq!(m.settings.hostname, "reader");
        assert_eq!(m.settings.sleep_after_min, 5);
        assert_eq!(m.pin.as_deref(), Some("2468"));
        assert_eq!(m.settings.version, 1);
        assert!(merge(&s, r#"{"sleep_after_min":"soon"}"#).is_err());
        assert!(merge(&s, r#"[1]"#).is_err());
        assert!(merge(&s, r#"{"pin":"12"}"#).is_err());
        let m = merge(&s, r#"{"pin":""}"#).unwrap();
        assert_eq!(m.pin.as_deref(), Some(""));
    }
}
