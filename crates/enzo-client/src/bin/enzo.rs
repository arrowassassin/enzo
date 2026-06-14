//! Enzo client — GPU workspace UI (egui/eframe) over the ATP daemon.
//!
//! Usage: `enzo-client`  (or launch via the `enzo` orchestrator)
//!
//! Environment:
//!   `ENZO_ATP_SOCK`  Override the daemon socket path (default: /tmp/enzo-atp.sock)
//!   `RUST_LOG`       Log level (e.g. `debug`)

fn main() -> eframe::Result<()> {
    env_logger::init();
    enzo_client::gui::run()
}
