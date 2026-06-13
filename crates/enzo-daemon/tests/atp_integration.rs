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
