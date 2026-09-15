//! One SNTP request per station session (RFC 4330): the reader's clock is kept by a
//! DS3231 that drifts a minute or two a year and is set by hand, so a fix from the pool
//! whenever the radio is up keeps it (and the TLS validity checks) honest. The main loop
//! turns the UTC fix into local time by keeping the zone offset the user's own setting
//! implies, so no time-zone setting is needed here.

use embassy_net::dns::DnsQueryType;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Stack};
use embassy_time::{with_timeout, Duration, Timer};

use crate::proto::sntp::parse;
use crate::{post, NetToMain};

/// The pool, resolved through the network's DNS.
const SERVER: &str = "pool.ntp.org";
const TIMEOUT: Duration = Duration::from_secs(4);

/// Wait for the stack to come up, ask once (three attempts a few seconds apart), and
/// post the UTC time as `NetToMain::TimeSync`.
pub async fn sync_once(stack: Stack<'_>) {
    stack.wait_config_up().await;
    for attempt in 0..3u32 {
        if attempt > 0 {
            Timer::after(Duration::from_secs(5)).await;
        }
        match query(stack).await {
            Some(utc) => {
                log::info!("sntp: {utc}");
                post(NetToMain::TimeSync(utc)).await;
                return;
            }
            None => log::warn!("sntp: no answer (attempt {})", attempt + 1),
        }
    }
}

async fn query(stack: Stack<'_>) -> Option<u32> {
    let addrs = with_timeout(TIMEOUT, stack.dns_query(SERVER, DnsQueryType::A)).await.ok()?.ok()?;
    let ip: IpAddress = *addrs.first()?;
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf = [0u8; 96];
    let mut tx_buf = [0u8; 96];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(0).ok()?;
    // LI 0, version 4, mode 3 (client); everything else zero is a valid request.
    let mut req = [0u8; 48];
    req[0] = 0x23;
    socket.send_to(&req, IpEndpoint::new(ip, 123)).await.ok()?;
    let mut resp = [0u8; 48];
    let (n, _) = with_timeout(TIMEOUT, socket.recv_from(&mut resp)).await.ok()?.ok()?;
    parse(&resp[..n])
}
