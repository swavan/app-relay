//! Phase D.0 scaffold for the `str0m`-backed server-side WebRTC peer.
//!
//! This module exists only when the `webrtc-peer` cargo feature is
//! enabled. Phase D.0 ships the contract + the type registration so
//! enabling the feature swaps the in-memory no-op for this scaffold.
//! Phase D.1 will replace the placeholder bodies with real `str0m`
//! integration (SDP/ICE handling consuming Phase C signaling, RTP
//! packetization of Phase B Annex-B H.264 payloads, peer state
//! machine).
//!
//! Until D.1 lands, every state-changing method returns
//! [`AppRelayError::ServiceUnavailable`] with a precise "Phase D.1
//! pending" message so enabling the feature is never a silent no-op.
//! Polling methods return empty `Vec`s — those are the explicit
//! "nothing pending" signal that the trait contract permits.
//!
//! The `mod str0m_webrtc_peer;` declaration in `lib.rs` is already
//! gated on `cfg(feature = "webrtc-peer")`, so this file inherits the
//! same gate without an inner `#![cfg(...)]` attribute.

use apprelay_protocol::{
    AppRelayError, EncodedVideoFrame, RtpPacketBatch, SignalingEnvelope, WebRtcPeerRole,
};

use crate::webrtc_peer::WebRtcPeer;

const PHASE_D1_PENDING: &str = "Phase D.1 pending: str0m WebRTC peer integration is not yet \
                                wired (feature `webrtc-peer` only registers the runtime)";

#[derive(Debug, Default)]
pub struct Str0mWebRtcPeer;

impl Str0mWebRtcPeer {
    pub fn new() -> Self {
        Self
    }
}

impl WebRtcPeer for Str0mWebRtcPeer {
    fn start(
        &mut self,
        _session_id: &str,
        _stream_id: &str,
        _role: WebRtcPeerRole,
    ) -> Result<(), AppRelayError> {
        Err(AppRelayError::ServiceUnavailable(
            PHASE_D1_PENDING.to_string(),
        ))
    }

    fn stop(&mut self, _session_id: &str, _stream_id: &str) -> Result<(), AppRelayError> {
        // `stop` on a never-started stream is a legitimate no-op (the
        // trait documents idempotency); reject only if a caller is
        // trying to stop an actually-tracked stream, which is
        // impossible until `start` lands in D.1. Returning Ok keeps
        // teardown paths tidy.
        Ok(())
    }

    fn consume_signaling(
        &mut self,
        _session_id: &str,
        _envelope: SignalingEnvelope,
    ) -> Result<(), AppRelayError> {
        Err(AppRelayError::ServiceUnavailable(
            PHASE_D1_PENDING.to_string(),
        ))
    }

    fn take_outbound_signaling(&mut self, _session_id: &str) -> Vec<SignalingEnvelope> {
        Vec::new()
    }

    fn push_encoded_frame(
        &mut self,
        _stream_id: &str,
        _frame: &EncodedVideoFrame,
    ) -> Result<(), AppRelayError> {
        Err(AppRelayError::ServiceUnavailable(
            PHASE_D1_PENDING.to_string(),
        ))
    }

    fn take_outbound_rtp(&mut self) -> Vec<RtpPacketBatch> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apprelay_protocol::SdpRole;

    fn frame() -> EncodedVideoFrame {
        EncodedVideoFrame {
            sequence: 1,
            timestamp_ms: 0,
            byte_length: 0,
            keyframe: true,
            payload: Vec::new(),
        }
    }

    #[test]
    fn start_returns_phase_d1_pending() {
        let mut peer = Str0mWebRtcPeer::new();
        let err = peer
            .start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect_err("expected ServiceUnavailable while D.1 is pending");
        assert!(
            matches!(err, AppRelayError::ServiceUnavailable(message) if message.contains("Phase D.1"))
        );
    }

    #[test]
    fn consume_signaling_returns_phase_d1_pending() {
        let mut peer = Str0mWebRtcPeer::new();
        let err = peer
            .consume_signaling(
                "session-1",
                SignalingEnvelope::SdpOffer {
                    sdp: "v=0".to_string(),
                    role: SdpRole::Offerer,
                },
            )
            .expect_err("expected ServiceUnavailable while D.1 is pending");
        assert!(matches!(err, AppRelayError::ServiceUnavailable(_)));
    }

    #[test]
    fn push_encoded_frame_returns_phase_d1_pending() {
        let mut peer = Str0mWebRtcPeer::new();
        let err = peer
            .push_encoded_frame("stream-1", &frame())
            .expect_err("expected ServiceUnavailable while D.1 is pending");
        assert!(matches!(err, AppRelayError::ServiceUnavailable(_)));
    }

    #[test]
    fn drains_are_empty_in_scaffold() {
        let mut peer = Str0mWebRtcPeer::new();
        assert!(peer.take_outbound_signaling("session-1").is_empty());
        assert!(peer.take_outbound_rtp().is_empty());
    }

    #[test]
    fn stop_is_idempotent_in_scaffold() {
        let mut peer = Str0mWebRtcPeer::new();
        peer.stop("session-unknown", "stream-unknown")
            .expect("stop is idempotent even on never-started streams");
    }
}
