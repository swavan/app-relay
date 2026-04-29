use swavan_protocol::{
    ApplicationSession, Feature, Platform, ReconnectVideoStreamRequest, ResizeSessionRequest,
    SessionState, StartVideoStreamRequest, StopVideoStreamRequest, SwavanError, VideoStreamHealth,
    VideoStreamSession, VideoStreamSignaling, VideoStreamSignalingKind, VideoStreamState,
    VideoStreamStats,
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
    fn reconnect_stream(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError>;
    fn record_resize(&mut self, request: &ResizeSessionRequest);
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
                reconnect_attempts: 0,
            },
            health: VideoStreamHealth {
                healthy: true,
                message: None,
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
        stream.health = VideoStreamHealth {
            healthy: false,
            message: Some("stream stopped by client".to_string()),
        };
        Ok(stream.clone())
    }

    fn reconnect_stream(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| stream.id == request.stream_id)
            .ok_or_else(|| {
                SwavanError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;

        if stream.state == VideoStreamState::Stopped {
            return Err(SwavanError::InvalidRequest(format!(
                "stream {} has been stopped",
                request.stream_id
            )));
        }

        stream.state = VideoStreamState::Starting;
        stream.stats.reconnect_attempts += 1;
        stream.health = VideoStreamHealth {
            healthy: true,
            message: Some("reconnect requested".to_string()),
        };
        Ok(stream.clone())
    }

    fn record_resize(&mut self, request: &ResizeSessionRequest) {
        for stream in self.streams.iter_mut().filter(|stream| {
            stream.session_id == request.session_id
                && stream.state != VideoStreamState::Stopped
                && stream.state != VideoStreamState::Failed
        }) {
            stream.viewport = request.viewport.clone();
            stream.health = VideoStreamHealth {
                healthy: true,
                message: Some("stream viewport updated".to_string()),
            };
        }
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
    fn video_stream_service_reconnects_active_stream() {
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

        let reconnected = stream_service
            .reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: stream.id,
            })
            .expect("reconnect stream");

        assert_eq!(reconnected.state, VideoStreamState::Starting);
        assert_eq!(reconnected.stats.reconnect_attempts, 1);
        assert_eq!(
            reconnected.health.message.as_deref(),
            Some("reconnect requested")
        );
    }

    #[test]
    fn video_stream_service_updates_viewport_on_session_resize() {
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

        stream_service.record_resize(&ResizeSessionRequest {
            session_id: session.id,
            viewport: ViewportSize::new(1440, 900),
        });

        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.viewport, ViewportSize::new(1440, 900));
        assert_eq!(
            status.health.message.as_deref(),
            Some("stream viewport updated")
        );
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
