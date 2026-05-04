//! Phase D.1.2 — UDP transport boundary for the server-side WebRTC peer.
//!
//! The peer (`Str0mWebRtcPeer`) is sans-IO: it owns the protocol state
//! machine but never touches a socket. This module defines the trait
//! the I/O worker thread uses to actually read/write UDP datagrams,
//! plus a thin `std::net::UdpSocket`-backed implementation. Tests can
//! swap in an in-process channel-backed transport without binding a
//! real socket — see `tests::loopback_round_trip_through_channels`.
//!
//! Only available with the `webrtc-peer` cargo feature on, since the
//! transport is only useful in conjunction with the real peer.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

/// Default read timeout for [`StdUdpTransport`]. Short enough that the
/// background worker thread can poll a shutdown flag every loop, long
/// enough that we don't burn CPU when the network is idle.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(100);

/// Sans-IO transport boundary for the WebRTC peer's UDP datagrams.
///
/// The implementation owns a UDP socket; the peer owns the protocol
/// state machine. Tests can swap in an in-process channel-backed
/// transport without binding a real socket.
pub trait WebRtcUdpTransport: Send + Sync + std::fmt::Debug {
    /// Local socket address the transport advertises as the peer's
    /// host candidate. With a kernel-assigned port, this is the
    /// post-bind concrete `SocketAddr`.
    fn local_addr(&self) -> SocketAddr;

    /// Send a single UDP datagram. Returns `Ok(bytes_written)` (which
    /// must equal `payload.len()` for a successful UDP send).
    /// Implementations must NOT block indefinitely.
    fn send_to(&self, payload: &[u8], destination: SocketAddr) -> io::Result<usize>;

    /// Block up to the implementation-specific read timeout, copy any
    /// arriving datagram into `buf`, and return `(bytes_read, source)`.
    /// On timeout, return an `Err` whose `kind()` is `WouldBlock` or
    /// `TimedOut`. The caller distinguishes timeout from real error.
    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
}

/// `std::net::UdpSocket`-backed [`WebRtcUdpTransport`].
///
/// Holds the socket as `Arc<UdpSocket>` so cloning is cheap (the
/// background I/O worker and the peer construction site may both want
/// a handle for `local_addr()` lookups).
#[derive(Clone, Debug)]
pub struct StdUdpTransport {
    socket: Arc<UdpSocket>,
    local_addr: SocketAddr,
}

impl StdUdpTransport {
    /// Bind a UDP socket at `addr` and configure a default read
    /// timeout. Use `127.0.0.1:0` (or `0.0.0.0:0`) to ask the kernel
    /// for an ephemeral port; consult [`Self::local_addr`] afterwards
    /// for the concrete bound address.
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        Self::bind_with_timeout(addr, DEFAULT_READ_TIMEOUT)
    }

    /// Like [`Self::bind`] but with a caller-provided read timeout.
    /// A zero timeout disables the timeout entirely; callers should
    /// avoid that in production because the background worker relies
    /// on the timeout to poll the shutdown flag.
    pub fn bind_with_timeout(addr: SocketAddr, read_timeout: Duration) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        let timeout = if read_timeout.is_zero() {
            None
        } else {
            Some(read_timeout)
        };
        socket.set_read_timeout(timeout)?;
        let local_addr = socket.local_addr()?;
        Ok(Self {
            socket: Arc::new(socket),
            local_addr,
        })
    }

    /// Borrow the underlying `Arc<UdpSocket>` (mostly for tests).
    pub fn socket(&self) -> Arc<UdpSocket> {
        Arc::clone(&self.socket)
    }
}

impl WebRtcUdpTransport for StdUdpTransport {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    fn send_to(&self, payload: &[u8], destination: SocketAddr) -> io::Result<usize> {
        self.socket.send_to(payload, destination)
    }

    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Mutex;

    /// In-process channel-backed transport pair. `send_to` puts a
    /// datagram on the partner's queue; `recv_from` reads with a
    /// short blocking poll. Loopback round-trip without the OS
    /// network stack.
    #[derive(Debug)]
    struct ChannelTransport {
        local: SocketAddr,
        outgoing: Sender<(Vec<u8>, SocketAddr)>,
        incoming: Mutex<Receiver<(Vec<u8>, SocketAddr)>>,
    }

    fn paired(addr_a: SocketAddr, addr_b: SocketAddr) -> (ChannelTransport, ChannelTransport) {
        let (tx_a_to_b, rx_a_to_b) = channel::<(Vec<u8>, SocketAddr)>();
        let (tx_b_to_a, rx_b_to_a) = channel::<(Vec<u8>, SocketAddr)>();
        let a = ChannelTransport {
            local: addr_a,
            outgoing: tx_a_to_b,
            incoming: Mutex::new(rx_b_to_a),
        };
        let b = ChannelTransport {
            local: addr_b,
            outgoing: tx_b_to_a,
            incoming: Mutex::new(rx_a_to_b),
        };
        (a, b)
    }

    impl WebRtcUdpTransport for ChannelTransport {
        fn local_addr(&self) -> SocketAddr {
            self.local
        }

        fn send_to(&self, payload: &[u8], _destination: SocketAddr) -> io::Result<usize> {
            self.outgoing
                .send((payload.to_vec(), self.local))
                .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err.to_string()))?;
            Ok(payload.len())
        }

        fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
            let rx = self
                .incoming
                .lock()
                .map_err(|err| io::Error::other(err.to_string()))?;
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok((payload, source)) => {
                    let n = payload.len().min(buf.len());
                    buf[..n].copy_from_slice(&payload[..n]);
                    Ok((n, source))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err(io::Error::new(io::ErrorKind::WouldBlock, "timeout"))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err(io::Error::new(io::ErrorKind::BrokenPipe, "channel closed"))
                }
            }
        }
    }

    fn fake_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }

    #[test]
    fn loopback_round_trip_through_channels() {
        let (a, b) = paired(fake_addr(40001), fake_addr(40002));
        a.send_to(b"hello", b.local_addr()).expect("send a->b");
        let mut buf = [0u8; 32];
        let (n, source) = b.recv_from(&mut buf).expect("recv on b");
        assert_eq!(n, 5);
        assert_eq!(&buf[..n], b"hello");
        assert_eq!(source, a.local_addr());
    }

    #[test]
    fn channel_transport_recv_times_out_when_idle() {
        let (a, _b) = paired(fake_addr(40003), fake_addr(40004));
        let mut buf = [0u8; 8];
        let err = a.recv_from(&mut buf).expect_err("must time out when idle");
        assert!(matches!(
            err.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
    }

    #[test]
    fn std_udp_transport_binds_and_reports_local_addr() {
        let transport = StdUdpTransport::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .expect("bind ephemeral");
        let addr = transport.local_addr();
        assert!(addr.ip().is_loopback(), "expected loopback, got {addr}");
        assert_ne!(addr.port(), 0, "kernel should assign a real port");
    }

    #[test]
    fn std_udp_transport_round_trips_real_datagram() {
        let a = StdUdpTransport::bind_with_timeout(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            Duration::from_millis(500),
        )
        .expect("bind a");
        let b = StdUdpTransport::bind_with_timeout(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            Duration::from_millis(500),
        )
        .expect("bind b");

        let written = a.send_to(b"ping", b.local_addr()).expect("send");
        assert_eq!(written, 4);

        let mut buf = [0u8; 32];
        let (n, source) = b.recv_from(&mut buf).expect("recv on b");
        assert_eq!(n, 4);
        assert_eq!(&buf[..n], b"ping");
        assert_eq!(source, a.local_addr());
    }

    #[test]
    fn std_udp_transport_recv_times_out_when_idle() {
        let transport = StdUdpTransport::bind_with_timeout(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            Duration::from_millis(50),
        )
        .expect("bind");
        let mut buf = [0u8; 8];
        let err = transport
            .recv_from(&mut buf)
            .expect_err("must time out when idle");
        assert!(matches!(
            err.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
    }
}
