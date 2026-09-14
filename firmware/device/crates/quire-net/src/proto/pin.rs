//! The optional Drop page PIN that gates writes (06 §8).

/// Whether a write may proceed: no PIN configured, or the request carries it in the
/// `X-Pin` header or a `pin` query parameter.
pub fn check(configured: &str, header: Option<&str>, query_pin: Option<&str>) -> bool {
    if configured.is_empty() {
        return true;
    }
    let ok = |s: Option<&str>| s.is_some_and(|s| constant_eq(s.trim().as_bytes(), configured.as_bytes()));
    ok(header) || ok(query_pin)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A PIN is 4–8 digits.
pub fn valid_pin(s: &str) -> bool {
    s.is_empty() || (s.len() >= 4 && s.len() <= 8 && s.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate() {
        assert!(check("", None, None));
        assert!(!check("1234", None, None));
        assert!(check("1234", Some("1234"), None));
        assert!(check("1234", Some(" 1234 "), None));
        assert!(check("1234", None, Some("1234")));
        assert!(!check("1234", Some("123"), Some("12345")));
        assert!(valid_pin("") && valid_pin("1234") && !valid_pin("12") && !valid_pin("12a4"));
    }
}
