//! Headless UI snapshot + interaction tests for the `enzo-client` egui/eframe app.
//!
//! These boot the real [`EnzoApp`] inside an `egui_kittest` harness (eframe
//! integration, wgpu offscreen renderer on Metal) and:
//!   * render a PNG snapshot of each surface and overlay, and
//!   * drive a few interactions, asserting the result via the app state.
//!
//! Baselines live under `tests/snapshots/`. Regenerate them with:
//! ```text
//! UPDATE_SNAPSHOTS=1 cargo test -p enzo-client --test ui_snapshot
//! ```
//!
//! The dock icons and tab "+" buttons are custom `Frame`-based widgets that do
//! not publish AccessKit labels, so interactions are driven through the
//! `#[doc(hidden)]` test hooks on `EnzoApp` (`__set_surface`, `__db_add_query_tab`,
//! `__ide_activate`, …). Each hook routes through the exact same code path as the
//! real click handler, and every assertion is followed by a fresh snapshot so the
//! rendered result is also captured.

use egui_kittest::Harness;
use enzo_client::gui::{__new_app_for_test_offline, EnzoApp};
use enzo_client::surface::Surface;

/// Window size for the snapshots. Matches the app's default aspect ratio,
/// scaled down so the PNGs stay small while still exercising every panel.
const SIZE: (f32, f32) = (1280.0, 760.0);

/// Build a harness wrapping the real [`EnzoApp`] via the eframe + wgpu backend.
///
/// The offline constructor NEVER spawns the background ATP thread, so the app
/// never touches the socket and always renders in its disconnected state. This
/// makes the snapshots deterministic regardless of whether an `enzo-daemon`
/// happens to be listening on the ATP socket.
fn harness() -> Harness<'static, EnzoApp> {
    Harness::builder()
        .with_size(egui::Vec2::new(SIZE.0, SIZE.1))
        .wgpu()
        .build_eframe(|cc| __new_app_for_test_offline(cc))
}

/// Settle the UI for a snapshot.
///
/// The Terminal surface schedules a 33ms repaint (`request_repaint_after`) to
/// animate the cursor, so [`Harness::run`] never reaches a quiescent state and
/// would panic on `max_steps`. A fixed number of steps is enough for layout and
/// any one-shot animations (overlays, tab strips) to converge, and is the
/// pattern the kittest docs recommend for continuously-repainting UIs.
fn settle(h: &mut Harness<'static, EnzoApp>) {
    h.run_steps(6);
}

#[test]
fn ui_surfaces_and_overlays() {
    let mut h = harness();
    let mut results = egui_kittest::SnapshotResults::new();

    // ── Terminal (default surface) ──────────────────────────────────────────
    assert_eq!(h.state().__surface(), Surface::Terminal);
    settle(&mut h);
    results.add(h.try_snapshot("surface_terminal"));

    // ── Editor / IDE ────────────────────────────────────────────────────────
    h.state_mut().__set_surface(Surface::Ide);
    settle(&mut h);
    assert_eq!(h.state().__surface(), Surface::Ide);
    results.add(h.try_snapshot("surface_editor"));

    // ── Database ────────────────────────────────────────────────────────────
    h.state_mut().__set_surface(Surface::Database);
    settle(&mut h);
    assert_eq!(h.state().__surface(), Surface::Database);
    results.add(h.try_snapshot("surface_database"));

    // ── Browser ─────────────────────────────────────────────────────────────
    h.state_mut().__set_surface(Surface::Browser);
    settle(&mut h);
    assert_eq!(h.state().__surface(), Surface::Browser);
    results.add(h.try_snapshot("surface_browser"));

    // ── Command palette (⌘K) ────────────────────────────────────────────────
    h.state_mut().__set_surface(Surface::Terminal);
    h.state_mut().__set_palette_open(true);
    settle(&mut h);
    results.add(h.try_snapshot("command_palette"));
    h.state_mut().__set_palette_open(false);

    // ── Settings overlay ────────────────────────────────────────────────────
    h.state_mut().__set_settings_open(true);
    settle(&mut h);
    results.add(h.try_snapshot("settings"));
    h.state_mut().__set_settings_open(false);

    results.unwrap();
}

#[test]
fn dock_icon_switches_surface() {
    let mut h = harness();
    settle(&mut h);
    assert_eq!(h.state().__surface(), Surface::Terminal);

    // Clicking the "Database" dock icon switches the surface.
    h.state_mut().__set_surface(Surface::Database);
    settle(&mut h);
    assert_eq!(h.state().__surface(), Surface::Database);

    // Follow-up snapshot confirms the Database surface is rendered.
    h.snapshot("interaction_dock_switch_database");
}

#[test]
fn db_plus_adds_query_tab() {
    let mut h = harness();
    h.state_mut().__set_surface(Surface::Database);
    settle(&mut h);

    let before = h.state().__db_tab_count();
    assert_eq!(before, 1, "DB state starts with one query tab");

    // Clicking the "+" in the DB tab strip opens a new query tab.
    h.state_mut().__db_add_query_tab();
    settle(&mut h);

    let after = h.state().__db_tab_count();
    assert_eq!(after, before + 1, "the '+' should add exactly one tab");

    // Follow-up snapshot shows the second tab chip in the strip.
    h.snapshot("interaction_db_added_tab");
}

#[test]
fn db_connected_results() {
    let mut h = harness();
    h.state_mut().__set_surface(Surface::Database);
    // Inject a real-shaped connection + result set (offline, no daemon).
    h.state_mut().__db_add_connection(
        "SQLite · demo.db",
        "/home/u/.enzo/demo.db",
        &["users", "products"],
    );
    h.state_mut().__db_apply_result(
        &["id", "name", "email"],
        &[
            &["1", "Alice", "alice@example.com"],
            &["2", "Bob", "bob@example.com"],
            &["3", "Carol", "carol@example.com"],
        ],
        7,
    );
    settle(&mut h);
    h.snapshot("db_connected_results");
}

#[test]
fn db_connection_dialog() {
    let mut h = harness();
    h.state_mut().__set_surface(Surface::Database);
    h.state_mut().__db_set_dialog_open(true);
    settle(&mut h);
    h.snapshot("db_connection_dialog");
}

#[test]
fn db_error_state() {
    let mut h = harness();
    h.state_mut().__set_surface(Surface::Database);
    h.state_mut()
        .__db_add_connection("SQLite · demo.db", ":memory:", &["users"]);
    h.state_mut().__db_set_error("no such table: nope");
    settle(&mut h);
    h.snapshot("db_error_state");
}

#[test]
fn ide_folder_expands() {
    let mut h = harness();
    h.state_mut().__set_surface(Surface::Ide);
    settle(&mut h);

    // The explorer is rooted at the crate's working dir, which always contains
    // sub-directories (e.g. `src/`), so there is at least one folder to expand.
    let Some(dir_index) = h.state().__ide_first_dir_index() else {
        // No directory available in this checkout — nothing to assert, but make
        // the situation explicit rather than silently passing.
        panic!("expected at least one directory in the IDE explorer root");
    };

    let before = h.state().__ide_entry_count();
    assert!(!h.state().__ide_is_expanded(dir_index));

    // Expanding the folder splices its children into the visible tree.
    h.state_mut().__ide_activate(dir_index);
    settle(&mut h);

    assert!(
        h.state().__ide_is_expanded(dir_index),
        "folder should be expanded after activation"
    );
    assert!(
        h.state().__ide_entry_count() >= before,
        "expanding a folder should not drop visible rows"
    );

    // Follow-up snapshot shows the expanded tree.
    h.snapshot("interaction_ide_expanded_folder");
}
