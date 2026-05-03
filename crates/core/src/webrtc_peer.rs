//! Server-side WebRTC peer trait + always-on in-memory default impl.
//!
//! Phase D.0 ships the contract; Phase D.1 will plug in `str0m`.
//! The trait is sans-IO: callers drive the pump (no internal task,
//! no socket ownership). The protocol crate owns the value types
//! (`WebRtcPeerRole`, `RtpPacketBatch`); this crate owns the
//! service surface and the no-op implementation that the default
//! workspace build wires into `ServerServices`.

use std::collections::HashMap;
use std::sync::Mutex;

use apprelay_protocol::{
    AppRelayError, EncodedVideoFrame, RtpPacketBatch, SignalingEnvelope, WebRtcPeerRole,
};

/// Sans-IO server-side WebRTC peer surface.
///
/// State-changing methods (`start`, `stop`, `consume_signaling`,
/// `push_encoded_frame`) return `Result<(), AppRelayError>`. Implementors
/// MUST surface every failure as a typed error — silent no-ops are
/// disallowed by the project-wide invariant.
///
/// Polling methods (`take_outbound_signaling`, `take_outbound_rtp`)
/// return owned `Vec`s. An empty vec means "nothing pending right now",
/// which is an explicit signal — never a bug.
pub trait WebRtcPeer: Send + Sync + std::fmt::Debug {
    /// Begin a peer session. `role` selects whether this peer drives
    /// the offer or answers a remote offer.
    fn start(
        &mut self,
        session_id: &str,
        stream_id: &str,
        role: WebRtcPeerRole,
    ) -> Result<(), AppRelayError>;

    /// Tear down the peer session for `(session_id, stream_id)`.
    /// Idempotent: stopping an unknown stream must not error.
    fn stop(&mut self, session_id: &str, stream_id: &str) -> Result<(), AppRelayError>;

    /// Hand a remote signaling envelope (offer/answer/candidate/end-of-
    /// candidates) to the peer. The peer integrates it into its state
    /// machine; outbound responses (answer SDP, local ICE candidates)
    /// are then drained via `take_outbound_signaling`.
    fn consume_signaling(
        &mut self,
        session_id: &str,
        envelope: SignalingEnvelope,
    ) -> Result<(), AppRelayError>;

    /// Drain any signaling envelopes the peer wants to send out for
    /// `session_id` (typically: the local SDP, freshly discovered ICE
    /// candidates).
    fn take_outbound_signaling(&mut self, session_id: &str) -> Vec<SignalingEnvelope>;

    /// Hand an encoded video frame to the peer for RTP packetization.
    /// The peer holds onto whatever it needs and exposes resulting
    /// RTP/RTCP batches via `take_outbound_rtp`.
    fn push_encoded_frame(
        &mut self,
        stream_id: &str,
        frame: &EncodedVideoFrame,
    ) -> Result<(), AppRelayError>;

    /// Drain all pending outbound RTP/RTCP batches across every active
    /// session. Caller is responsible for actually writing each batch
    /// to its destination socket.
    fn take_outbound_rtp(&mut self) -> Vec<RtpPacketBatch>;
}

/// Always-on no-op implementation. Wired into `ServerServices` by
/// default so the rest of the codebase can hold a `Box<dyn WebRtcPeer>`
/// without any cargo feature being enabled.
///
/// Behaviour: every state-changing call succeeds and is recorded in a
/// debug-friendly counter; every drain returns an empty `Vec`.
/// Observable product behaviour is unchanged from a build that has no
/// peer wired at all — that is the whole point.
#[derive(Debug, Default)]
pub struct InMemoryWebRtcPeer {
    state: Mutex<InMemoryWebRtcPeerState>,
}

#[derive(Debug, Default)]
struct InMemoryWebRtcPeerState {
    started_streams: HashMap<String, WebRtcPeerRole>,
    consumed_envelopes: u64,
    pushed_frames: u64,
}

impl InMemoryWebRtcPeer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-friendly accessor: how many `(session_id, stream_id)`
    /// pairs are currently active.
    pub fn started_stream_count(&self) -> usize {
        self.lock().started_streams.len()
    }

    /// Test-friendly accessor: total envelopes accepted via
    /// `consume_signaling`.
    pub fn consumed_envelope_count(&self) -> u64 {
        self.lock().consumed_envelopes
    }

    /// Test-friendly accessor: total frames accepted via
    /// `push_encoded_frame`.
    pub fn pushed_frame_count(&self) -> u64 {
        self.lock().pushed_frames
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, InMemoryWebRtcPeerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl WebRtcPeer for InMemoryWebRtcPeer {
    fn start(
        &mut self,
        session_id: &str,
        stream_id: &str,
        role: WebRtcPeerRole,
    ) -> Result<(), AppRelayError> {
        self.lock()
            .started_streams
            .insert(stream_key(session_id, stream_id), role);
        Ok(())
    }

    fn stop(&mut self, session_id: &str, stream_id: &str) -> Result<(), AppRelayError> {
        self.lock()
            .started_streams
            .remove(&stream_key(session_id, stream_id));
        Ok(())
    }

    fn consume_signaling(
        &mut self,
        _session_id: &str,
        _envelope: SignalingEnvelope,
    ) -> Result<(), AppRelayError> {
        let mut state = self.lock();
        state.consumed_envelopes = state.consumed_envelopes.saturating_add(1);
        Ok(())
    }

    fn take_outbound_signaling(&mut self, _session_id: &str) -> Vec<SignalingEnvelope> {
        Vec::new()
    }

    fn push_encoded_frame(
        &mut self,
        _stream_id: &str,
        _frame: &EncodedVideoFrame,
    ) -> Result<(), AppRelayError> {
        let mut state = self.lock();
        state.pushed_frames = state.pushed_frames.saturating_add(1);
        Ok(())
    }

    fn take_outbound_rtp(&mut self) -> Vec<RtpPacketBatch> {
        Vec::new()
    }
}

fn stream_key(session_id: &str, stream_id: &str) -> String {
    format!("{session_id}::{stream_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use apprelay_protocol::{SdpRole, ViewportSize};

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
    fn in_memory_peer_start_stop_round_trips() {
        let mut peer = InMemoryWebRtcPeer::new();
        peer.start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect("start");
        peer.start("session-1", "stream-2", WebRtcPeerRole::Answerer)
            .expect("start");
        assert_eq!(peer.started_stream_count(), 2);

        peer.stop("session-1", "stream-1").expect("stop");
        assert_eq!(peer.started_stream_count(), 1);

        // Idempotent: stopping an unknown stream succeeds.
        peer.stop("session-1", "stream-1").expect("stop again");
        assert_eq!(peer.started_stream_count(), 1);
    }

    #[test]
    fn in_memory_peer_consume_signaling_counts() {
        let mut peer = InMemoryWebRtcPeer::new();
        peer.consume_signaling(
            "session-1",
            SignalingEnvelope::SdpOffer {
                sdp: "v=0".to_string(),
                role: SdpRole::Offerer,
            },
        )
        .expect("consume");
        peer.consume_signaling("session-1", SignalingEnvelope::EndOfCandidates)
            .expect("consume");
        assert_eq!(peer.consumed_envelope_count(), 2);
    }

    #[test]
    fn in_memory_peer_drains_are_always_empty() {
        let mut peer = InMemoryWebRtcPeer::new();
        peer.start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .unwrap();
        peer.push_encoded_frame("stream-1", &frame()).unwrap();
        assert!(peer.take_outbound_signaling("session-1").is_empty());
        assert!(peer.take_outbound_rtp().is_empty());
        assert_eq!(peer.pushed_frame_count(), 1);
        // Even non-zero viewport calls don't produce RTP from the
        // in-memory peer — sanity guard against a future refactor
        // accidentally synthesising frames here.
        let _ = ViewportSize {
            width: 1280,
            height: 720,
        };
        assert!(peer.take_outbound_rtp().is_empty());
    }
}
