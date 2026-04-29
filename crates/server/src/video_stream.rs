use apprelay_core::{InMemoryVideoStreamService, VideoStreamService, WindowCaptureBackendService};
use apprelay_protocol::{
    AppRelayError, ApplicationSession, NegotiateVideoStreamRequest, Platform,
    ReconnectVideoStreamRequest, ResizeSessionRequest, StartVideoStreamRequest,
    StopVideoStreamRequest, VideoStreamSession,
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

    pub fn for_platform(platform: Platform) -> Self {
        let capture_backend = match platform {
            Platform::Linux => WindowCaptureBackendService::LinuxSelectedWindow,
            Platform::Android
            | Platform::Ios
            | Platform::Macos
            | Platform::Windows
            | Platform::Unknown => WindowCaptureBackendService::Unsupported { platform },
        };

        Self {
            stream_service: InMemoryVideoStreamService::new(capture_backend),
        }
    }

    pub fn start(
        &mut self,
        request: StartVideoStreamRequest,
        sessions: &[ApplicationSession],
    ) -> Result<VideoStreamSession, AppRelayError> {
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
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.stream_service.stop_stream(request)
    }

    pub fn reconnect(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.stream_service.reconnect_stream(request)
    }

    pub fn negotiate(
        &mut self,
        request: NegotiateVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.stream_service.negotiate_stream(request)
    }

    pub fn record_resize(&mut self, request: &ResizeSessionRequest) {
        self.stream_service.record_resize(request);
    }

    pub fn status(&self, stream_id: &str) -> Result<VideoStreamSession, AppRelayError> {
        self.stream_service.stream_status(stream_id)
    }
}

impl Default for VideoStreamControl {
    fn default() -> Self {
        Self::new()
    }
}
