use swavan_protocol::{
    ApplicationSession, Feature, Platform, SessionState, StartVideoStreamRequest,
    StopVideoStreamRequest, SwavanError, VideoStreamSession, VideoStreamSignaling,
    VideoStreamSignalingKind, VideoStreamState, VideoStreamStats,
};

pub trait VideoStreamService {
    fn start_stream(
        &mut self,
        request: StartVideoStreamRequest,
        session: &ApplicationSession,
    ) -> Result<VideoStreamSession, SwavanError>;
    fn stop_stream(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError>;
    fn stream_status(&self, stream_id: &str) -> Result<VideoStreamSession, SwavanError>;
}

pub trait WindowCaptureBackend {
    fn start_capture(
        &self,
        stream_id: &str,
        session: &ApplicationSession,
    ) -> Result<VideoStreamSignaling, SwavanError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowCaptureBackendService {
    RecordOnly,
    Unsupported { platform: Platform },
}

impl WindowCaptureBackend for WindowCaptureBackendService {
    fn start_capture(
        &self,
        stream_id: &str,
        session: &ApplicationSession,
    ) -> Result<VideoStreamSignaling, SwavanError> {
        match self {
            Self::RecordOnly => Ok(VideoStreamSignaling {
                kind: VideoStreamSignalingKind::WebRtcOffer,
                offer: Some(format!(
                    "swavan-webrtc-offer:{stream_id}:{}",
                    session.selected_window.id
                )),
            }),
            Self::Unsupported { platform } => Err(SwavanError::unsupported(
                *platform,
                Feature::WindowVideoStream,
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryVideoStreamService {
    capture_backend: WindowCaptureBackendService,
    streams: Vec<VideoStreamSession>,
    next_stream_sequence: u64,
}

impl InMemoryVideoStreamService {
    pub fn new(capture_backend: WindowCaptureBackendService) -> Self {
        Self {
            capture_backend,
            streams: Vec::new(),
            next_stream_sequence: 1,
        }
    }

    fn next_stream_id(&mut self) -> String {
        let id = format!("stream-{}", self.next_stream_sequence);
        self.next_stream_sequence += 1;
        id
    }
}

impl Default for InMemoryVideoStreamService {
    fn default() -> Self {
        Self::new(WindowCaptureBackendService::RecordOnly)
    }
}

impl VideoStreamService for InMemoryVideoStreamService {
    fn start_stream(
        &mut self,
        request: StartVideoStreamRequest,
        session: &ApplicationSession,
    ) -> Result<VideoStreamSession, SwavanError> {
        if session.id != request.session_id || session.state == SessionState::Closed {
            return Err(SwavanError::NotFound(format!(
                "session {} was not found",
                request.session_id
            )));
        }

        let stream_id = self.next_stream_id();
        let signaling = self.capture_backend.start_capture(&stream_id, session)?;
        let stream = VideoStreamSession {
            id: stream_id,
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            viewport: session.viewport.clone(),
            signaling,
            stats: VideoStreamStats {
                frames_encoded: 0,
                bitrate_kbps: 0,
                latency_ms: 0,
            },
            state: VideoStreamState::Starting,
            failure_reason: None,
        };

        self.streams.push(stream.clone());
        Ok(stream)
    }

    fn stop_stream(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| {
                stream.id == request.stream_id && stream.state != VideoStreamState::Stopped
            })
            .ok_or_else(|| {
                SwavanError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;

        stream.state = VideoStreamState::Stopped;
        Ok(stream.clone())
    }

    fn stream_status(&self, stream_id: &str) -> Result<VideoStreamSession, SwavanError> {
        self.streams
            .iter()
            .find(|stream| stream.id == stream_id)
            .cloned()
            .ok_or_else(|| SwavanError::NotFound(format!("stream {stream_id} was not found")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApplicationSessionService, InMemoryApplicationSessionService};
    use swavan_protocol::{CreateSessionRequest, ViewportSize};

    #[test]
    fn video_stream_service_starts_and_stops_selected_window_stream() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryVideoStreamService::default();

        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        assert_eq!(stream.id, "stream-1");
        assert_eq!(stream.session_id, session.id);
        assert_eq!(stream.selected_window_id, session.selected_window.id);
        assert_eq!(stream.viewport, ViewportSize::new(1280, 720));
        assert_eq!(stream.state, VideoStreamState::Starting);
        assert_eq!(stream.signaling.kind, VideoStreamSignalingKind::WebRtcOffer);
        assert_eq!(
            stream_service
                .stream_status("stream-1")
                .expect("stream status"),
            stream
        );

        let stopped = stream_service
            .stop_stream(StopVideoStreamRequest {
                stream_id: "stream-1".to_string(),
            })
            .expect("stop stream");

        assert_eq!(stopped.state, VideoStreamState::Stopped);
    }

    #[test]
    fn video_stream_service_reports_unsupported_capture_backend() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service =
            InMemoryVideoStreamService::new(WindowCaptureBackendService::Unsupported {
                platform: Platform::Linux,
            });

        assert_eq!(
            stream_service.start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            ),
            Err(SwavanError::unsupported(
                Platform::Linux,
                Feature::WindowVideoStream
            ))
        );
    }
}
