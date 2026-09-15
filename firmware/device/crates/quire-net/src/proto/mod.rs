//! The pure part of the network layer: codecs, parsers and writers with no dependency on
//! the radio or the executor, unit-tested on the host
//! (`cargo test --target x86_64-unknown-linux-gnu --no-default-features`).

pub mod api;
pub mod bignum;
pub mod calibre;
pub mod ec;
pub mod feeds;
pub mod github;
pub mod hotspot;
pub mod html;
pub mod json;
pub mod jsonlite;
pub mod kosync;
pub mod md5;
pub mod mirror;
pub mod multipart;
pub mod opds;
pub mod pin;
pub mod range;
pub mod routes;
pub mod rsa;
pub mod settings_json;
pub mod sleepidx;
pub mod url;
pub mod weather;
pub mod wifi_bin;
pub mod wiki;
pub mod wsmsg;
pub mod x509;
pub mod xmlscan;
