//! Integration tests: drive the ATP dispatch layer directly, without a real socket.
//!
//! We call `enzo_daemon::atp::call()` which wraps the internal dispatch function,
//! letting us test all method branches (including PTY spawn) in-process.

use serde_json::json;

use enzo_daemon::{atp::call as atp_call, state::DaemonState};

#[tokio::test]
async fn ping_returns_pong() {
    let state = DaemonState::new();
    let r = atp_call(&state, "ping", json!({})).await;
    assert_eq!(r["result"]["pong"], json!(true));
}

#[tokio::test]
async fn unknown_method_returns_method_not_found() {
    let state = DaemonState::new();
    let r = atp_call(&state, "no_such_method", json!({})).await;
    assert_eq!(r["error"]["code"], json!(-32601));
}

#[tokio::test]
async fn session_spawn_missing_id_is_invalid_params() {
    let state = DaemonState::new();
    let r = atp_call(&state, "session.spawn", json!({ "cols": 80, "rows": 24 })).await;
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn session_input_unknown_session() {
    let state = DaemonState::new();
    let r = atp_call(
        &state,
        "session.input",
        json!({ "id": "ghost", "data": "aGVsbG8=" }),
    )
    .await;
    assert_eq!(r["error"]["code"], json!(-32001));
}

#[tokio::test]
async fn session_resize_unknown_session() {
    let state = DaemonState::new();
    let r = atp_call(
        &state,
        "session.resize",
        json!({ "id": "ghost", "cols": 100, "rows": 40 }),
    )
    .await;
    assert_eq!(r["error"]["code"], json!(-32001));
}

#[tokio::test]
async fn session_close_unknown_session() {
    let state = DaemonState::new();
    let r = atp_call(&state, "session.close", json!({ "id": "ghost" })).await;
    assert_eq!(r["error"]["code"], json!(-32001));
}

#[tokio::test]
async fn session_input_missing_id() {
    let state = DaemonState::new();
    let r = atp_call(&state, "session.input", json!({ "data": "YQ==" })).await;
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn session_input_missing_data() {
    let state = DaemonState::new();
    let r = atp_call(&state, "session.input", json!({ "id": "x" })).await;
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn session_input_invalid_base64() {
    let state = DaemonState::new();
    // Spawn a real session first.
    let spawn_r = atp_call(
        &state,
        "session.spawn",
        json!({ "id": "b64-test", "cols": 80, "rows": 24 }),
    )
    .await;
    assert!(spawn_r["result"].is_object(), "spawn failed: {spawn_r}");

    let r = atp_call(
        &state,
        "session.input",
        json!({ "id": "b64-test", "data": "!!!invalid!!!" }),
    )
    .await;
    assert_eq!(r["error"]["code"], json!(-32602));

    let _ = atp_call(&state, "session.close", json!({ "id": "b64-test" })).await;
}

#[tokio::test]
async fn session_resize_missing_id() {
    let state = DaemonState::new();
    let r = atp_call(&state, "session.resize", json!({ "cols": 80, "rows": 24 })).await;
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn session_close_missing_id() {
    let state = DaemonState::new();
    let r = atp_call(&state, "session.close", json!({})).await;
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn full_session_lifecycle() {
    let state = DaemonState::new();

    let r = atp_call(
        &state,
        "session.spawn",
        json!({ "id": "lifecycle", "cols": 80, "rows": 24 }),
    )
    .await;
    assert!(r["result"].is_object(), "spawn: {r}");

    let r = atp_call(
        &state,
        "session.resize",
        json!({ "id": "lifecycle", "cols": 120, "rows": 40 }),
    )
    .await;
    assert!(r["result"].is_object(), "resize: {r}");

    // Send "\n" to stdin (base64 of b"\n").
    let r = atp_call(
        &state,
        "session.input",
        json!({ "id": "lifecycle", "data": "Cg==" }),
    )
    .await;
    assert!(r["result"].is_object(), "input: {r}");

    let r = atp_call(&state, "session.close", json!({ "id": "lifecycle" })).await;
    assert!(r["result"].is_object(), "close: {r}");

    // Session is gone now.
    let r = atp_call(
        &state,
        "session.resize",
        json!({ "id": "lifecycle", "cols": 80, "rows": 24 }),
    )
    .await;
    assert_eq!(r["error"]["code"], json!(-32001), "after-close resize: {r}");
}

// ── Theme family ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn theme_list_includes_builtins() {
    let state = DaemonState::new();
    let r = atp_call(&state, "theme.list", json!({})).await;
    assert_eq!(r["result"]["active"], "enzo-dark");
    let themes = r["result"]["themes"].as_array().unwrap();
    assert!(themes.iter().any(|t| t["id"] == "matrix"));
}

#[tokio::test]
async fn theme_apply_switches_active() {
    let state = DaemonState::new();
    let r = atp_call(&state, "theme.apply", json!({ "id": "matrix" })).await;
    assert_eq!(r["result"]["meta"]["id"], "matrix");
    // The active theme is now matrix.
    let g = atp_call(&state, "theme.get", json!({})).await;
    assert_eq!(g["result"]["meta"]["id"], "matrix");
}

#[tokio::test]
async fn theme_apply_unknown_errors() {
    let state = DaemonState::new();
    let r = atp_call(&state, "theme.apply", json!({ "id": "nope" })).await;
    assert_eq!(r["error"]["code"], json!(-32001));
}

// ── Editor family ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn editor_highlight_rust_keyword() {
    let state = DaemonState::new();
    let r = atp_call(
        &state,
        "editor.highlight",
        json!({ "language": "rust", "source": "fn main() {}" }),
    )
    .await;
    let spans = r["result"]["spans"].as_array().unwrap();
    assert!(spans.iter().any(|s| s["name"] == "keyword"));
}

#[tokio::test]
async fn editor_languages_lists_rust() {
    let state = DaemonState::new();
    let r = atp_call(&state, "editor.languages", json!({})).await;
    let langs = r["result"]["languages"].as_array().unwrap();
    let rust = langs.iter().find(|l| l["id"] == "rust").unwrap();
    assert_eq!(rust["lsp"], "rust-analyzer");
}

// ── Git family ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn git_status_on_non_repo_errors() {
    let state = DaemonState::new();
    let r = atp_call(&state, "git.status", json!({ "path": "/" })).await;
    assert!(r["error"].is_object(), "expected error for non-repo: {r}");
}

#[tokio::test]
async fn git_info_on_self_repo() {
    let state = DaemonState::new();
    // The test runs inside the enzo git repo; CARGO_MANIFEST_DIR is within it.
    let path = env!("CARGO_MANIFEST_DIR");
    let r = atp_call(&state, "git.info", json!({ "path": path })).await;
    // Either we're in a repo (Ok with head) or sandboxed (Err) — accept both,
    // but if Ok the shape must be right.
    if r["result"].is_object() {
        assert!(r["result"]["head"].is_string());
    }
}

// ── DB schema + table + tabs families ─────────────────────────────────────────

async fn connect_seeded_db(state: &DaemonState) {
    atp_call(
        state,
        "db.connect",
        json!({ "id": "c1", "path": ":memory:" }),
    )
    .await;
    atp_call(
        state,
        "db.execute",
        json!({ "conn": "c1", "sql": "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)" }),
    )
    .await;
    atp_call(
        state,
        "db.execute",
        json!({ "conn": "c1", "sql": "INSERT INTO users VALUES (1,'alice'),(2,'bob')" }),
    )
    .await;
}

#[tokio::test]
async fn db_schema_tables_lists_users() {
    let state = DaemonState::new();
    connect_seeded_db(&state).await;
    let r = atp_call(&state, "db.schema.tables", json!({ "conn": "c1" })).await;
    let tables = r["result"]["tables"].as_array().unwrap();
    assert!(tables.iter().any(|t| t["name"] == "users"));
}

#[tokio::test]
async fn db_schema_columns_reports_pk() {
    let state = DaemonState::new();
    connect_seeded_db(&state).await;
    let r = atp_call(
        &state,
        "db.schema.columns",
        json!({ "conn": "c1", "table": "users" }),
    )
    .await;
    let cols = r["result"]["columns"].as_array().unwrap();
    let id = cols.iter().find(|c| c["name"] == "id").unwrap();
    assert_eq!(id["primary_key"], true);
}

#[tokio::test]
async fn db_table_browse_paginates() {
    let state = DaemonState::new();
    connect_seeded_db(&state).await;
    let r = atp_call(
        &state,
        "db.table.browse",
        json!({ "conn": "c1", "table": "users", "page": 0, "size": 1 }),
    )
    .await;
    assert_eq!(r["result"]["total"], 2);
    assert_eq!(r["result"]["rows"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn db_table_update_and_delete_cycle() {
    let state = DaemonState::new();
    connect_seeded_db(&state).await;

    let upd = atp_call(
        &state,
        "db.table.update",
        json!({
            "conn": "c1", "table": "users",
            "cells": [{ "column": "name", "value": "ALICE" }],
            "pk": [{ "column": "id", "value": "1" }]
        }),
    )
    .await;
    assert_eq!(upd["result"]["affected"], 1);

    let del = atp_call(
        &state,
        "db.table.delete",
        json!({ "conn": "c1", "table": "users", "pk": [{ "column": "id", "value": "2" }] }),
    )
    .await;
    assert_eq!(del["result"]["affected"], 1);
}

#[tokio::test]
async fn db_table_update_without_pk_rejected() {
    let state = DaemonState::new();
    connect_seeded_db(&state).await;
    let r = atp_call(
        &state,
        "db.table.update",
        json!({ "conn": "c1", "table": "users", "cells": [{ "column": "name", "value": "x" }], "pk": [] }),
    )
    .await;
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[tokio::test]
async fn db_tabs_open_rename_list() {
    let state = DaemonState::new();
    connect_seeded_db(&state).await;

    let open = atp_call(&state, "db.tabs.open", json!({ "conn": "c1" })).await;
    let tab_id = open["result"]["id"].as_str().unwrap().to_owned();

    let ren = atp_call(
        &state,
        "db.tabs.rename",
        json!({ "conn": "c1", "tab": tab_id, "title": "Reports" }),
    )
    .await;
    assert_eq!(ren["result"]["ok"], true);

    let list = atp_call(&state, "db.tabs.list", json!({ "conn": "c1" })).await;
    let tabs = list["result"]["tabs"].as_array().unwrap();
    assert!(tabs.iter().any(|t| t["title"] == "Reports"));
}

#[tokio::test]
async fn db_connect_execute_query_roundtrip() {
    // The full path the live DB surface drives: connect → create+insert →
    // query, asserting the daemon streams back the real rows it just stored.
    let state = DaemonState::new();
    connect_seeded_db(&state).await;

    let r = atp_call(
        &state,
        "db.query",
        json!({ "conn": "c1", "sql": "SELECT id, name FROM users ORDER BY id" }),
    )
    .await;
    assert_eq!(r["result"]["columns"], json!(["id", "name"]));
    let rows = r["result"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], json!(["1", "alice"]));
    assert_eq!(rows[1], json!(["2", "bob"]));
}

#[tokio::test]
async fn db_query_reports_sql_error() {
    // A bad query must come back as a JSON-RPC error (rendered red in the UI),
    // not a panic or an empty result.
    let state = DaemonState::new();
    connect_seeded_db(&state).await;
    let r = atp_call(
        &state,
        "db.query",
        json!({ "conn": "c1", "sql": "SELECT * FROM does_not_exist" }),
    )
    .await;
    assert!(r["error"].is_object());
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("does_not_exist")
    );
}
