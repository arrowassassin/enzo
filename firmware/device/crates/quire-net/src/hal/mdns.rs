//! mDNS responder: `<hostname>.local` and `_http._tcp` (edge-mdns's codec over an
//! embassy-net UDP socket joined to 224.0.0.251).

use alloc::boxed::Box;
use core::net::{Ipv4Addr, Ipv6Addr};

use edge_mdns::domain::base::Ttl;
use edge_mdns::host::{Host, Service, ServiceAnswers};
use edge_mdns::{ChainedHostAnswers, HostAnswersMdnsHandler, MdnsHandler, MdnsRequest, MdnsResponse};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Stack};
use embassy_time::{Duration, Timer};

const MDNS_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

/// Answer for `hostname.local` at `ip` forever.
pub async fn run(stack: Stack<'_>, hostname: &str, ip: Ipv4Addr) {
    if let Err(e) = stack.join_multicast_group(MDNS_ADDR) {
        log::warn!("mdns: multicast join {e:?}");
    }
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf: Box<[u8]> = alloc::vec![0u8; 1536].into_boxed_slice();
    let mut tx_buf: Box<[u8]> = alloc::vec![0u8; 1536].into_boxed_slice();
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if socket.bind(MDNS_PORT).is_err() {
        log::warn!("mdns: bind failed");
        return;
    }
    let host = Host { hostname, ipv4: ip, ipv6: Ipv6Addr::UNSPECIFIED, ttl: Ttl::from_secs(120) };
    let txt = [("path", "/")];
    let service = Service {
        name: hostname,
        priority: 0,
        weight: 0,
        service: "_http",
        protocol: "_tcp",
        port: 80,
        service_subtypes: &[],
        txt_kvs: &txt,
    };
    let answers = ChainedHostAnswers::new(&host, ServiceAnswers::new(&host, &service));
    let mut handler = HostAnswersMdnsHandler::new(answers);
    let group = IpEndpoint::new(IpAddress::Ipv4(MDNS_ADDR), MDNS_PORT);
    let mut req = alloc::vec![0u8; 1024];
    let mut reply = alloc::vec![0u8; 1024];

    // Announce a few times, then answer queries.
    for _ in 0..3 {
        if let Ok(MdnsResponse::Reply { data, .. }) = handler.handle(MdnsRequest::None, &mut reply) {
            let _ = socket.send_to(data, group).await;
        }
        Timer::after(Duration::from_millis(700)).await;
    }
    loop {
        let Ok((n, meta)) = socket.recv_from(&mut req).await else { continue };
        let multicast = matches!(meta.local_address, Some(IpAddress::Ipv4(a)) if a.is_multicast());
        let legacy = meta.endpoint.port != MDNS_PORT;
        let request = MdnsRequest::Request { legacy, multicast, data: &req[..n] };
        match handler.handle(request, &mut reply) {
            Ok(MdnsResponse::Reply { data, delay }) => {
                if delay {
                    Timer::after(Duration::from_millis(60)).await;
                }
                let to = if legacy { meta.endpoint } else { group };
                let _ = socket.send_to(data, to).await;
            }
            Ok(MdnsResponse::None) => {}
            Err(e) => log::debug!("mdns: {e:?}"),
        }
    }
}
