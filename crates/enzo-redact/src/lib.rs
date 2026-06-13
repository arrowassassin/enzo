//! Secret redaction for text before it reaches logs, scrollback, or agent context.
//!
//! The redactor is deliberately conservative and dependency-free. It handles the
//! leak shapes Enzo can easily create itself: connection strings, shell/env style
//! key-value pairs, and HTTP authorization headers.

use std::borrow::Cow;

/// Text used in place of redacted secret values.
pub const REDACTED: &str = "****";

const SECRET_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "authorization",
    "client_secret",
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
];

/// Redact likely secret values from `input`.
///
/// Returns a borrowed [`Cow`] when no secret-shaped text is present.
#[must_use]
pub fn redact_secrets(input: &str) -> Cow<'_, str> {
    let mut ranges = Vec::new();
    collect_key_value_ranges(input, &mut ranges);
    collect_basic_auth_url_ranges(input, &mut ranges);

    if ranges.is_empty() {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(apply_ranges(input, &mut ranges))
    }
}

fn collect_key_value_ranges(input: &str, ranges: &mut Vec<(usize, usize)>) {
    let bytes = input.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        if !is_key_start(bytes[cursor]) {
            cursor += 1;
            continue;
        }

        let key_start = cursor;
        cursor += 1;
        while cursor < bytes.len() && is_key_continue(bytes[cursor]) {
            cursor += 1;
        }

        let key = &input[key_start..cursor];
        if !is_secret_key(key) {
            continue;
        }

        let after_spaces = skip_spaces(bytes, cursor);
        if after_spaces >= bytes.len() {
            continue;
        }

        let separator = bytes[after_spaces];
        let value_start = match separator {
            b'=' => skip_spaces(bytes, after_spaces + 1),
            b':' if is_header_key(input, key_start) => skip_spaces(bytes, after_spaces + 1),
            _ => continue,
        };

        if value_start < bytes.len() {
            let value_range = if key.eq_ignore_ascii_case("authorization") && separator == b':' {
                Some((value_start, line_value_end(bytes, value_start)))
            } else if separator == b'=' && looks_like_assignment(input, value_start) {
                None
            } else {
                Some(value_range(input, value_start))
            };

            if let Some((value_start, value_end)) = value_range
                && value_end > value_start
            {
                ranges.push((value_start, value_end));
            }
        }
    }
}

fn collect_basic_auth_url_ranges(input: &str, ranges: &mut Vec<(usize, usize)>) {
    let mut search_start = 0;

    while let Some(relative_scheme_end) = input[search_start..].find("://") {
        let scheme_end = search_start + relative_scheme_end;
        let authority_start = scheme_end + 3;
        let authority_end = input[authority_start..]
            .find(is_authority_delimiter)
            .map_or(input.len(), |offset| authority_start + offset);
        let authority = &input[authority_start..authority_end];

        if let Some(at) = authority.rfind('@') {
            let userinfo = &authority[..at];
            if let Some(colon) = userinfo.find(':') {
                let password_start = authority_start + colon + 1;
                let password_end = authority_start + at;
                if password_end > password_start {
                    ranges.push((password_start, password_end));
                }
            }
        }

        search_start = authority_end;
    }
}

fn apply_ranges(input: &str, ranges: &mut [(usize, usize)]) -> String {
    ranges.sort_unstable_by_key(|(start, _)| *start);

    let mut output = String::with_capacity(input.len());
    let mut copied_until = 0;
    let mut redacted_until = 0;

    for &(start, end) in ranges.iter() {
        if end <= start || start < redacted_until {
            continue;
        }
        output.push_str(&input[copied_until..start]);
        output.push_str(REDACTED);
        copied_until = end;
        redacted_until = end;
    }

    output.push_str(&input[copied_until..]);
    output
}

fn value_range(input: &str, start: usize) -> (usize, usize) {
    let bytes = input.as_bytes();
    match bytes[start] {
        b'"' | b'\'' => (start + 1, quoted_value_end(bytes, start)),
        _ => (start, unquoted_value_end(bytes, start)),
    }
}

fn quoted_value_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut cursor = start + 1;

    while cursor < bytes.len() {
        if bytes[cursor] == quote {
            return cursor;
        }
        cursor += 1;
    }

    bytes.len()
}

fn unquoted_value_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start;

    while cursor < bytes.len() && !is_unquoted_value_delimiter(bytes[cursor]) {
        cursor += 1;
    }

    cursor
}

fn line_value_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start;

    while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
        cursor += 1;
    }

    cursor
}

fn skip_spaces(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
        cursor += 1;
    }
    cursor
}

fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS
        .iter()
        .any(|candidate| key.eq_ignore_ascii_case(candidate))
}

fn is_header_key(input: &str, key_start: usize) -> bool {
    input[..key_start]
        .rsplit_once('\n')
        .map_or(key_start == 0, |(_, line_prefix)| line_prefix.is_empty())
}

fn looks_like_assignment(input: &str, start: usize) -> bool {
    let bytes = input.as_bytes();
    if start == 0 || !bytes[start - 1].is_ascii_whitespace() || !is_key_start(bytes[start]) {
        return false;
    }

    let mut cursor = start + 1;
    while cursor < bytes.len() && is_key_continue(bytes[cursor]) {
        cursor += 1;
    }

    let after_spaces = skip_spaces(bytes, cursor);
    after_spaces < bytes.len() && bytes[after_spaces] == b'='
}

fn is_key_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn is_key_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn is_unquoted_value_delimiter(byte: u8) -> bool {
    matches!(byte, b'\0'..=b' ' | b',' | b';')
}

fn is_authority_delimiter(ch: char) -> bool {
    matches!(ch, '/' | '?' | '#' | '\\' | ' ' | '\t' | '\r' | '\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_is_borrowed() {
        let input = "select * from users";
        assert!(matches!(redact_secrets(input), Cow::Borrowed(_)));
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn redacts_common_key_value_shapes() {
        let input = "password=hunter2 token = abc; api_key='quoted value' pwd=\"space value\"";
        assert_eq!(
            redact_secrets(input),
            "password=**** token = ****; api_key='****' pwd=\"****\""
        );
    }

    #[test]
    fn redacts_case_insensitive_keys() {
        assert_eq!(
            redact_secrets("PASSWORD=one Client_Secret=two PASSWD=three"),
            "PASSWORD=**** Client_Secret=**** PASSWD=****"
        );
    }

    #[test]
    fn redacts_authorization_header_values() {
        assert_eq!(
            redact_secrets("Authorization: Bearer abc.def\nnext: line"),
            "Authorization: ****\nnext: line"
        );
    }

    #[test]
    fn colon_only_redacts_header_style_keys_at_line_start() {
        assert_eq!(
            redact_secrets("note authorization: public"),
            "note authorization: public"
        );
        assert_eq!(
            redact_secrets("token: abc\n  secret: no"),
            "token: ****\n  secret: no"
        );
    }

    #[test]
    fn redacts_basic_auth_urls() {
        assert_eq!(
            redact_secrets("postgres://enzo:hunter2@localhost/db?sslmode=require"),
            "postgres://enzo:****@localhost/db?sslmode=require"
        );
    }

    #[test]
    fn leaves_urls_without_passwords_alone() {
        let input = "https://github.com/arrowassassin/enzo and ssh://git@github.com/repo";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn redacts_multiple_urls_and_values() {
        assert_eq!(
            redact_secrets("a://u:p@h token=x b://u:second@h/path"),
            "a://u:****@h token=**** b://u:****@h/path"
        );
    }

    #[test]
    fn redacts_unclosed_quoted_values_to_end() {
        assert_eq!(redact_secrets("secret='abc def"), "secret='****");
    }

    #[test]
    fn skips_empty_values() {
        assert_eq!(redact_secrets("password"), "password");
        assert_eq!(redact_secrets("password="), "password=");
        assert_eq!(
            redact_secrets("password token= password= token=abc"),
            "password token= password= token=****"
        );
    }

    #[test]
    fn overlapping_ranges_are_redacted_once() {
        assert_eq!(
            redact_secrets("url=https://u:password=abc@example.com"),
            "url=https://u:****@example.com"
        );
    }
}
