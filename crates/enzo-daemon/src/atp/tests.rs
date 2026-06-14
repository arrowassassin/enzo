use super::{base64_decode, call};
use crate::state::DaemonState;
use serde_json::json;

#[test]
fn base64_decode_roundtrip_ascii() {
    // "Man" → "TWFu"
    assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
}

#[test]
fn base64_decode_with_padding_one() {
    // "Ma" → "TWE="
    assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
}

#[test]
fn base64_decode_with_padding_two() {
    // "M" → "TQ=="
    assert_eq!(base64_decode("TQ==").unwrap(), b"M");
}

#[test]
fn base64_decode_empty() {
    assert_eq!(base64_decode("").unwrap(), b"");
}

#[test]
fn base64_decode_invalid_char_errors() {
    assert!(base64_decode("TW!u").is_err());
}

#[test]
fn base64_decode_all_zeros() {
    // Three zero bytes → "AAAA"
    assert_eq!(base64_decode("AAAA").unwrap(), [0u8, 0, 0]);
}

#[test]
fn base64_decode_binary_data() {
    let original: Vec<u8> = (0u8..=255).collect();
    let encoded = {
        use std::fmt::Write as _;
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        let mut i = 0;
        while i < original.len() {
            let remaining = original.len() - i;
            if remaining >= 3 {
                let b0 = original[i] as usize;
                let b1 = original[i + 1] as usize;
                let b2 = original[i + 2] as usize;
                write!(
                    out,
                    "{}{}{}{}",
                    TABLE[b0 >> 2] as char,
                    TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char,
                    TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char,
                    TABLE[b2 & 0x3f] as char,
                )
                .unwrap();
            } else if remaining == 2 {
                let b0 = original[i] as usize;
                let b1 = original[i + 1] as usize;
                write!(
                    out,
                    "{}{}{}=",
                    TABLE[b0 >> 2] as char,
                    TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char,
                    TABLE[(b1 & 0xf) << 2] as char,
                )
                .unwrap();
            } else {
                let b0 = original[i] as usize;
                write!(
                    out,
                    "{}{}==",
                    TABLE[b0 >> 2] as char,
                    TABLE[(b0 & 3) << 4] as char,
                )
                .unwrap();
            }
            i += 3;
        }
        out
    };
    assert_eq!(base64_decode(&encoded).unwrap(), original);
}

#[test]
fn base64_decode_chunk_size_one_is_silently_skipped() {
    // A single-char chunk after stripping padding (length % 4 == 1) hits the
    // `_ => {}` arm. We send "A" as a degenerate input — output is empty.
    assert_eq!(base64_decode("A").unwrap(), b"");
}

// ── ATP dispatch tests via the internal call() helper ────────────────────────

#[tokio::test]
async fn ping_via_internal_call() {
    let state = DaemonState::new();
    let r = call(&state, "ping", json!({})).await;
    assert_eq!(r["result"]["pong"], json!(true));
}

#[tokio::test]
async fn session_spawn_ok_then_close() {
    let state = DaemonState::new();
    let r = call(
        &state,
        "session.spawn",
        json!({ "id": "u1", "cols": 80, "rows": 24 }),
    )
    .await;
    assert!(r["result"].is_object(), "spawn: {r}");
    let r = call(&state, "session.close", json!({ "id": "u1" })).await;
    assert!(r["result"].is_object(), "close: {r}");
}

#[tokio::test]
async fn session_spawn_with_explicit_shell() {
    let state = DaemonState::new();
    let r = call(
        &state,
        "session.spawn",
        json!({ "id": "sh-explicit", "cols": 80, "rows": 24, "shell": "/bin/sh" }),
    )
    .await;
    assert!(r["result"].is_object(), "spawn with shell: {r}");
    let _ = call(&state, "session.close", json!({ "id": "sh-explicit" })).await;
}

// ── Block handler tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn block_push_broadcasts_and_returns_ok() {
    let state = DaemonState::new();
    let mut rx = state.subscribe_notifications().await;

    let r = call(
        &state,
        "block.push",
        json!({ "id": "b1", "type": "text", "title": "hello", "body": "world" }),
    )
    .await;
    assert!(r["result"].is_object(), "block.push: {r}");

    // The notification must be reachable on the broadcast channel.
    let raw = rx.try_recv().expect("notification sent");
    let notif: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(notif["method"], "block.push");
    assert_eq!(notif["params"]["id"], "b1");
}

#[tokio::test]
async fn block_clear_broadcasts_and_returns_ok() {
    let state = DaemonState::new();
    let mut rx = state.subscribe_notifications().await;

    let r = call(&state, "block.clear", json!({ "id": "b1" })).await;
    assert!(r["result"].is_object(), "block.clear: {r}");

    let raw = rx.try_recv().expect("notification sent");
    let notif: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(notif["method"], "block.clear");
}

// ── Prompt handler tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn prompt_respond_resolves_show() {
    let state = DaemonState::new();

    // prompt.show blocks — run it on a separate task.
    let s = state.clone();
    let show_handle = tokio::spawn(async move {
        call(
            &s,
            "prompt.show",
            json!({
                "id": "p1",
                "type": "diff",
                "title": "claude wants to edit renderer.rs",
                "body": "remove redraw_all",
                "actions": ["accept","reject","edit"]
            }),
        )
        .await
    });

    // Give the show task a moment to register the channel and block.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let r = call(
        &state,
        "prompt.respond",
        json!({ "id": "p1", "action": "accept" }),
    )
    .await;
    assert!(r["result"].is_object(), "prompt.respond: {r}");

    let show_result = show_handle.await.unwrap();
    assert_eq!(
        show_result["result"]["action"], "accept",
        "show result: {show_result}"
    );
}

#[tokio::test]
async fn prompt_dismiss_rejects_pending_show() {
    let state = DaemonState::new();

    let s = state.clone();
    let show_handle = tokio::spawn(async move {
        call(
            &s,
            "prompt.show",
            json!({ "id": "p2", "type": "text", "title": "x" }),
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let r = call(&state, "prompt.dismiss", json!({ "id": "p2" })).await;
    assert!(r["result"].is_object(), "prompt.dismiss: {r}");

    let show_result = show_handle.await.unwrap();
    assert_eq!(
        show_result["result"]["action"], "reject",
        "dismissed show should be reject: {show_result}"
    );
}

#[tokio::test]
async fn prompt_respond_to_unknown_id_is_error() {
    let state = DaemonState::new();
    let r = call(
        &state,
        "prompt.respond",
        json!({ "id": "no-such-prompt", "action": "accept" }),
    )
    .await;
    assert!(
        r["error"].is_object(),
        "expected error for unknown prompt: {r}"
    );
}

#[tokio::test]
async fn prompt_show_missing_id_is_error() {
    let state = DaemonState::new();
    let r = call(
        &state,
        "prompt.show",
        json!({ "type": "text", "title": "x" }),
    )
    .await;
    assert!(r["error"].is_object(), "expected error for missing id: {r}");
}
