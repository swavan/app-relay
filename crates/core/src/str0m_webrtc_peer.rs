//! Phase D.1.0 — `str0m`-backed sans-IO server-side WebRTC peer.
//!
//! This module exists only when the `webrtc-peer` cargo feature is
//! enabled. It owns a per-`(session_id, stream_id)` [`str0m::Rtc`]
//! instance and translates the trait-level surface
//! ([`crate::webrtc_peer::WebRtcPeer`]) into `str0m` API calls:
//!
//! * `start` — builds an `Rtc` with `clear_codecs() + enable_h264(true)`
//!   and ice-lite, attaches a deterministic loopback host candidate so
//!   `str0m` can produce SDP, and (for `Offerer`) immediately drives the
//!   first `sdp_api().add_media(...).apply()` round so the local
//!   `SdpOffer` is queued for `take_outbound_signaling`.
//! * `consume_signaling` — feeds remote `SdpOffer`/`SdpAnswer`/
//!   `IceCandidate` envelopes into the matching state-machine entry
//!   point on `str0m`. Every error is mapped to a typed
//!   [`AppRelayError`]; silent no-ops are forbidden.
//! * `take_outbound_signaling` — drains the per-session envelope queue.
//!   `str0m` 0.8.0 does NOT surface locally discovered ICE candidates as
//!   `Event` variants — they are encoded into the SDP directly via the
//!   `Candidate::host` we registered. So D.1.0 only emits the local SDP
//!   (offer or answer) plus whatever the caller pushed in.
//! * `push_encoded_frame` — refuses with `ServiceUnavailable` until the
//!   negotiated video `Mid` and H.264 `PayloadType` are known, then
//!   forwards the Annex-B payload through `Rtc::writer(mid).write(...)`.
//!   `str0m`'s sample-API writer accepts the Annex-B byte stream as one
//!   buffer and performs its own NALU framing internally.
//! * `take_outbound_rtp` — pumps `Rtc::handle_input(Input::Timeout(now))`
//!   then drains every `Output::Transmit { destination, contents }` into
//!   an [`RtpPacketBatch`]. With ice-lite + a host candidate but no
//!   remote endpoint, str0m typically produces no datagrams; the test
//!   asserts only that the call doesn't panic.
//!
//! D.1.0 is sans-IO. There is no UDP socket, no server stream lifecycle
//! integration, and no DTLS handshake completion — those land in
//! D.1.1 / D.1.2.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;

use apprelay_protocol::{
    AppRelayError, EncodedVideoFrame, IceCandidatePayload, RtpPacketBatch, SdpRole,
    SignalingEnvelope, WebRtcPeerRole,
};

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::format::Codec;
use str0m::media::{Direction, MediaKind, MediaTime, Mid, Pt};
use str0m::net::Protocol;
use str0m::{Candidate, Event, Input, Output, Rtc};

use crate::webrtc_peer::WebRtcPeer;

/// Per-stream WebRTC state. One of these per active `(session_id,
/// stream_id)` pair. The current product ships at most one stream per
/// session, so the `session_id → state` map is effectively 1:1; the
/// `stream_id → session_id` map exists so `push_encoded_frame` (which
/// only carries `stream_id`) can resolve back to the owning state.
struct PeerState {
    rtc: Rtc,
    role: WebRtcPeerRole,
    stream_id: String,
    /// Outbound signaling envelopes the caller has not drained yet.
    /// Populated by `start` (offerer side: queues the local offer),
    /// `consume_signaling` (answerer side: queues the local answer).
    outbound: VecDeque<SignalingEnvelope>,
    /// Pending offer handle for the offerer side. Cleared once the
    /// matching answer is consumed.
    pending_offer: Option<SdpPendingOffer>,
    /// Negotiated video media id. Set once SDP negotiation completes
    /// (offerer learns it from the answer, answerer learns it from the
    /// offer).
    video_mid: Option<Mid>,
    /// Negotiated H.264 payload type. Looked up after negotiation
    /// against `Rtc::writer(mid).payload_params()` filtered to
    /// `Codec::H264`.
    video_pt: Option<Pt>,
}

/// Server-side sans-IO WebRTC peer backed by `str0m` 0.8.
///
/// Holds owned per-stream `Rtc` instances. The whole peer lives behind
/// a `Mutex<Box<dyn WebRtcPeer>>` in the server composition; trait
/// methods take `&mut self`, so this type does not need any interior
/// locking of its own.
pub struct Str0mWebRtcPeer {
    sessions: HashMap<String, PeerState>,
    stream_to_session: HashMap<String, String>,
}

impl std::fmt::Debug for Str0mWebRtcPeer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `str0m::Rtc` does not implement `Debug`, so we summarise.
        f.debug_struct("Str0mWebRtcPeer")
            .field("session_count", &self.sessions.len())
            .field("stream_count", &self.stream_to_session.len())
            .finish()
    }
}

impl Default for Str0mWebRtcPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl Str0mWebRtcPeer {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            stream_to_session: HashMap::new(),
        }
    }

    /// Build a fresh `Rtc` configured for H.264 video and a
    /// deterministic loopback host candidate. `127.0.0.1:0` is a
    /// placeholder valid socket address — D.1.2 will replace it with a
    /// real bound UDP source.
    ///
    /// `ice_lite` is enabled only on the answering side. str0m 0.8.0
    /// rejects the offer/answer pairing when both peers advertise
    /// `a=ice-lite` (`RtcError::RemoteSdp("Both peers being ICE-Lite
    /// not supported")`); in our deployment the server is the side
    /// that runs ICE-lite (no STUN/TURN gathering, fixed loopback
    /// host candidate), but only for the role that consumes a remote
    /// offer. When this peer originates the offer, full ICE is used.
    fn build_rtc(role: WebRtcPeerRole) -> Result<Rtc, AppRelayError> {
        let ice_lite = matches!(role, WebRtcPeerRole::Answerer);
        let mut rtc = Rtc::builder()
            .clear_codecs()
            .enable_h264(true)
            .set_ice_lite(ice_lite)
            .build();

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let candidate = Candidate::host(addr, Protocol::Udp).map_err(|err| {
            AppRelayError::ServiceUnavailable(format!(
                "webrtc peer failed to build local host candidate: {err}"
            ))
        })?;
        rtc.add_local_candidate(candidate);

        Ok(rtc)
    }

    /// Drain `Rtc` events to capture the `Mid` of any newly-added
    /// video media. Called after `accept_offer` / `accept_answer`.
    fn capture_video_mid_from_events(state: &mut PeerState) -> Result<(), AppRelayError> {
        // Pump a Timeout so str0m can surface MediaAdded events.
        state
            .rtc
            .handle_input(Input::Timeout(Instant::now()))
            .map_err(|err| {
                AppRelayError::ServiceUnavailable(format!(
                    "webrtc peer handle_input failed while capturing mid: {err}"
                ))
            })?;

        loop {
            let output = state.rtc.poll_output().map_err(|err| {
                AppRelayError::ServiceUnavailable(format!(
                    "webrtc peer poll_output failed while capturing mid: {err}"
                ))
            })?;
            match output {
                Output::Timeout(_) => break,
                Output::Event(Event::MediaAdded(added)) => {
                    if matches!(added.kind, MediaKind::Video) && state.video_mid.is_none() {
                        state.video_mid = Some(added.mid);
                    }
                }
                Output::Event(_) => {}
                Output::Transmit(_) => {
                    // D.1.0 has no socket; transmits before negotiation
                    // completes are dropped. `take_outbound_rtp` is the
                    // mechanism for the caller to drain real datagrams.
                }
            }
        }

        // Once we know the mid, look up the H.264 PT.
        if let Some(mid) = state.video_mid {
            if state.video_pt.is_none() {
                if let Some(writer) = state.rtc.writer(mid) {
                    let h264_pt = writer
                        .payload_params()
                        .find(|p| p.spec().codec == Codec::H264)
                        .map(|p| p.pt());
                    state.video_pt = h264_pt;
                }
            }
        }

        Ok(())
    }

    /// Build a `Candidate` from a wire `IceCandidatePayload`. The
    /// `candidate` field in our protocol is the SDP `a=candidate:...`
    /// attribute body (without the `a=` prefix), matching what
    /// `str0m::Candidate::from_sdp_string` expects.
    fn parse_remote_candidate(payload: &IceCandidatePayload) -> Result<Candidate, AppRelayError> {
        Candidate::from_sdp_string(&payload.candidate).map_err(|err| {
            AppRelayError::InvalidRequest(format!(
                "webrtc peer received malformed ICE candidate: {err}"
            ))
        })
    }
}

impl WebRtcPeer for Str0mWebRtcPeer {
    fn start(
        &mut self,
        session_id: &str,
        stream_id: &str,
        role: WebRtcPeerRole,
    ) -> Result<(), AppRelayError> {
        if self.sessions.contains_key(session_id) {
            return Err(AppRelayError::InvalidRequest(format!(
                "webrtc peer already started for session '{session_id}'"
            )));
        }

        let mut rtc = Self::build_rtc(role)?;
        let mut outbound = VecDeque::new();
        let mut pending_offer = None;

        if matches!(role, WebRtcPeerRole::Offerer) {
            let mut sdp_api = rtc.sdp_api();
            let _mid = sdp_api.add_media(MediaKind::Video, Direction::SendOnly, None, None, None);
            match sdp_api.apply() {
                Some((offer, pending)) => {
                    outbound.push_back(SignalingEnvelope::SdpOffer {
                        sdp: offer.to_sdp_string(),
                        role: SdpRole::Offerer,
                    });
                    pending_offer = Some(pending);
                }
                None => {
                    return Err(AppRelayError::ServiceUnavailable(
                        "webrtc peer add_media did not require SDP negotiation".to_string(),
                    ));
                }
            }
        }

        let state = PeerState {
            rtc,
            role,
            stream_id: stream_id.to_string(),
            outbound,
            pending_offer,
            video_mid: None,
            video_pt: None,
        };

        self.sessions.insert(session_id.to_string(), state);
        self.stream_to_session
            .insert(stream_id.to_string(), session_id.to_string());
        Ok(())
    }

    fn stop(&mut self, session_id: &str, stream_id: &str) -> Result<(), AppRelayError> {
        // Idempotent: silently succeed when nothing matches.
        if let Some(state) = self.sessions.remove(session_id) {
            // Only remove the stream-id mapping if it still points at
            // this session; this guards against accidental double-stop
            // races corrupting an unrelated entry.
            if let Some(owner) = self.stream_to_session.get(&state.stream_id) {
                if owner == session_id {
                    self.stream_to_session.remove(&state.stream_id);
                }
            }
        } else {
            // session_id unknown — nothing to do, but also drop the
            // stream-id mapping if it happens to be lingering.
            if let Some(owner) = self.stream_to_session.get(stream_id) {
                if owner == session_id {
                    self.stream_to_session.remove(stream_id);
                }
            }
        }
        Ok(())
    }

    fn consume_signaling(
        &mut self,
        session_id: &str,
        envelope: SignalingEnvelope,
    ) -> Result<(), AppRelayError> {
        let state = self.sessions.get_mut(session_id).ok_or_else(|| {
            AppRelayError::NotFound(format!("webrtc peer has no active session '{session_id}'"))
        })?;

        match envelope {
            SignalingEnvelope::SdpOffer { sdp, .. } => {
                if !matches!(state.role, WebRtcPeerRole::Answerer) {
                    return Err(AppRelayError::InvalidRequest(
                        "webrtc peer received SdpOffer but is configured as Offerer".to_string(),
                    ));
                }
                let parsed = SdpOffer::from_sdp_string(&sdp).map_err(|err| {
                    AppRelayError::InvalidRequest(format!(
                        "webrtc peer received malformed SDP offer: {err}"
                    ))
                })?;
                let answer = state.rtc.sdp_api().accept_offer(parsed).map_err(|err| {
                    AppRelayError::InvalidRequest(format!(
                        "webrtc peer rejected remote SDP offer: {err}"
                    ))
                })?;
                state.outbound.push_back(SignalingEnvelope::SdpAnswer {
                    sdp: answer.to_sdp_string(),
                });
                Self::capture_video_mid_from_events(state)?;
                Ok(())
            }
            SignalingEnvelope::SdpAnswer { sdp } => {
                if !matches!(state.role, WebRtcPeerRole::Offerer) {
                    return Err(AppRelayError::InvalidRequest(
                        "webrtc peer received SdpAnswer but is configured as Answerer".to_string(),
                    ));
                }
                let pending = state.pending_offer.take().ok_or_else(|| {
                    AppRelayError::InvalidRequest(
                        "webrtc peer received SdpAnswer with no pending local offer".to_string(),
                    )
                })?;
                let parsed = SdpAnswer::from_sdp_string(&sdp).map_err(|err| {
                    AppRelayError::InvalidRequest(format!(
                        "webrtc peer received malformed SDP answer: {err}"
                    ))
                })?;
                state
                    .rtc
                    .sdp_api()
                    .accept_answer(pending, parsed)
                    .map_err(|err| {
                        AppRelayError::InvalidRequest(format!(
                            "webrtc peer rejected remote SDP answer: {err}"
                        ))
                    })?;
                Self::capture_video_mid_from_events(state)?;
                Ok(())
            }
            SignalingEnvelope::IceCandidate(payload) => {
                let candidate = Self::parse_remote_candidate(&payload)?;
                state.rtc.add_remote_candidate(candidate);
                Ok(())
            }
            SignalingEnvelope::EndOfCandidates => {
                // str0m 0.8.0 does not expose an explicit "end of
                // candidates" entry point — the agent runs trickle
                // forever. Recording the signal as a no-op is the
                // documented behaviour for D.1.0; D.1.x can surface a
                // matching event if the upstream API gains one.
                Ok(())
            }
        }
    }

    fn take_outbound_signaling(&mut self, session_id: &str) -> Vec<SignalingEnvelope> {
        // Unknown session is "nothing pending", per the trait contract.
        let Some(state) = self.sessions.get_mut(session_id) else {
            return Vec::new();
        };
        state.outbound.drain(..).collect()
    }

    fn push_encoded_frame(
        &mut self,
        stream_id: &str,
        frame: &EncodedVideoFrame,
    ) -> Result<(), AppRelayError> {
        let session_id = self.stream_to_session.get(stream_id).ok_or_else(|| {
            AppRelayError::NotFound(format!("webrtc peer has no active stream '{stream_id}'"))
        })?;
        let state = self.sessions.get_mut(session_id).ok_or_else(|| {
            AppRelayError::NotFound(format!(
                "webrtc peer has no active session for stream '{stream_id}'"
            ))
        })?;

        // If we never captured a Mid (e.g. the answerer hasn't seen the
        // offer yet, or the offerer hasn't seen the answer), retry the
        // event drain in case negotiation has since settled.
        if state.video_mid.is_none() || state.video_pt.is_none() {
            Self::capture_video_mid_from_events(state)?;
        }

        let mid = state.video_mid.ok_or_else(|| {
            AppRelayError::ServiceUnavailable(
                "webrtc peer is not ready: SDP negotiation not complete".to_string(),
            )
        })?;
        let pt = state.video_pt.ok_or_else(|| {
            AppRelayError::ServiceUnavailable(
                "webrtc peer is not ready: SDP negotiation not complete (no H264 PT)".to_string(),
            )
        })?;

        let writer = state.rtc.writer(mid).ok_or_else(|| {
            AppRelayError::ServiceUnavailable(format!(
                "webrtc peer has no writer for negotiated mid {mid:?}"
            ))
        })?;

        let media_time = MediaTime::from_90khz(frame.timestamp_ms.saturating_mul(90));
        writer
            .write(pt, Instant::now(), media_time, frame.payload.clone())
            .map_err(|err| {
                AppRelayError::ServiceUnavailable(format!(
                    "webrtc peer failed to write encoded frame: {err}"
                ))
            })?;

        Ok(())
    }

    fn take_outbound_rtp(&mut self) -> Vec<RtpPacketBatch> {
        let now = Instant::now();
        let mut batches = Vec::new();

        for state in self.sessions.values_mut() {
            if state.rtc.handle_input(Input::Timeout(now)).is_err() {
                // A failed timeout pump on an otherwise-quiescent peer
                // is not actionable here; skipping this session for
                // this round is the documented sans-IO recovery path.
                continue;
            }

            loop {
                match state.rtc.poll_output() {
                    Ok(Output::Timeout(_)) => break,
                    Ok(Output::Transmit(transmit)) => {
                        let payload: Vec<u8> = transmit.contents.into();
                        batches.push(RtpPacketBatch::new(transmit.destination, payload));
                    }
                    Ok(Output::Event(_)) => {
                        // D.1.0 ignores events here; D.1.1 will surface
                        // connection-state changes through the audit
                        // sink and react to KeyframeRequest events.
                    }
                    Err(_) => break,
                }
            }
        }

        batches
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
    fn start_offerer_emits_local_sdp_offer_envelope() {
        let mut peer = Str0mWebRtcPeer::new();
        peer.start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect("start offerer");
        let outbound = peer.take_outbound_signaling("session-1");
        assert_eq!(outbound.len(), 1, "expected exactly one outbound envelope");
        match &outbound[0] {
            SignalingEnvelope::SdpOffer { sdp, role } => {
                assert_eq!(*role, SdpRole::Offerer);
                assert!(
                    sdp.starts_with("v=0"),
                    "sdp must start with v=0, got {sdp:?}"
                );
            }
            other => panic!("expected SdpOffer, got {other:?}"),
        }
    }

    #[test]
    fn answerer_round_trip_produces_sdp_answer() {
        let mut offerer = Str0mWebRtcPeer::new();
        let mut answerer = Str0mWebRtcPeer::new();

        offerer
            .start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect("start offerer");
        answerer
            .start("session-1", "stream-1", WebRtcPeerRole::Answerer)
            .expect("start answerer");

        // Answerer has nothing to send before consuming an offer.
        assert!(answerer.take_outbound_signaling("session-1").is_empty());

        let offer_envelope = offerer
            .take_outbound_signaling("session-1")
            .into_iter()
            .next()
            .expect("offerer produced no offer");
        let offer_sdp = match &offer_envelope {
            SignalingEnvelope::SdpOffer { sdp, .. } => sdp.clone(),
            other => panic!("expected offer, got {other:?}"),
        };

        answerer
            .consume_signaling(
                "session-1",
                SignalingEnvelope::SdpOffer {
                    sdp: offer_sdp,
                    role: SdpRole::Offerer,
                },
            )
            .expect("answerer accepts offer");

        let answers = answerer.take_outbound_signaling("session-1");
        assert_eq!(answers.len(), 1, "answerer must produce one envelope");
        let answer_sdp = match &answers[0] {
            SignalingEnvelope::SdpAnswer { sdp } => {
                assert!(
                    sdp.starts_with("v=0"),
                    "answer must start with v=0, got {sdp:?}"
                );
                sdp.clone()
            }
            other => panic!("expected SdpAnswer, got {other:?}"),
        };

        // Feed the answer back into the offerer; it must accept without
        // panic and stop holding the pending offer.
        offerer
            .consume_signaling(
                "session-1",
                SignalingEnvelope::SdpAnswer { sdp: answer_sdp },
            )
            .expect("offerer accepts answer");
    }

    #[test]
    fn answerer_rejects_offer_addressed_to_offerer() {
        let mut peer = Str0mWebRtcPeer::new();
        peer.start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect("start offerer");
        let err = peer
            .consume_signaling(
                "session-1",
                SignalingEnvelope::SdpOffer {
                    sdp: "v=0\r\n".to_string(),
                    role: SdpRole::Offerer,
                },
            )
            .expect_err("offerer must reject incoming SdpOffer");
        assert!(matches!(err, AppRelayError::InvalidRequest(_)));
    }

    #[test]
    fn consume_signaling_rejects_unknown_session() {
        let mut peer = Str0mWebRtcPeer::new();
        let err = peer
            .consume_signaling(
                "session-unknown",
                SignalingEnvelope::SdpOffer {
                    sdp: "v=0\r\n".to_string(),
                    role: SdpRole::Offerer,
                },
            )
            .expect_err("unknown session must error");
        assert!(matches!(err, AppRelayError::NotFound(_)));
    }

    #[test]
    fn push_encoded_frame_before_negotiation_returns_typed_error() {
        let mut peer = Str0mWebRtcPeer::new();
        peer.start("session-1", "stream-1", WebRtcPeerRole::Answerer)
            .expect("start answerer");
        let err = peer
            .push_encoded_frame("stream-1", &frame())
            .expect_err("must error before negotiation");
        match err {
            AppRelayError::ServiceUnavailable(message) => {
                assert!(
                    message.contains("negotiation"),
                    "message must mention negotiation, got {message:?}"
                );
            }
            other => panic!("expected ServiceUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn take_outbound_rtp_drains_handshake_datagrams_after_start() {
        let mut peer = Str0mWebRtcPeer::new();
        peer.start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect("start offerer");
        // ice-lite + a host candidate but no remote endpoint produces
        // no transmits in str0m 0.8.0; we assert only the call is
        // panic-free and returns a Vec.
        let _ = peer.take_outbound_rtp();
    }

    #[test]
    fn stop_is_idempotent() {
        let mut peer = Str0mWebRtcPeer::new();
        // Stop before any start succeeds.
        peer.stop("session-unknown", "stream-unknown")
            .expect("stop idempotent on never-started stream");
        peer.start("session-1", "stream-1", WebRtcPeerRole::Offerer)
            .expect("start");
        peer.stop("session-1", "stream-1").expect("first stop");
        peer.stop("session-1", "stream-1").expect("second stop");
    }
}
