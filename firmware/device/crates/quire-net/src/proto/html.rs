//! HTML to plain text for feed articles and catalog blurbs: tags dropped, block
//! elements become paragraph breaks, entities decoded, whitespace collapsed, and the
//! result cut at a byte budget on a character boundary.

use alloc::string::String;

use super::xmlscan::decode;

const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "tr",
    "table",
    "section",
    "article",
    "header",
    "footer",
    "figure",
    "figcaption",
    "hr",
    "dd",
    "dt",
];
const DROP_TAGS: &[&str] = &["script", "style", "svg", "iframe", "noscript", "template"];

/// Strip markup. `max` bounds the output length in bytes.
pub fn to_text(html: &str, max: usize) -> String {
    let mut out = String::with_capacity(html.len().min(max) + 16);
    let mut rest = html;
    let mut skipping: Option<&str> = None;
    while let Some(i) = rest.find('<') {
        if let Some(name) = skipping {
            // Inside <script>/<style>…: nothing counts until the matching close tag.
            let lower = rest.to_ascii_lowercase();
            let close = alloc::format!("</{name}");
            match lower.find(&close).and_then(|c| rest[c..].find('>').map(|e| c + e + 1)) {
                Some(e) => rest = &rest[e..],
                None => {
                    rest = "";
                    break;
                }
            }
            skipping = None;
            continue;
        }
        push_text(&mut out, &rest[..i]);
        rest = &rest[i..];
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(e) => rest = &rest[e + 3..],
                None => break,
            }
            continue;
        }
        let Some(e) = rest.find('>') else { break };
        let inner = &rest[1..e];
        rest = &rest[e + 1..];
        let closing = inner.starts_with('/');
        let name: String =
            inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        if !closing && !inner.ends_with('/') {
            if let Some(t) = DROP_TAGS.iter().find(|t| **t == name) {
                skipping = Some(t);
                continue;
            }
        }
        if BLOCK_TAGS.contains(&name.as_str()) {
            let sep = if name == "br" || name == "li" { "\n" } else { "\n\n" };
            if !out.is_empty() && !out.ends_with("\n\n") {
                if sep == "\n" && out.ends_with('\n') {
                    // already broken
                } else {
                    out.push_str(sep);
                }
            }
        }
        if out.len() > max + 64 {
            break;
        }
    }
    push_text(&mut out, rest);
    let mut text = decode(&out);
    // Collapse leftover blank runs and trim.
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    let text = text.trim();
    let mut cut = max.min(text.len());
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut result = String::from(&text[..cut]);
    if cut < text.len() {
        result = String::from(result.trim_end());
        result.push('…');
    }
    result
}

/// Append text with runs of whitespace collapsed to one space (newlines kept as the
/// paragraph separators the tags produced).
fn push_text(out: &mut String, s: &str) {
    let mut last_space = out.ends_with([' ', '\n']) || out.is_empty();
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips() {
        let html =
            "<p>Hello <b>bold</b> &amp; <a href=x>link</a>.</p>\n<p>Second\n  para</p><script>x<y</script><ul><li>a</li><li>b</li></ul>";
        assert_eq!(to_text(html, 1000), "Hello bold & link.\n\nSecond para\n\na\nb");
        assert_eq!(to_text("plain &lt;text&gt;", 1000), "plain <text>");
        assert_eq!(to_text("<p>abcdef ghij</p>", 6), "abcdef…");
        assert_eq!(to_text("<p>é😀 x</p>", 2), "é…");
        assert_eq!(to_text("line<br>break<br/>x", 100), "line\nbreak\nx");
    }
}
