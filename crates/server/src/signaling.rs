//! Server composition for the SDP/ICE signaling queue.

use apprelay_core::{InMemorySignalingService, SignalingService};
use apprelay_protocol::{
    AppRelayError, PollSignalingRequest, SignalingPoll, SignalingSubmitAck, SubmitSignalingRequest,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SignalingControl {
    service: InMemorySignalingService,
}

impl SignalingControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(
        &mut self,
        request: SubmitSignalingRequest,
    ) -> Result<SignalingSubmitAck, AppRelayError> {
        self.service.submit(request)
    }

    pub fn poll(&mut self, request: PollSignalingRequest) -> Result<SignalingPoll, AppRelayError> {
        self.service.poll(request)
    }

    pub fn record_session_closed(&mut self, session_id: &str) {
        self.service.close_session(session_id);
    }

    /// Combined backlog depth (both directions) for `session_id`. Used by
    /// the foreground wire codec to populate the
    /// `ServerEvent::SignalingBacklogFull` audit event after a backlog-full
    /// rejection.
    pub fn current_depth(&self, session_id: &str) -> usize {
        self.service.current_depth(session_id)
    }
}
