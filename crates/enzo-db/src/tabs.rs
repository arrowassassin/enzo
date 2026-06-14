//! Query-tab manager — multiple named SQL editor tabs with per-tab history.
//!
//! Mirrors Harlequin's multi-query workflow (design doc §5.4): each connection
//! can have several query tabs, each tab remembers its SQL text and a bounded
//! history of executed statements. The renderer drives this via `db.tabs.*`.

use serde::{Deserialize, Serialize};

/// Maximum executed statements retained per tab.
const HISTORY_CAP: usize = 100;

/// One query editor tab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryTab {
    /// Stable tab id.
    pub id: String,
    /// User-facing tab title.
    pub title: String,
    /// Current SQL editor contents.
    pub sql: String,
    /// History of previously-executed statements, newest last.
    pub history: Vec<String>,
}

impl QueryTab {
    /// Create an empty tab.
    fn new(id: String, title: String) -> Self {
        Self {
            id,
            title,
            sql: String::new(),
            history: Vec::new(),
        }
    }

    /// Record an executed statement in this tab's history (deduped, bounded).
    fn record(&mut self, sql: &str) {
        let sql = sql.trim();
        if sql.is_empty() {
            return;
        }
        // Avoid consecutive duplicates.
        if self.history.last().map(String::as_str) == Some(sql) {
            return;
        }
        self.history.push(sql.to_owned());
        if self.history.len() > HISTORY_CAP {
            let overflow = self.history.len() - HISTORY_CAP;
            self.history.drain(0..overflow);
        }
    }
}

/// A set of query tabs for one connection, with an active selection.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TabManager {
    tabs: Vec<QueryTab>,
    active: usize,
    next_seq: u64,
}

impl TabManager {
    /// Create an empty tab manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new tab with an auto-generated title (`Query 1`, `Query 2`, …).
    ///
    /// Returns the new tab's id and makes it active.
    pub fn open(&mut self) -> String {
        self.next_seq += 1;
        let id = format!("tab-{}", self.next_seq);
        let title = format!("Query {}", self.next_seq);
        self.tabs.push(QueryTab::new(id.clone(), title));
        self.active = self.tabs.len() - 1;
        id
    }

    /// Close the tab with `id`. Returns `true` if it existed.
    pub fn close(&mut self, id: &str) -> bool {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        self.tabs.remove(pos);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        true
    }

    /// Rename the tab with `id`. Returns `true` if it existed.
    pub fn rename(&mut self, id: &str, title: &str) -> bool {
        match self.tabs.iter_mut().find(|t| t.id == id) {
            Some(tab) => {
                title.clone_into(&mut tab.title);
                true
            }
            None => false,
        }
    }

    /// Replace the SQL text of the tab with `id`. Returns `true` if it existed.
    pub fn set_sql(&mut self, id: &str, sql: &str) -> bool {
        match self.tabs.iter_mut().find(|t| t.id == id) {
            Some(tab) => {
                sql.clone_into(&mut tab.sql);
                true
            }
            None => false,
        }
    }

    /// Record `sql` in the history of tab `id`. Returns `true` if it existed.
    pub fn record_execution(&mut self, id: &str, sql: &str) -> bool {
        match self.tabs.iter_mut().find(|t| t.id == id) {
            Some(tab) => {
                tab.record(sql);
                true
            }
            None => false,
        }
    }

    /// Make the tab with `id` active. Returns `true` if it existed.
    pub fn set_active(&mut self, id: &str) -> bool {
        match self.tabs.iter().position(|t| t.id == id) {
            Some(pos) => {
                self.active = pos;
                true
            }
            None => false,
        }
    }

    /// Get the tab with `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&QueryTab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// The currently active tab, if any.
    #[must_use]
    pub fn active(&self) -> Option<&QueryTab> {
        self.tabs.get(self.active)
    }

    /// All tabs in order.
    #[must_use]
    pub fn tabs(&self) -> &[QueryTab] {
        &self.tabs
    }

    /// Number of open tabs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// `true` if there are no tabs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// JSON summary for `db.tabs.list`.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let items: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "active": i == self.active,
                    "history_len": t.history.len(),
                })
            })
            .collect();
        serde_json::json!({ "tabs": items, "active_index": self.active })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_active_tab() {
        let mut m = TabManager::new();
        let id = m.open();
        assert_eq!(m.len(), 1);
        assert_eq!(m.active().unwrap().id, id);
        assert_eq!(m.active().unwrap().title, "Query 1");
    }

    #[test]
    fn open_multiple_increments_titles() {
        let mut m = TabManager::new();
        m.open();
        let id2 = m.open();
        assert_eq!(m.get(&id2).unwrap().title, "Query 2");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn rename_changes_title() {
        let mut m = TabManager::new();
        let id = m.open();
        assert!(m.rename(&id, "Reports"));
        assert_eq!(m.get(&id).unwrap().title, "Reports");
        assert!(!m.rename("nope", "x"));
    }

    #[test]
    fn set_sql_updates_contents() {
        let mut m = TabManager::new();
        let id = m.open();
        assert!(m.set_sql(&id, "SELECT 1"));
        assert_eq!(m.get(&id).unwrap().sql, "SELECT 1");
    }

    #[test]
    fn close_adjusts_active() {
        let mut m = TabManager::new();
        let id1 = m.open();
        let _id2 = m.open();
        m.set_active(&id1);
        assert!(m.close(&id1));
        // active was 0, removed → clamps to remaining tab
        assert_eq!(m.len(), 1);
        assert!(m.active().is_some());
    }

    #[test]
    fn history_records_and_dedupes() {
        let mut m = TabManager::new();
        let id = m.open();
        m.record_execution(&id, "SELECT 1");
        m.record_execution(&id, "SELECT 1"); // consecutive dup ignored
        m.record_execution(&id, "SELECT 2");
        let tab = m.get(&id).unwrap();
        assert_eq!(tab.history, vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn history_is_bounded() {
        let mut m = TabManager::new();
        let id = m.open();
        for i in 0..(HISTORY_CAP + 50) {
            m.record_execution(&id, &format!("SELECT {i}"));
        }
        assert_eq!(m.get(&id).unwrap().history.len(), HISTORY_CAP);
        // Oldest entries dropped.
        assert_eq!(m.get(&id).unwrap().history[0], "SELECT 50");
    }

    #[test]
    fn empty_sql_not_recorded() {
        let mut m = TabManager::new();
        let id = m.open();
        m.record_execution(&id, "   ");
        assert!(m.get(&id).unwrap().history.is_empty());
    }

    #[test]
    fn to_json_marks_active() {
        let mut m = TabManager::new();
        m.open();
        let id2 = m.open();
        let j = m.to_json();
        assert_eq!(j["active_index"], 1);
        let tabs = j["tabs"].as_array().unwrap();
        assert_eq!(tabs[1]["id"], id2);
        assert_eq!(tabs[1]["active"], true);
    }
}
