use apprelay_core::{AudioBackendService, AudioStreamService, InMemoryAudioStreamService};
use apprelay_protocol::{
    AppRelayError, ApplicationSession, AudioStreamSession, Platform, StartAudioStreamRequest,
    StopAudioStreamRequest, UpdateAudioStreamRequest,
};

#[derive(Debug)]
pub struct AudioStreamControl {
    stream_service: InMemoryAudioStreamService,
}

impl AudioStreamControl {
    pub fn new() -> Self {
        Self {
            stream_service: InMemoryAudioStreamService::default(),
        }
    }

    pub fn for_platform(platform: Platform) -> Self {
        Self {
            stream_service: InMemoryAudioStreamService::new(AudioBackendService::for_platform(
                platform,
            )),
        }
    }

    pub fn start(
        &mut self,
        request: StartAudioStreamRequest,
        sessions: &[ApplicationSession],
    ) -> Result<AudioStreamSession, AppRelayError> {
        let session = sessions
            .iter()
            .find(|session| session.id == request.session_id)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("session {} was not found", request.session_id))
            })?;

        self.stream_service.start_stream(request, session)
    }

    pub fn stop(
        &mut self,
        request: StopAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.stream_service.stop_stream(request)
    }

    pub fn update(
        &mut self,
        request: UpdateAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.stream_service.update_stream(request)
    }

    pub fn record_session_closed(&mut self, session_id: &str) {
        self.stream_service.record_session_closed(session_id);
    }

    pub fn status(&self, stream_id: &str) -> Result<AudioStreamSession, AppRelayError> {
        self.stream_service.stream_status(stream_id)
    }
}

impl Default for AudioStreamControl {
    fn default() -> Self {
        Self::new()
    }
}
