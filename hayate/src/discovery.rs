//! Discovery over UDP multicast/broadcast using compio.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use sha2::{Digest, Sha256};

/// RAII guard that signals a broadcaster to stop when dropped.
///
/// When the guard is dropped it sends a cancel signal over the channel,
/// ensuring the UDP broadcast task terminates cleanly.
pub struct BroadcasterGuard {
    tx: Option<flume::Sender<()>>,
}

impl BroadcasterGuard {
    /// Creates a new guard linked to the given cancel channel.
    #[must_use]
    pub fn new(tx: flume::Sender<()>) -> Self {
        Self { tx: Some(tx) }
    }
}

impl Drop for BroadcasterGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Result of a discovery probe containing peer metadata.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// Human-readable name of the peer.
    pub name: String,
    /// Socket address of the peer's QUIC listener.
    pub addr: SocketAddr,
    /// Operating system reported by the peer.
    pub os: String,
    /// Round-trip time of the probe in milliseconds.
    pub rtt_ms: Option<f64>,
}

/// Computes SHA-256 of the phrase and returns the first 4 bytes as a hex string.
#[must_use]
pub fn derive_channel_id(phrase: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(phrase.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..4])
}

/// Periodically broadcasts a UDP packet with the channel ID and QUIC listening port.
pub async fn start_broadcaster(
    channel_id: &str,
    port: u16,
    cancel_rx: flume::Receiver<()>,
) -> Result<(), io::Error> {
    let socket = compio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;
    let msg = format!(
        "HAYATE_PEER:{}:{}:{}",
        channel_id,
        std::env::consts::OS,
        port
    );
    let msg_bytes = msg.into_bytes();
    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), 50002);
    let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50002);

    loop {
        let compio::BufResult(res, _) = socket.send_to(msg_bytes.clone(), target).await;
        if let Err(_e) = res {
            // Ignore network send errors silently
        }
        let compio::BufResult(res, _) = socket.send_to(msg_bytes.clone(), loopback).await;
        if let Err(_e) = res {
            // Ignore
        }

        let sleep_fut = compio::time::sleep(Duration::from_secs(1));
        let cancel_fut = cancel_rx.recv_async();
        let sleep_pinned = std::pin::pin!(sleep_fut);
        let cancel_pinned = std::pin::pin!(cancel_fut);

        if let futures_util::future::Either::Right(_) =
            futures_util::future::select(sleep_pinned, cancel_pinned).await
        {
            break;
        }
    }
    Ok(())
}

/// Listens for a UDP broadcast. If `target_phrase` is provided, it only yields a peer
/// whose derived `ChannelID` matches. Otherwise, it yields the first peer detected.
/// Returns the peer's resolved IP and port if found within the timeout.
pub async fn listen_for_broadcast(
    target_phrase: Option<&str>,
    timeout: Duration,
) -> Result<Option<(String, SocketAddr, String)>, io::Error> {
    let target_channel_id = target_phrase.map(derive_channel_id);

    let std_socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    std_socket.set_reuse_address(true)?;
    #[cfg(not(windows))]
    std_socket.set_reuse_port(true)?;

    let listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 50002);
    std_socket.bind(&socket2::SockAddr::from(listen_addr))?;

    let socket = compio::net::UdpSocket::from_std(std_socket.into())?;
    let buf = vec![0u8; 1024];

    // Wrap in a compio timeout
    let res = compio::time::timeout(timeout, async move {
        let mut temp_buf = buf;
        loop {
            let compio::BufResult(recv_res, b) = socket.recv_from(temp_buf).await;
            temp_buf = b;
            match recv_res {
                Ok((n, src_addr)) => {
                    let data = &temp_buf[..n];
                    if let Ok(text) = std::str::from_utf8(data) {
                        let mut parts = text.split(':');
                        // Format: HAYATE_PEER:<ChannelID>:<OS>:<Port>
                        let parsed = (|| {
                            if parts.next()? != "HAYATE_PEER" {
                                return None;
                            }
                            let channel_id = parts.next()?;
                            let os = parts.next()?;
                            let port_str = parts.next()?;
                            let matches = match &target_channel_id {
                                Some(expected_id) => channel_id == expected_id,
                                None => true,
                            };
                            if matches {
                                let port = port_str.parse::<u16>().ok()?;
                                let peer_addr = SocketAddr::new(src_addr.ip(), port);
                                Some(("Hayate Peer".to_owned(), peer_addr, os.to_owned()))
                            } else {
                                None
                            }
                        })();
                        if let Some(res) = parsed {
                            return Ok(Some(res));
                        }
                    }
                }
                Err(e) => return Err(e),
            }
        }
    })
    .await;

    match res {
        Ok(inner_res) => inner_res,
        Err(_) => Ok(None), // timeout expired
    }
}
