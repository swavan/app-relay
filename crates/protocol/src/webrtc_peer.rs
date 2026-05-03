//! Transport-neutral WebRTC peer types.
//!
//! Phase D.0 of the real-media implementation roadmap introduces the
//! structural ground for a server-side WebRTC peer. The peer itself is
//! sans-IO (Phase D.1 will plug in `str0m`), so the protocol crate only
//! owns the value types that cross the in-process boundary between the
//! server composition and the peer.
//!
//! No real WebRTC peer is wired here — Phase D.0 ships the contract
//! and an in-memory no-op default; Phase D.1 lands the real
//! `str0m`-backed implementation behind the `webrtc-peer` cargo
//! feature.

use std::net::SocketAddr;

/// Role of a WebRTC peer relative to SDP negotiation. Mirrors
/// [`crate::signaling::SdpRole`] but is reproduced here so the WebRTC
/// peer surface can be reasoned about without depending on the
/// signaling envelope vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WebRtcPeerRole {
    /// The peer drives SDP negotiation: it produces the offer and
    /// consumes the matching answer.
    Offerer,
    /// The peer responds to a remote offer with an answer.
    Answerer,
}

impl WebRtcPeerRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Offerer => "offerer",
            Self::Answerer => "answerer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "offerer" => Some(Self::Offerer),
            "answerer" => Some(Self::Answerer),
            _ => None,
        }
    }
}

/// One batch of outbound RTP/RTCP bytes the peer wants the runtime to
/// send to a remote socket address. The peer does not own the UDP
/// socket; the server composition is responsible for actually writing
/// these bytes (or, in Phase D.0, for confirming the batch is empty).
///
/// The payload is opaque to the protocol crate — Phase D.1 will
/// produce real DTLS/SRTP-protected datagrams, but for D.0 the type
/// only exists so the trait surface in `apprelay-core` can name it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RtpPacketBatch {
    pub destination: SocketAddr,
    pub payload: Vec<u8>,
}

impl RtpPacketBatch {
    pub fn new(destination: SocketAddr, payload: Vec<u8>) -> Self {
        Self {
            destination,
            payload,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.payload.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn webrtc_peer_role_round_trips_through_label() {
        for role in [WebRtcPeerRole::Offerer, WebRtcPeerRole::Answerer] {
            assert_eq!(WebRtcPeerRole::parse(role.label()), Some(role));
        }
        assert_eq!(WebRtcPeerRole::parse("observer"), None);
    }

    #[test]
    fn rtp_packet_batch_reports_payload_byte_length() {
        let batch = RtpPacketBatch::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
            vec![0u8; 12],
        );
        assert_eq!(batch.byte_length(), 12);
        assert_eq!(batch.destination.port(), 4242);
    }
}
