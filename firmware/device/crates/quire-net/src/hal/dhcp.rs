//! DHCP server for the hotspot (edge-dhcp's codec over an embassy-net UDP socket):
//! leases 192.168.4.50–200, the device as gateway and DNS, and the captive URL option.

use alloc::boxed::Box;
use core::net::{Ipv4Addr, SocketAddrV4};

use edge_dhcp::server::{Server, ServerOptions};
use edge_dhcp::{Options, Packet};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Stack};

/// Serve DHCP on port 67 forever.
pub async fn run(stack: Stack<'_>, ip: Ipv4Addr) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf: Box<[u8]> = alloc::vec![0u8; 1536].into_boxed_slice();
    let mut tx_buf: Box<[u8]> = alloc::vec![0u8; 1536].into_boxed_slice();
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if socket.bind(67).is_err() {
        log::warn!("dhcp: bind failed");
        return;
    }
    let mut gw = [ip];
    let mut options = ServerOptions::new(ip, Some(&mut gw));
    let dns = [ip];
    options.dns = &dns;
    options.captive_url = Some("http://192.168.4.1/captive");
    options.lease_duration_secs = 3600;
    let mut server: Server<_, 8> = Server::new(|| embassy_time::Instant::now().as_secs(), ip);
    let mut buf = alloc::vec![0u8; 1024];
    let mut out = alloc::vec![0u8; 1024];
    loop {
        let Ok((n, meta)) = socket.recv_from(&mut buf).await else { continue };
        let request = match Packet::decode(&buf[..n]) {
            Ok(r) => r,
            Err(e) => {
                log::debug!("dhcp: bad packet {e:?}");
                continue;
            }
        };
        let mut opt_buf = Options::buf();
        let Some(reply) = server.handle_request(&mut opt_buf, &options, &request) else { continue };
        // RFC 2131 §4.1: relay, else unicast to a client that already has an address and
        // did not ask for broadcast, else broadcast.
        let dest = if !request.giaddr.is_unspecified() {
            SocketAddrV4::new(request.giaddr, 67)
        } else if !request.ciaddr.is_unspecified() && !request.broadcast {
            SocketAddrV4::new(request.ciaddr, 68)
        } else {
            SocketAddrV4::new(Ipv4Addr::BROADCAST, 68)
        };
        let _ = meta;
        match reply.encode(&mut out) {
            Ok(bytes) => {
                let ep = IpEndpoint::new(IpAddress::Ipv4(*dest.ip()), dest.port());
                if let Err(e) = socket.send_to(bytes, ep).await {
                    log::debug!("dhcp: send {e:?}");
                }
            }
            Err(e) => log::debug!("dhcp: encode {e:?}"),
        }
    }
}
