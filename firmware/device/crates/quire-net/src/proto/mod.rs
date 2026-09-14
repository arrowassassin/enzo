//! The pure part of the network layer: codecs, parsers and writers with no dependency on
//! the radio or the executor, unit-tested on the host
//! (`cargo test --target x86_64-unknown-linux-gnu --no-default-features`).

pub mod api;
pub mod hotspot;
pub mod json;
pub mod mirror;
pub mod multipart;
pub mod pin;
pub mod range;
pub mod routes;
pub mod settings_json;
pub mod wifi_bin;
pub mod wsmsg;
