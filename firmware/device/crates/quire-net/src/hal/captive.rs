//! Captive DNS for the hotspot: every name resolves to the device (edge-captive's codec
//! over an embassy-net UDP socket).

use alloc::boxed::Box;
use core::net::Ipv4Addr;

use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::Stack;

/// Serve DNS on port 53 forever.
pub async fn run(stack: Stack<'_>, ip: Ipv4Addr) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf: Box<[u8]> = alloc::vec![0u8; 1024].into_boxed_slice();
    let mut tx_buf: Box<[u8]> = alloc::vec![0u8; 1024].into_boxed_slice();
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if socket.bind(53).is_err() {
        log::warn!("captive dns: bind failed");
        return;
    }
    let mut req = alloc::vec![0u8; 512];
    let mut reply = alloc::vec![0u8; 512];
    loop {
        let Ok((n, meta)) = socket.recv_from(&mut req).await else { continue };
        match edge_captive::reply(&req[..n], &ip.octets(), core::time::Duration::from_secs(60), &mut reply) {
            Ok(len) => {
                let _ = socket.send_to(&reply[..len], meta.endpoint).await;
            }
            Err(e) => log::debug!("captive dns: {e:?}"),
        }
    }
}
