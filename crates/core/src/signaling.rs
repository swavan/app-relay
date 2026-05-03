//! In-memory store-and-forward queue for SDP/ICE signaling envelopes.
//!
//! Phase C only handles signaling transport; Phase D will plug in a real
//! WebRTC peer that consumes and produces envelopes.

use std::collections::{HashMap, VecDeque};

use apprelay_protocol::{
    AppRelayError, PollSignalingRequest, SignalingDirection, SignalingMessage, SignalingPoll,
    SignalingSubmitAck, SubmitSignalingRequest,
};

/// Maximum number of in-flight signaling envelopes retained per session,
/// summed across both directions. `submit` rejects with
/// `AppRelayError::ServiceUnavailable` once a session reaches this depth, so
/// a misbehaving paired client cannot exhaust server memory by flooding a
/// session it owns. Polling acks (`since_sequence`) drain the queue and free
/// slots, making this a depth cap rather than a lifetime cap.
pub const MAX_ENVELOPES_PER_SESSION: usize = 256;

/// Stable prefix on the `ServiceUnavailable` message produced by
/// `InMemorySignalingService::submit` when the per-session backlog reaches
/// [`MAX_ENVELOPES_PER_SESSION`]. The server wire codec matches on this
/// prefix to convert the typed service error into the stable
/// `ERROR signaling-backlog-full` response line and to emit the matching
/// `ServerEvent::SignalingBacklogFull` audit event.
pub const SIGNALING_BACKLOG_FULL_MESSAGE_PREFIX: &str = "signaling backlog full for session ";

/// Service contract for a per-session signaling queue.
pub trait SignalingService {
    fn submit(
        &mut self,
        request: SubmitSignalingRequest,
    ) -> Result<SignalingSubmitAck, AppRelayError>;

    fn poll(&mut self, request: PollSignalingRequest) -> Result<SignalingPoll, AppRelayError>;

    fn close_session(&mut self, session_id: &str);
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemorySignalingService {
    sessions: HashMap<String, SignalingSessionState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SignalingSessionState {
    next_sequence: u64,
    offer_to_answerer: VecDeque<SignalingMessage>,
    answerer_to_offerer: VecDeque<SignalingMessage>,
}

impl SignalingSessionState {
    fn queue_for(&self, direction: SignalingDirection) -> &VecDeque<SignalingMessage> {
        match direction {
            SignalingDirection::OfferToAnswerer => &self.offer_to_answerer,
            SignalingDirection::AnswererToOfferer => &self.answerer_to_offerer,
        }
    }

    fn queue_for_mut(&mut self, direction: SignalingDirection) -> &mut VecDeque<SignalingMessage> {
        match direction {
            SignalingDirection::OfferToAnswerer => &mut self.offer_to_answerer,
            SignalingDirection::AnswererToOfferer => &mut self.answerer_to_offerer,
        }
    }

    /// Combined depth across both directions, used to enforce
    /// [`MAX_ENVELOPES_PER_SESSION`].
    fn total_depth(&self) -> usize {
        self.offer_to_answerer.len() + self.answerer_to_offerer.len()
    }
}

impl InMemorySignalingService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Combined backlog depth across both directions for `session_id`.
    /// Returns `0` for sessions with no recorded signaling state.
    pub fn current_depth(&self, session_id: &str) -> usize {
        self.sessions
            .get(session_id)
            .map(SignalingSessionState::total_depth)
            .unwrap_or(0)
    }
}

impl SignalingService for InMemorySignalingService {
    fn submit(
        &mut self,
        request: SubmitSignalingRequest,
    ) -> Result<SignalingSubmitAck, AppRelayError> {
        if request.session_id.trim().is_empty() {
            return Err(AppRelayError::InvalidRequest(
                "session id is required".to_string(),
            ));
        }

        let envelope_kind = request.envelope.kind_label().to_string();
        let payload_byte_length =
            u32::try_from(request.envelope.payload_byte_length()).unwrap_or(u32::MAX);

        let state = self.sessions.entry(request.session_id.clone()).or_default();
        if state.total_depth() >= MAX_ENVELOPES_PER_SESSION {
            return Err(AppRelayError::ServiceUnavailable(format!(
                "{SIGNALING_BACKLOG_FULL_MESSAGE_PREFIX}{}",
                request.session_id
            )));
        }
        state.next_sequence = state.next_sequence.saturating_add(1);
        let sequence = state.next_sequence;
        let message = SignalingMessage {
            sequence,
            direction: request.direction,
            envelope: request.envelope,
        };
        state.queue_for_mut(request.direction).push_back(message);

        Ok(SignalingSubmitAck {
            session_id: request.session_id,
            direction: request.direction,
            sequence,
            envelope_kind,
            payload_byte_length,
        })
    }

    fn poll(&mut self, request: PollSignalingRequest) -> Result<SignalingPoll, AppRelayError> {
        if request.session_id.trim().is_empty() {
            return Err(AppRelayError::InvalidRequest(
                "session id is required".to_string(),
            ));
        }

        // Treat `since_sequence` as an ack: drop everything the caller has
        // already seen so the per-session backlog cap measures depth, not
        // lifetime count. Note that draining only affects the polled
        // direction; the opposite direction's queue is untouched.
        if let Some(state) = self.sessions.get_mut(&request.session_id) {
            let queue = state.queue_for_mut(request.direction);
            while queue
                .front()
                .is_some_and(|message| message.sequence <= request.since_sequence)
            {
                queue.pop_front();
            }
        }

        let messages: Vec<SignalingMessage> = self
            .sessions
            .get(&request.session_id)
            .map(|state| state.queue_for(request.direction))
            .map(|queue| {
                queue
                    .iter()
                    .filter(|message| message.sequence > request.since_sequence)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        let last_sequence = messages
            .last()
            .map(|message| message.sequence)
            .unwrap_or(request.since_sequence);

        Ok(SignalingPoll {
            session_id: request.session_id,
            direction: request.direction,
            last_sequence,
            messages,
        })
    }

    fn close_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apprelay_protocol::{IceCandidatePayload, SdpRole, SignalingEnvelope};

    fn submit_offer(service: &mut InMemorySignalingService, session: &str) -> SignalingSubmitAck {
        service
            .submit(SubmitSignalingRequest {
                session_id: session.to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                envelope: SignalingEnvelope::SdpOffer {
                    sdp: "v=0\r\n".to_string(),
                    role: SdpRole::Offerer,
                },
            })
            .expect("submit offer")
    }

    #[test]
    fn submit_returns_monotonic_sequence_per_session() {
        let mut service = InMemorySignalingService::new();
        let first = submit_offer(&mut service, "session-1");
        let second = submit_offer(&mut service, "session-1");
        let other = submit_offer(&mut service, "session-2");

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(other.sequence, 1);
        assert_eq!(first.envelope_kind, "sdp-offer");
        assert_eq!(first.payload_byte_length, 5);
    }

    #[test]
    fn poll_returns_only_messages_after_since_sequence() {
        let mut service = InMemorySignalingService::new();
        submit_offer(&mut service, "session-1");
        service
            .submit(SubmitSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                envelope: SignalingEnvelope::IceCandidate(IceCandidatePayload {
                    candidate: "candidate:foo".to_string(),
                    sdp_mid: "video".to_string(),
                    sdp_mline_index: 0,
                }),
            })
            .expect("submit candidate");
        service
            .submit(SubmitSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                envelope: SignalingEnvelope::EndOfCandidates,
            })
            .expect("submit end-of-candidates");

        let poll = service
            .poll(PollSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                since_sequence: 1,
            })
            .expect("poll envelopes");

        assert_eq!(poll.messages.len(), 2);
        assert_eq!(poll.messages[0].sequence, 2);
        assert_eq!(poll.messages[1].sequence, 3);
        assert_eq!(poll.last_sequence, 3);
        assert!(matches!(
            poll.messages[1].envelope,
            SignalingEnvelope::EndOfCandidates
        ));
    }

    #[test]
    fn poll_isolates_directions() {
        let mut service = InMemorySignalingService::new();
        service
            .submit(SubmitSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                envelope: SignalingEnvelope::SdpOffer {
                    sdp: "offer".to_string(),
                    role: SdpRole::Offerer,
                },
            })
            .expect("submit offer");
        service
            .submit(SubmitSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::AnswererToOfferer,
                envelope: SignalingEnvelope::SdpAnswer {
                    sdp: "answer".to_string(),
                },
            })
            .expect("submit answer");

        let offer_poll = service
            .poll(PollSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                since_sequence: 0,
            })
            .expect("poll offer side");
        let answer_poll = service
            .poll(PollSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::AnswererToOfferer,
                since_sequence: 0,
            })
            .expect("poll answer side");

        assert_eq!(offer_poll.messages.len(), 1);
        assert!(matches!(
            offer_poll.messages[0].envelope,
            SignalingEnvelope::SdpOffer { .. }
        ));
        assert_eq!(answer_poll.messages.len(), 1);
        assert!(matches!(
            answer_poll.messages[0].envelope,
            SignalingEnvelope::SdpAnswer { .. }
        ));
    }

    #[test]
    fn poll_unknown_session_returns_empty_with_since_sequence_preserved() {
        let mut service = InMemorySignalingService::new();
        let poll = service
            .poll(PollSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                since_sequence: 5,
            })
            .expect("poll unknown session");

        assert!(poll.messages.is_empty());
        assert_eq!(poll.last_sequence, 5);
    }

    #[test]
    fn close_session_drops_queued_messages_and_resets_sequence() {
        let mut service = InMemorySignalingService::new();
        submit_offer(&mut service, "session-1");
        service.close_session("session-1");

        let poll = service
            .poll(PollSignalingRequest {
                session_id: "session-1".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                since_sequence: 0,
            })
            .expect("poll closed session");
        assert!(poll.messages.is_empty());

        let resubmit = submit_offer(&mut service, "session-1");
        assert_eq!(
            resubmit.sequence, 1,
            "sequence resets after the session is closed"
        );
    }

    #[test]
    fn submit_rejects_blank_session_id() {
        let mut service = InMemorySignalingService::new();
        let error = service
            .submit(SubmitSignalingRequest {
                session_id: "  ".to_string(),
                direction: SignalingDirection::OfferToAnswerer,
                envelope: SignalingEnvelope::EndOfCandidates,
            })
            .expect_err("blank session id rejected");
        assert!(matches!(error, AppRelayError::InvalidRequest(_)));
    }
}
