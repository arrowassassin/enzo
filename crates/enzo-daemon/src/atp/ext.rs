//! Extended ATP handlers: `theme.*`, `git.*`, `editor.*`, and the
//! `db.tabs.* / db.schema.* / db.table.*` families.
//!
//! These live in a child module so they can reuse the parent's private
//! [`Response`](super::Response) type while keeping `mod.rs` focused on the
//! transport and core session/db/lsp/browser handlers.

use serde_json::{Value, json};

use enzo_db::Cell;
use enzo_db::paginate::Page;
use enzo_editor::Language;

use super::Response;
use crate::state::DaemonState;

// ── Theme handlers ─────────────────────────────────────────────────────────

/// `theme.list` → `{ themes: [...], active }`.
pub(super) async fn theme_list(id: Value, state: &DaemonState) -> Response {
    Response::ok(id, state.theme_list().await)
}

/// `theme.get` `{ id? }` → resolved theme JSON (active theme if `id` omitted).
pub(super) async fn theme_get(id: Value, p: &Value, state: &DaemonState) -> Response {
    match p["id"].as_str() {
        None => Response::ok(id, state.theme_active().await),
        Some(theme_id) => match state.theme_get(theme_id).await {
            Some(theme) => Response::ok(id, theme),
            None => Response::err(id, -32001, format!("unknown theme '{theme_id}'")),
        },
    }
}

/// `theme.apply` `{ id }` → resolved theme JSON for the now-active theme.
pub(super) async fn theme_apply(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(theme_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    match state.theme_apply(theme_id).await {
        Ok(theme) => Response::ok(id, theme),
        Err(e) => Response::err(id, -32001, e.to_string()),
    }
}

// ── Editor handlers ────────────────────────────────────────────────────────

/// Map an ATP language id string to a [`Language`].
fn language_from_id(s: &str) -> Language {
    match s {
        "rust" => Language::Rust,
        "python" => Language::Python,
        "javascript" | "typescript" => Language::JavaScript,
        "json" => Language::Json,
        _ => Language::PlainText,
    }
}

/// `editor.highlight` `{ language, source }` → `{ spans: [{start,end,name}] }`.
#[allow(
    clippy::unused_async,
    reason = "async keeps the dispatch table uniform"
)]
pub(super) async fn editor_highlight(id: Value, p: &Value) -> Response {
    let lang = language_from_id(p["language"].as_str().unwrap_or("plaintext"));
    let source = p["source"].as_str().unwrap_or("");
    match enzo_editor::highlight(lang, source) {
        Ok(spans) => {
            let arr: Vec<Value> = spans
                .into_iter()
                .map(|s| json!({ "start": s.start, "end": s.end, "name": s.name }))
                .collect();
            Response::ok(id, json!({ "spans": arr }))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

/// `editor.format` `{ language, source }` → `{ formatted }`.
///
/// Runs the external formatter in a blocking task (it spawns a subprocess).
pub(super) async fn editor_format(id: Value, p: &Value) -> Response {
    let lang = language_from_id(p["language"].as_str().unwrap_or("plaintext"));
    let source = p["source"].as_str().unwrap_or("").to_owned();
    let result =
        tokio::task::spawn_blocking(move || enzo_editor::format::format_source(lang, &source))
            .await;
    match result {
        Ok(Ok(formatted)) => Response::ok(id, json!({ "formatted": formatted })),
        Ok(Err(e)) => Response::err(id, -32000, e.to_string()),
        Err(e) => Response::err(id, -32000, format!("format task panicked: {e}")),
    }
}

/// `editor.languages` → list of supported languages with their services.
#[allow(
    clippy::unused_async,
    reason = "async keeps the dispatch table uniform"
)]
pub(super) async fn editor_languages(id: Value) -> Response {
    let langs = [
        Language::Rust,
        Language::Python,
        Language::JavaScript,
        Language::Json,
        Language::PlainText,
    ];
    let arr: Vec<Value> = langs
        .iter()
        .map(|l| {
            json!({
                "id": l.id(),
                "name": l.display_name(),
                "lsp": l.lsp_server().map(|s| s.command),
                "formatter": l.formatter().map(|f| f.command),
            })
        })
        .collect();
    Response::ok(id, json!({ "languages": arr }))
}

// ── Git handlers ───────────────────────────────────────────────────────────

/// Run a synchronous git operation on `path` in a blocking task.
///
/// `op` receives an opened [`enzo_git::Repo`] and returns serializable JSON.
async fn git_op<F>(id: Value, p: &Value, op: F) -> Response
where
    F: FnOnce(&enzo_git::Repo) -> anyhow::Result<Value> + Send + 'static,
{
    let path = p["path"].as_str().unwrap_or(".").to_owned();
    let result = tokio::task::spawn_blocking(move || {
        let repo = enzo_git::Repo::open(&path)?;
        op(&repo)
    })
    .await;
    match result {
        Ok(Ok(v)) => Response::ok(id, v),
        Ok(Err(e)) => Response::err(id, -32000, e.to_string()),
        Err(e) => Response::err(id, -32000, format!("git task panicked: {e}")),
    }
}

/// `git.status` `{ path }` → `{ entries: [...] }`.
pub(super) async fn git_status(id: Value, p: &Value) -> Response {
    git_op(id, p, |r| Ok(json!({ "entries": r.status()? }))).await
}

/// `git.info` `{ path }` → repo summary.
pub(super) async fn git_info(id: Value, p: &Value) -> Response {
    git_op(id, p, |r| Ok(serde_json::to_value(r.info()?)?)).await
}

/// `git.diff` `{ path, staged? }` → `{ files: [...] }`.
pub(super) async fn git_diff(id: Value, p: &Value) -> Response {
    let staged = p["staged"].as_bool().unwrap_or(false);
    git_op(id, p, move |r| {
        let files = if staged {
            r.diff_staged()?
        } else {
            r.diff_unstaged()?
        };
        Ok(json!({ "files": files }))
    })
    .await
}

/// `git.stage` `{ path, file }` (or `all: true`) → `{}`.
pub(super) async fn git_stage(id: Value, p: &Value) -> Response {
    let all = p["all"].as_bool().unwrap_or(false);
    let file = p["file"].as_str().map(str::to_owned);
    git_op(id, p, move |r| {
        if all {
            r.stage_all()?;
        } else if let Some(f) = file {
            r.stage(&f)?;
        } else {
            anyhow::bail!("missing file (or all: true)");
        }
        Ok(json!({}))
    })
    .await
}

/// `git.unstage` `{ path, file }` → `{}`.
pub(super) async fn git_unstage(id: Value, p: &Value) -> Response {
    let Some(file) = p["file"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing file");
    };
    git_op(id, p, move |r| {
        r.unstage(&file)?;
        Ok(json!({}))
    })
    .await
}

/// `git.commit` `{ path, message }` → `{ id }`.
pub(super) async fn git_commit(id: Value, p: &Value) -> Response {
    let Some(message) = p["message"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing message");
    };
    git_op(id, p, move |r| Ok(json!({ "id": r.commit(&message)? }))).await
}

/// `git.branches` `{ path }` → `{ branches: [...] }`.
pub(super) async fn git_branches(id: Value, p: &Value) -> Response {
    git_op(id, p, |r| Ok(json!({ "branches": r.branches()? }))).await
}

/// `git.create_branch` `{ path, name }` → `{}`.
pub(super) async fn git_create_branch(id: Value, p: &Value) -> Response {
    let Some(name) = p["name"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing name");
    };
    git_op(id, p, move |r| {
        r.create_branch(&name)?;
        Ok(json!({}))
    })
    .await
}

/// `git.checkout` `{ path, name }` → `{}`.
pub(super) async fn git_checkout(id: Value, p: &Value) -> Response {
    let Some(name) = p["name"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing name");
    };
    git_op(id, p, move |r| {
        r.checkout(&name)?;
        Ok(json!({}))
    })
    .await
}

/// `git.log` `{ path, limit? }` → `{ commits: [...] }`.
pub(super) async fn git_log(id: Value, p: &Value) -> Response {
    let limit = usize::try_from(p["limit"].as_u64().unwrap_or(50)).unwrap_or(50);
    git_op(id, p, move |r| Ok(json!({ "commits": r.log(limit)? }))).await
}

/// `git.fetch` `{ path, remote? }` → `{}`.
pub(super) async fn git_fetch(id: Value, p: &Value) -> Response {
    let remote = p["remote"].as_str().unwrap_or("origin").to_owned();
    git_op(id, p, move |r| {
        r.fetch(&remote)?;
        Ok(json!({}))
    })
    .await
}

/// `git.push` `{ path, remote? }` → `{}`.
pub(super) async fn git_push(id: Value, p: &Value) -> Response {
    let remote = p["remote"].as_str().unwrap_or("origin").to_owned();
    git_op(id, p, move |r| {
        r.push(&remote)?;
        Ok(json!({}))
    })
    .await
}

/// `git.worktrees` `{ path }` → `{ worktrees: [...] }`.
pub(super) async fn git_worktrees(id: Value, p: &Value) -> Response {
    git_op(id, p, |r| Ok(json!({ "worktrees": r.worktrees()? }))).await
}

/// `git.add_worktree` `{ path, name, worktree_path }` → `{}`.
pub(super) async fn git_add_worktree(id: Value, p: &Value) -> Response {
    let Some(name) = p["name"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing name");
    };
    let Some(wt_path) = p["worktree_path"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing worktree_path");
    };
    git_op(id, p, move |r| {
        r.add_worktree(&name, &wt_path)?;
        Ok(json!({}))
    })
    .await
}

// ── DB schema browser handlers ──────────────────────────────────────────────

/// `db.schema.tables` `{ conn }` → `{ tables: [...] }`.
pub(super) async fn db_schema_tables(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn) = p["conn"].as_str() else {
        return Response::err(id, -32602, "missing conn");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    match enzo_db::introspect::list_tables(&pool).await {
        Ok(tables) => Response::ok(id, json!({ "tables": tables })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

/// `db.schema.columns` `{ conn, table }` → `{ columns: [...] }`.
pub(super) async fn db_schema_columns(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(table)) = (p["conn"].as_str(), p["table"].as_str()) else {
        return Response::err(id, -32602, "missing conn or table");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    match enzo_db::introspect::columns(&pool, table).await {
        Ok(cols) => Response::ok(id, json!({ "columns": cols })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

/// `db.schema.indexes` `{ conn, table }` → `{ indexes: [...] }`.
pub(super) async fn db_schema_indexes(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(table)) = (p["conn"].as_str(), p["table"].as_str()) else {
        return Response::err(id, -32602, "missing conn or table");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    match enzo_db::introspect::indexes(&pool, table).await {
        Ok(idx) => Response::ok(id, json!({ "indexes": idx })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

// ── DB table viewer / editor handlers ───────────────────────────────────────

/// `db.table.browse` `{ conn, table, page?, size? }` → `{ columns, rows, total }`.
pub(super) async fn db_table_browse(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(table)) = (p["conn"].as_str(), p["table"].as_str()) else {
        return Response::err(id, -32602, "missing conn or table");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    let page = Page::new(
        p["page"].as_u64().unwrap_or(0),
        p["size"].as_u64().unwrap_or(100),
    );

    let browse_sql = enzo_db::table::browse_page_sql(table, page);
    let count_sql = enzo_db::table::count_table_sql(table);

    let rows = match pool.query(&browse_sql).await {
        Ok(b) => match enzo_db::batches_to_json(&b) {
            Ok(v) => v,
            Err(e) => return Response::err(id, -32000, e.to_string()),
        },
        Err(e) => return Response::err(id, -32000, e.to_string()),
    };
    let total = match pool.query(&count_sql).await {
        Ok(b) => enzo_db::batches_to_json(&b)
            .ok()
            .and_then(|v| v["rows"][0][0].as_str().and_then(|s| s.parse::<u64>().ok()))
            .unwrap_or(0),
        Err(_) => 0,
    };

    Response::ok(
        id,
        json!({
            "columns": rows["columns"],
            "rows": rows["rows"],
            "total": total,
            "page": page.index,
            "size": page.size,
        }),
    )
}

/// Parse a JSON array of `{ column, value }` objects into [`Cell`]s.
fn parse_cells(v: &Value) -> Vec<Cell> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(Cell {
                        column: c["column"].as_str()?.to_owned(),
                        value: c["value"].as_str().map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `db.table.update` `{ conn, table, cells:[...], pk:[...] }` → `{ affected }`.
pub(super) async fn db_table_update(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(table)) = (p["conn"].as_str(), p["table"].as_str()) else {
        return Response::err(id, -32602, "missing conn or table");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    let cells = parse_cells(&p["cells"]);
    let pk = parse_cells(&p["pk"]);
    let sql = match enzo_db::table::update_row_sql(table, &cells, &pk) {
        Ok(s) => s,
        Err(e) => return Response::err(id, -32602, e.to_string()),
    };
    exec_and_reply(id, &pool, &sql).await
}

/// `db.table.delete` `{ conn, table, pk:[...] }` → `{ affected }`.
pub(super) async fn db_table_delete(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(table)) = (p["conn"].as_str(), p["table"].as_str()) else {
        return Response::err(id, -32602, "missing conn or table");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    let pk = parse_cells(&p["pk"]);
    let sql = match enzo_db::table::delete_row_sql(table, &pk) {
        Ok(s) => s,
        Err(e) => return Response::err(id, -32602, e.to_string()),
    };
    exec_and_reply(id, &pool, &sql).await
}

/// `db.table.insert` `{ conn, table, cells:[...] }` → `{ affected }`.
pub(super) async fn db_table_insert(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(table)) = (p["conn"].as_str(), p["table"].as_str()) else {
        return Response::err(id, -32602, "missing conn or table");
    };
    let Some(pool) = state.get_db_conn(conn).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    let cells = parse_cells(&p["cells"]);
    let sql = match enzo_db::table::insert_row_sql(table, &cells) {
        Ok(s) => s,
        Err(e) => return Response::err(id, -32602, e.to_string()),
    };
    exec_and_reply(id, &pool, &sql).await
}

async fn exec_and_reply(id: Value, pool: &enzo_db::AnyPool, sql: &str) -> Response {
    match pool.execute(sql).await {
        Ok(n) => Response::ok(id, json!({ "affected": n })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

// ── DB query-tab handlers ───────────────────────────────────────────────────

/// `db.tabs.list` `{ conn }` → tab summary.
pub(super) async fn db_tabs_list(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn) = p["conn"].as_str() else {
        return Response::err(id, -32602, "missing conn");
    };
    let v = state.with_db_tabs(conn, |t| t.to_json()).await;
    Response::ok(id, v)
}

/// `db.tabs.open` `{ conn }` → `{ id }`.
pub(super) async fn db_tabs_open(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn) = p["conn"].as_str() else {
        return Response::err(id, -32602, "missing conn");
    };
    let tab_id = state
        .with_db_tabs(conn, enzo_db::tabs::TabManager::open)
        .await;
    Response::ok(id, json!({ "id": tab_id }))
}

/// `db.tabs.close` `{ conn, tab }` → `{ closed }`.
pub(super) async fn db_tabs_close(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(tab)) = (p["conn"].as_str(), p["tab"].as_str()) else {
        return Response::err(id, -32602, "missing conn or tab");
    };
    let tab = tab.to_owned();
    let closed = state.with_db_tabs(conn, move |t| t.close(&tab)).await;
    Response::ok(id, json!({ "closed": closed }))
}

/// `db.tabs.rename` `{ conn, tab, title }` → `{ ok }`.
pub(super) async fn db_tabs_rename(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(tab), Some(title)) =
        (p["conn"].as_str(), p["tab"].as_str(), p["title"].as_str())
    else {
        return Response::err(id, -32602, "missing conn, tab, or title");
    };
    let (tab, title) = (tab.to_owned(), title.to_owned());
    let ok = state
        .with_db_tabs(conn, move |t| t.rename(&tab, &title))
        .await;
    Response::ok(id, json!({ "ok": ok }))
}

/// `db.tabs.set_sql` `{ conn, tab, sql }` → `{ ok }`.
pub(super) async fn db_tabs_set_sql(id: Value, p: &Value, state: &DaemonState) -> Response {
    let (Some(conn), Some(tab), Some(sql)) =
        (p["conn"].as_str(), p["tab"].as_str(), p["sql"].as_str())
    else {
        return Response::err(id, -32602, "missing conn, tab, or sql");
    };
    let (tab, sql) = (tab.to_owned(), sql.to_owned());
    let ok = state
        .with_db_tabs(conn, move |t| t.set_sql(&tab, &sql))
        .await;
    Response::ok(id, json!({ "ok": ok }))
}
