use swavan_core::{InMemoryVideoStreamService, VideoStreamService};
use swavan_protocol::{
    ApplicationSession, ReconnectVideoStreamRequest, ResizeSessionRequest, StartVideoStreamRequest,
    StopVideoStreamRequest, SwavanError, VideoStreamSession,
};

#[derive(Debug)]
pub struct VideoStreamControl {
    stream_service: InMemoryVideoStreamService,
}

impl VideoStreamControl {
    pub fn new() -> Self {
        Self {
            stream_service: InMemoryVideoStreamService::default(),
        }
    }

    pub fn start(
        &mut self,
        request: StartVideoStreamRequest,
        sessions: &[ApplicationSession],
    ) -> Result<VideoStreamSession, SwavanError> {
        let session = sessions
            .iter()
            .find(|session| session.id == request.session_id)
            .ok_or_else(|| {
                SwavanError::NotFound(format!("session {} was not found", request.session_id))
            })?;

        self.stream_service.start_stream(request, session)
    }

    pub fn stop(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError> {
        self.stream_service.stop_stream(request)
    }

    pub fn reconnect(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError> {
        self.stream_service.reconnect_stream(request)
    }

    pub fn record_resize(&mut self, request: &ResizeSessionRequest) {
        self.stream_service.record_resize(request);
    }

    pub fn status(&self, stream_id: &str) -> Result<VideoStreamSession, SwavanError> {
        self.stream_service.stream_status(stream_id)
    }
}

impl Default for VideoStreamControl {
    fn default() -> Self {
        Self::new()
    }
}
