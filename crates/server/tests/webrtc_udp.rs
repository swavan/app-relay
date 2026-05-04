//! Phase D.1.2 integration test — drives two `Str0mWebRtcPeer`
//! instances over real `StdUdpTransport` sockets to confirm that
//! ICE binding requests flow in both directions once SDP negotiation
//! completes.
//!
//! `#[ignore]` because the test runs through the kernel's network
//! stack and DTLS/ICE timing can flake on hostile environments
//! (sandboxes that block AF_INET, CI runners with packet loss, etc).
//! The rest of the unit-test suite already covers the sans-IO API
//! surface; this is a smoke test for the real socket path.
//!
//! Only built with the `webrtc-peer` feature on. Without the feature
//! the file is empty so `cargo test --workspace` (default features)
//! still runs.

#![cfg(feature = "webrtc-peer")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use apprelay_core::{StdUdpTransport, Str0mWebRtcPeer, WebRtcPeer, WebRtcUdpTransport};
use apprelay_protocol::{SdpRole, SignalingEnvelope, WebRtcPeerRole};

fn loopback_zero() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

#[test]
#[ignore = "drives real UDP sockets; flaky in hostile network sandboxes"]
fn loopback_ice_binding_round_trip_through_real_sockets() {
    // Two transports bound on loopback with kernel-assigned ports.
    let transport_a =
        StdUdpTransport::bind_with_timeout(loopback_zero(), Duration::from_millis(50))
            .expect("bind transport A");
    let transport_b =
        StdUdpTransport::bind_with_timeout(loopback_zero(), Duration::from_millis(50))
            .expect("bind transport B");

    let mut offerer = Str0mWebRtcPeer::with_local_socket(transport_a.local_addr());
    let mut answerer = Str0mWebRtcPeer::with_local_socket(transport_b.local_addr());

    offerer
        .start("session-1", "stream-1", WebRtcPeerRole::Offerer)
        .expect("start offerer");
    answerer
        .start("session-1", "stream-1", WebRtcPeerRole::Answerer)
        .expect("start answerer");

    // Exchange SDP through the peers' outbound queues.
    let outbound_a = offerer.take_outbound_signaling("session-1");
    let offer_sdp = outbound_a
        .into_iter()
        .find_map(|env| match env {
            SignalingEnvelope::SdpOffer { sdp, .. } => Some(sdp),
            _ => None,
        })
        .expect("offerer produced SDP offer");

    answerer
        .consume_signaling(
            "session-1",
            SignalingEnvelope::SdpOffer {
                sdp: offer_sdp,
                role: SdpRole::Offerer,
            },
        )
        .expect("answerer consumes offer");
    let outbound_b = answerer.take_outbound_signaling("session-1");
    let answer_sdp = outbound_b
        .into_iter()
        .find_map(|env| match env {
            SignalingEnvelope::SdpAnswer { sdp } => Some(sdp),
            _ => None,
        })
        .expect("answerer produced SDP answer");
    offerer
        .consume_signaling(
            "session-1",
            SignalingEnvelope::SdpAnswer { sdp: answer_sdp },
        )
        .expect("offerer consumes answer");

    // Drive a tight pump loop directly on the test thread (no
    // background worker — keeps the test deterministic). 2-second
    // budget is enough for the STUN-binding round trip on a healthy
    // loopback; the worker thread cadence (~100 ms) is the upper
    // bound we'd otherwise hit.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut buf = [0u8; 2048];
    let mut a_received = 0usize;
    let mut b_received = 0usize;
    while Instant::now() < deadline && (a_received == 0 || b_received == 0) {
        // Drain outbound from both peers and write to the matching
        // transport.
        for batch in offerer.take_outbound_rtp() {
            let _ = transport_a.send_to(&batch.payload, batch.destination);
        }
        for batch in answerer.take_outbound_rtp() {
            let _ = transport_b.send_to(&batch.payload, batch.destination);
        }

        // Read with a short timeout on both transports.
        if let Ok((n, source)) = transport_a.recv_from(&mut buf) {
            if offerer
                .handle_inbound_datagram(source, transport_a.local_addr(), &buf[..n])
                .unwrap_or(false)
            {
                a_received += 1;
            }
        }
        if let Ok((n, source)) = transport_b.recv_from(&mut buf) {
            if answerer
                .handle_inbound_datagram(source, transport_b.local_addr(), &buf[..n])
                .unwrap_or(false)
            {
                b_received += 1;
            }
        }
    }

    assert!(
        a_received > 0 && b_received > 0,
        "expected ICE binding traffic in both directions; got a={a_received} b={b_received}"
    );
}
