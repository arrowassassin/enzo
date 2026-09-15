//! Progress sync with a KOReader-compatible server (`proto::kosync`): the positions of
//! the books read most recently are pushed, each named by the partial MD5 of its file.
//! A position the server holds that is newer than ours is left alone (and logged): the
//! main loop owns the library in RAM, so pulling would need a hook there.

use alloc::string::String;
use alloc::vec::Vec;

use embassy_net::Stack;
use quire_library::{Library, Status};
use quire_ui::net::NetEvent;
use quire_ui::{Event, Settings};

use super::client::{self, Error, Method, Request};
use crate::proto::kosync;
use crate::{post, with, CardFs, DynFs, NetToMain};

/// Books pushed per sync, most recently opened first.
const MAX_BOOKS: usize = 20;

struct Pos {
    path: String,
    section: u16,
    chars: u32,
    total: u32,
    last_opened: u32,
}

/// `SysRequest::SyncNow`.
pub async fn sync_now(stack: Stack<'_>, fs: &'static dyn CardFs) {
    let result = run(stack, fs).await;
    post(NetToMain::Ui(Event::Net(NetEvent::Sync(result)))).await;
}

async fn run(stack: Stack<'_>, fs: &'static dyn CardFs) -> Result<u32, String> {
    let s = Settings::load(&DynFs(fs));
    if s.sync_url.trim().is_empty() || s.sync_user.trim().is_empty() || s.sync_key.is_empty() {
        return Err(String::from("Set the server, user and key first."));
    }
    let base = kosync::base(&s.sync_url);
    let key = kosync::auth_key(&s.sync_key);
    let user = String::from(s.sync_user.trim());
    let device_id = with(|i| i.status.hostname.clone());
    let device_id = if device_id.is_empty() { String::from("quire") } else { device_id };
    let mut books: Vec<Pos> = {
        let lib = Library::load(&DynFs(fs));
        lib.books
            .iter()
            .filter(|b| !b.missing && b.path.starts_with('/') && b.status != Status::Unread && b.chars > 0)
            .map(|b| Pos { path: b.path.clone(), section: b.loc.section, chars: b.loc.chars, total: b.chars, last_opened: b.last_opened })
            .collect()
    };
    books.sort_by_key(|b| core::cmp::Reverse(b.last_opened));
    books.truncate(MAX_BOOKS);
    let headers = [
        ("x-auth-user", user.as_str()),
        ("x-auth-key", key.as_str()),
        ("Accept", "application/json"),
        ("Content-Type", "application/json"),
    ];
    let mut pushed = 0u32;
    let mut skipped = 0u32;
    for b in &books {
        let Ok(file) = fs.open(&b.path) else { continue };
        let doc = kosync::partial_md5(&*file);
        drop(file);
        let pct = (b.chars as f32 / b.total as f32).clamp(0.0, 1.0);
        // The server's copy first: a newer one from another reader is kept.
        let get_url = alloc::format!("{base}/syncs/progress/{doc}");
        match client::fetch_to_vec(stack, &Request { headers: &headers, ..Request::get(&get_url) }, 1024).await {
            Ok((_, body)) => {
                if let Some(sp) = kosync::parse_progress(&body) {
                    if sp.timestamp > b.last_opened as u64 && (sp.percentage - pct).abs() > 0.005 {
                        log::info!("sync: {} is newer on the server ({:.1}% from {})", b.path, sp.percentage * 100.0, sp.device);
                        skipped += 1;
                        continue;
                    }
                }
            }
            Err(Error::Status(404)) | Err(Error::Status(502)) => {}
            Err(e) => return finish(pushed, Err(e.text(&get_url))),
        }
        let body = kosync::put_body(&doc, &kosync::progress_text(b.section, b.chars), pct, &device_id);
        let put_url = alloc::format!("{base}/syncs/progress");
        let req = Request { method: Method::Put, url: &put_url, headers: &headers, body: Some(body.as_bytes()), range_from: None };
        match client::fetch_to_vec(stack, &req, 512).await {
            Ok(_) => pushed += 1,
            Err(e) => return finish(pushed, Err(e.text(&put_url))),
        }
    }
    log::info!("sync: {pushed} pushed, {skipped} newer on the server");
    Ok(pushed)
}

fn finish(pushed: u32, err: Result<u32, String>) -> Result<u32, String> {
    if pushed > 0 {
        Ok(pushed)
    } else {
        err
    }
}
