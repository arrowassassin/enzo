//! Wikipedia's REST summary endpoint: the request for a term and the (title, extract)
//! from its JSON.

use alloc::string::String;

use super::jsonlite;
use super::url::percent_encode;

/// Longest extract kept (bytes).
pub const EXTRACT_MAX: usize = 4096;

/// The summary URL for `term` in the `lang` Wikipedia (`en` when empty or odd).
pub fn summary_url(lang: &str, term: &str) -> String {
    let lang: String = lang.chars().filter(|c| c.is_ascii_alphabetic()).take(8).collect::<String>().to_ascii_lowercase();
    let lang = if lang.len() < 2 { String::from("en") } else { lang };
    let title: String = term.split_whitespace().collect::<alloc::vec::Vec<&str>>().join("_");
    alloc::format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{}", percent_encode(&title))
}

/// The summary: `Ok((title, extract))`, or a message for a missing page or odd document.
pub fn parse_summary(json: &[u8]) -> Result<(String, String), String> {
    let v = jsonlite::parse(json).ok_or_else(|| String::from("Wikipedia sent something unreadable"))?;
    if let Some(t) = v.get("title").and_then(|t| t.as_str()) {
        if t == "Not found." || v.get("type").and_then(|t| t.as_str()).is_some_and(|t| t.ends_with("not_found")) {
            return Err(String::from("No article with that name."));
        }
    }
    let title = v.get("title").and_then(|t| t.as_str()).ok_or_else(|| String::from("No article with that name."))?;
    let mut extract = v.get("extract").and_then(|e| e.as_str()).unwrap_or_default();
    if v.get("type").and_then(|t| t.as_str()).as_deref() == Some("disambiguation") && extract.is_empty() {
        extract = String::from("This name refers to several articles; try a more specific search.");
    }
    if extract.len() > EXTRACT_MAX {
        let mut cut = EXTRACT_MAX;
        while !extract.is_char_boundary(cut) {
            cut -= 1;
        }
        extract.truncate(cut);
        extract.push('…');
    }
    Ok((title, extract))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url() {
        assert_eq!(summary_url("en", "  Jane   Austen "), "https://en.wikipedia.org/api/rest_v1/page/summary/Jane_Austen");
        assert_eq!(summary_url("", "Café"), "https://en.wikipedia.org/api/rest_v1/page/summary/Caf%C3%A9");
        assert!(summary_url("de-CH", "x").starts_with("https://dech.wikipedia.org/"));
    }

    #[test]
    fn summary() {
        let json = br#"{"type":"standard","title":"Walden","extract":"Walden is a book by Thoreau.","thumbnail":{"source":"x"}}"#;
        assert_eq!(parse_summary(json).unwrap(), (String::from("Walden"), String::from("Walden is a book by Thoreau.")));
        let missing = br#"{"type":"https://mediawiki.org/wiki/HyperSwitch/errors/not_found","title":"Not found.","detail":"Page or revision not found."}"#;
        assert_eq!(parse_summary(missing).unwrap_err(), "No article with that name.");
        assert!(parse_summary(b"<html>").is_err());
    }
}
