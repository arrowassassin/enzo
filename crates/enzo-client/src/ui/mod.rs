//! Application UI state — tab bar and connection status.
//!
//! The renderer uses this to draw the two chrome rows that bracket the
//! terminal area: a tab bar on row 0 and a status bar on the last row.

/// Rows consumed by the tab bar (top) and status bar (bottom).
pub const CHROME_ROWS: u16 = 2;

/// One open terminal tab.
#[derive(Debug, Clone)]
pub struct Tab {
    /// ATP session identifier.
    pub session_id: String,
    /// Short label shown in the tab bar (e.g. `"bash"`).
    pub title: String,
}

/// Application-level UI state shared between the renderer and the event loop.
#[derive(Debug)]
pub struct UiState {
    tabs: Vec<Tab>,
    active: usize,
    /// Whether the ATP daemon connection is live.
    pub connected: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    /// Create an empty `UiState` (no tabs, disconnected).
    #[must_use]
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            connected: false,
        }
    }

    /// Append a new tab and make it active.
    pub fn add_tab(&mut self, session_id: String, title: String) {
        self.tabs.push(Tab { session_id, title });
        self.active = self.tabs.len() - 1;
    }

    /// Remove the active tab and return its session id, or `None` if there are no tabs.
    pub fn close_active(&mut self) -> Option<String> {
        if self.tabs.is_empty() {
            return None;
        }
        let removed = self.tabs.remove(self.active);
        if self.active >= self.tabs.len() && self.active > 0 {
            self.active -= 1;
        }
        Some(removed.session_id)
    }

    /// Switch to the next tab (wraps around).
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab (wraps around).
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = self.active.checked_sub(1).unwrap_or(self.tabs.len() - 1);
        }
    }

    /// Return the session id of the active tab, if any.
    #[must_use]
    pub fn active_session_id(&self) -> Option<&str> {
        self.tabs.get(self.active).map(|t| t.session_id.as_str())
    }

    /// Index of the active tab.
    #[must_use]
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// All tabs.
    #[must_use]
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Number of open tabs.
    #[must_use]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// `true` if there are no open tabs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(n: usize) -> UiState {
        let mut ui = UiState::new();
        for i in 0..n {
            ui.add_tab(format!("s{i}"), format!("bash{i}"));
        }
        ui
    }

    #[test]
    fn add_tab_sets_active() {
        let mut ui = UiState::new();
        ui.add_tab("s0".into(), "bash".into());
        assert_eq!(ui.active_index(), 0);
        ui.add_tab("s1".into(), "bash".into());
        assert_eq!(ui.active_index(), 1);
    }

    #[test]
    fn close_active_removes_last_adjusts_index() {
        let mut ui = make(3);
        ui.active = 2;
        let sid = ui.close_active().unwrap();
        assert_eq!(sid, "s2");
        assert_eq!(ui.tab_count(), 2);
        assert_eq!(ui.active_index(), 1);
    }

    #[test]
    fn close_only_tab_returns_session_id() {
        let mut ui = make(1);
        let sid = ui.close_active().unwrap();
        assert_eq!(sid, "s0");
        assert!(ui.is_empty());
    }

    #[test]
    fn close_empty_returns_none() {
        let mut ui = UiState::new();
        assert!(ui.close_active().is_none());
    }

    #[test]
    fn next_prev_wrap() {
        let mut ui = make(3);
        ui.active = 0;
        ui.prev_tab();
        assert_eq!(ui.active_index(), 2);
        ui.next_tab();
        assert_eq!(ui.active_index(), 0);
    }

    #[test]
    fn active_session_id_correct() {
        let mut ui = make(2);
        ui.active = 1;
        assert_eq!(ui.active_session_id(), Some("s1"));
    }

    #[test]
    fn active_session_id_empty() {
        let ui = UiState::new();
        assert!(ui.active_session_id().is_none());
    }
}
