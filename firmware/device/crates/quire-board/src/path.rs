//! Path handling for the FAT layer: `/`-separated absolute paths split into components,
//! with the parent/name split the volume manager needs.

/// Iterate the non-empty components of an absolute path.
pub fn components(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|c| !c.is_empty() && *c != ".")
}

/// Split into (parent components, final name). `"/a/b/c.txt"` → (`["a","b"]`, `"c.txt"`).
pub fn split_parent(path: &str) -> Option<(&str, &str)> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind('/') {
        Some(i) => Some((&trimmed[..i], &trimmed[i + 1..])),
        None => Some(("", trimmed)),
    }
}

/// Whether a name is acceptable on FAT (no reserved characters, not empty, ≤ 255).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 255
        && !name.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 0x20)
        && name != "."
        && name != ".."
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    #[test]
    fn splits() {
        assert_eq!(components("/Books/classics/moby.epub").collect::<Vec<_>>(), ["Books", "classics", "moby.epub"]);
        assert_eq!(split_parent("/Books/classics/moby.epub"), Some(("/Books/classics", "moby.epub")));
        assert_eq!(split_parent("/library.bin"), Some(("", "library.bin")));
        assert_eq!(split_parent("/"), None);
        assert!(valid_name("My Book (1).epub"));
        assert!(!valid_name("bad:name"));
        assert!(!valid_name(".."));
    }
}
