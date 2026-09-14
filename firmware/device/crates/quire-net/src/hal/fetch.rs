//! Client-side fetches (Bookshop, OPDS, Wikipedia, weather, news, sleep packs, OTA,
//! sync, Calibre). The transport is in place — a station session with DNS and TCP — but
//! the fetchers themselves are a separate task: every request answers with a clear
//! "not implemented yet" so the UI never waits.
//!
//! Extension point: replace the body of [`fetch`] with a reqwless client (`reqwless`
//! 0.14 over `embassy_net::tcp::client::TcpClient` + `DnsSocket`; HTTPS through
//! `embedded-tls` 0.19 with two 16 KB record buffers allocated only for the duration
//! of the call).

use alloc::string::String;
use quire_ui::net::{FetchRequest, NetEvent};
use quire_ui::Event;

use crate::{post, NetToMain};

const NOT_YET: &str = "not implemented yet";

/// Handle one fetch request; posts the matching `NetEvent`.
pub async fn fetch(req: FetchRequest) {
    let ev = match req {
        FetchRequest::Book { .. } | FetchRequest::Cancel(_) | FetchRequest::Retry(_) => NetEvent::Downloads,
        FetchRequest::Opds(_) => NetEvent::Opds(Err(String::from(NOT_YET))),
        FetchRequest::Shelves => NetEvent::Shelves,
        FetchRequest::Wikipedia(_) => NetEvent::Wikipedia(Err(String::from(NOT_YET))),
        FetchRequest::Weather => NetEvent::Weather,
        FetchRequest::News => NetEvent::News,
        FetchRequest::SleepPacks | FetchRequest::SleepPack(_) => NetEvent::SleepPacks,
        FetchRequest::OtaCheck => NetEvent::Ota(Err(String::from(NOT_YET))),
    };
    post(NetToMain::Ui(Event::Net(ev))).await;
}

/// `SysRequest::Ota(path)`.
pub async fn ota(_source: String) {
    post(NetToMain::Ui(Event::Net(NetEvent::OtaProgress { done: 0, total: 0, finished: Some(Err(String::from(NOT_YET))) }))).await;
}

/// `SysRequest::Calibre(on)`.
pub async fn calibre(on: bool) {
    crate::with(|i| i.calibre_status = String::from(if on { "Calibre server: not implemented yet" } else { "" }));
    post(NetToMain::Ui(Event::Net(NetEvent::Calibre(String::from(NOT_YET))))).await;
}

/// `SysRequest::SyncNow`.
pub async fn sync_now() {
    post(NetToMain::Ui(Event::Net(NetEvent::Sync(Err(String::from(NOT_YET)))))).await;
}
