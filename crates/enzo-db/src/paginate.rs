//! Pagination helpers for lazily loading large result sets.
//!
//! The data grid (design doc §5.4) streams millions of rows at 120fps by only
//! materialising the visible window. These helpers wrap a base query in
//! `LIMIT`/`OFFSET` and report total counts so the renderer can size the
//! scrollbar without loading everything.

/// A page request: which slice of a result set to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// Zero-based page index.
    pub index: u64,
    /// Rows per page.
    pub size: u64,
}

impl Page {
    /// Create a page request, clamping `size` to at least 1.
    #[must_use]
    pub fn new(index: u64, size: u64) -> Self {
        Self {
            index,
            size: size.max(1),
        }
    }

    /// The row offset this page starts at.
    #[must_use]
    pub fn offset(self) -> u64 {
        self.index.saturating_mul(self.size)
    }
}

/// Wrap a `SELECT` query so it returns only the requested page.
///
/// The base query is treated as a subquery, so it works with arbitrary
/// `SELECT`s (including joins and `ORDER BY`). Trailing semicolons are stripped.
#[must_use]
pub fn paged_query(base_sql: &str, page: Page) -> String {
    let trimmed = base_sql.trim().trim_end_matches(';');
    format!(
        "SELECT * FROM ({trimmed}) AS _enzo_page LIMIT {} OFFSET {}",
        page.size,
        page.offset()
    )
}

/// Wrap a `SELECT` query so it returns the total row count.
///
/// Used to compute the number of pages / scrollbar extent.
#[must_use]
pub fn count_query(base_sql: &str) -> String {
    let trimmed = base_sql.trim().trim_end_matches(';');
    format!("SELECT COUNT(*) AS n FROM ({trimmed}) AS _enzo_count")
}

/// Compute the total number of pages for `total_rows` at `page_size`.
#[must_use]
pub fn page_count(total_rows: u64, page_size: u64) -> u64 {
    if page_size == 0 {
        return 0;
    }
    total_rows.div_ceil(page_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_offset_calculates() {
        assert_eq!(Page::new(0, 100).offset(), 0);
        assert_eq!(Page::new(3, 50).offset(), 150);
    }

    #[test]
    fn page_size_clamped_to_one() {
        assert_eq!(Page::new(0, 0).size, 1);
    }

    #[test]
    fn paged_query_wraps_base() {
        let q = paged_query("SELECT * FROM users", Page::new(2, 25));
        assert!(q.contains("LIMIT 25"));
        assert!(q.contains("OFFSET 50"));
        assert!(q.contains("SELECT * FROM users"));
    }

    #[test]
    fn paged_query_strips_semicolon() {
        let q = paged_query("SELECT 1;", Page::new(0, 10));
        assert!(!q.contains("1;"));
        assert!(q.contains("LIMIT 10"));
    }

    #[test]
    fn count_query_wraps_base() {
        let q = count_query("SELECT * FROM users WHERE active = 1");
        assert!(q.contains("COUNT(*)"));
        assert!(q.contains("WHERE active = 1"));
    }

    #[test]
    fn page_count_rounds_up() {
        assert_eq!(page_count(0, 100), 0);
        assert_eq!(page_count(100, 100), 1);
        assert_eq!(page_count(101, 100), 2);
        assert_eq!(page_count(250, 100), 3);
    }

    #[test]
    fn page_count_zero_size() {
        assert_eq!(page_count(100, 0), 0);
    }
}
