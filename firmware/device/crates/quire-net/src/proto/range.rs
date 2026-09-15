//! `Content-Range` for resumable uploads: `bytes start-end/total` (total may be `*`).

/// A parsed `Content-Range` header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentRange {
    /// First byte offset.
    pub start: u64,
    /// Last byte offset, inclusive.
    pub end: u64,
    /// Total size when known.
    pub total: Option<u64>,
}

impl ContentRange {
    /// Bytes in this range.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
    /// True for an empty range (never produced by `parse`).
    pub fn is_empty(&self) -> bool {
        self.end < self.start
    }
    /// Whether this range ends the file.
    pub fn is_last(&self) -> bool {
        self.total.is_some_and(|t| self.end + 1 == t)
    }
}

/// Parse a `Content-Range` value. Returns `None` for `bytes */N` (a size probe) and for
/// anything malformed.
pub fn parse_content_range(v: &str) -> Option<ContentRange> {
    let v = v.trim();
    let rest = v.strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse::<u64>().ok()?),
    };
    let (s, e) = range.trim().split_once('-')?;
    let start: u64 = s.trim().parse().ok()?;
    let end: u64 = e.trim().parse().ok()?;
    if end < start {
        return None;
    }
    if let Some(t) = total {
        if end >= t {
            return None;
        }
    }
    Some(ContentRange { start, end, total })
}

/// What an upload request means for the file on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadPlan {
    /// Create or truncate the file, write `len` bytes; `last` says the file is complete.
    Fresh {
        /// Bytes expected.
        len: u64,
        /// Whether the file is complete after this body.
        last: bool,
    },
    /// Append `len` bytes to the partial file.
    Append {
        /// Bytes expected.
        len: u64,
        /// Whether the file is complete after this body.
        last: bool,
    },
    /// The range does not continue the partial file; the client should resume at `have`.
    Mismatch {
        /// Bytes already on the card.
        have: u64,
    },
}

/// Decide how to store a request body given its `Content-Length`, an optional
/// `Content-Range` and the size of the partial file already on the card.
pub fn plan_upload(content_length: u64, range: Option<ContentRange>, have: Option<u64>) -> Result<UploadPlan, &'static str> {
    match range {
        None => Ok(UploadPlan::Fresh { len: content_length, last: true }),
        Some(r) => {
            if r.len() != content_length {
                return Err("Content-Range does not match Content-Length");
            }
            let last = r.is_last();
            if r.start == 0 {
                Ok(UploadPlan::Fresh { len: content_length, last })
            } else if have == Some(r.start) {
                Ok(UploadPlan::Append { len: content_length, last })
            } else {
                Ok(UploadPlan::Mismatch { have: have.unwrap_or(0) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse_content_range("bytes 0-1048575/3000000"), Some(ContentRange { start: 0, end: 1048575, total: Some(3000000) }));
        assert_eq!(parse_content_range("bytes 5-9/*"), Some(ContentRange { start: 5, end: 9, total: None }));
        assert_eq!(parse_content_range("bytes */100"), None);
        assert_eq!(parse_content_range("bytes 9-5/100"), None);
        assert_eq!(parse_content_range("bytes 0-100/100"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
        assert!(parse_content_range("bytes 0-99/100").unwrap().is_last());
        assert_eq!(parse_content_range("bytes 0-99/100").unwrap().len(), 100);
    }

    #[test]
    fn plans() {
        let r = |s| parse_content_range(s);
        assert_eq!(plan_upload(10, None, Some(3)), Ok(UploadPlan::Fresh { len: 10, last: true }));
        assert_eq!(plan_upload(100, r("bytes 0-99/300"), None), Ok(UploadPlan::Fresh { len: 100, last: false }));
        assert_eq!(plan_upload(100, r("bytes 100-199/300"), Some(100)), Ok(UploadPlan::Append { len: 100, last: false }));
        assert_eq!(plan_upload(100, r("bytes 200-299/300"), Some(200)), Ok(UploadPlan::Append { len: 100, last: true }));
        assert_eq!(plan_upload(100, r("bytes 200-299/300"), Some(100)), Ok(UploadPlan::Mismatch { have: 100 }));
        assert_eq!(plan_upload(100, r("bytes 200-299/300"), None), Ok(UploadPlan::Mismatch { have: 0 }));
        assert!(plan_upload(50, r("bytes 0-99/300"), None).is_err());
    }
}
