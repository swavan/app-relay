use apprelay_protocol::{
    AppRelayError, ApplicationSession, Feature, NegotiateVideoStreamRequest, Platform,
    ReconnectVideoStreamRequest, ResizeSessionRequest, SessionState, StartVideoStreamRequest,
    StopVideoStreamRequest, VideoCaptureScope, VideoCaptureSource, VideoCodec,
    VideoEncodingContract, VideoEncodingOutput, VideoEncodingPipeline, VideoEncodingPipelineState,
    VideoEncodingTarget, VideoHardwareAcceleration, VideoPixelFormat, VideoResolutionAdaptation,
    VideoResolutionAdaptationReason, VideoResolutionLimits, VideoStreamHealth,
    VideoStreamNegotiationState, VideoStreamSession, VideoStreamSignaling,
    VideoStreamSignalingKind, VideoStreamState, VideoStreamStats, WebRtcIceCandidate,
    WebRtcSdpType, WebRtcSessionDescription,
};

pub trait VideoStreamService {
    fn start_stream(
        &mut self,
        request: StartVideoStreamRequest,
        session: &ApplicationSession,
    ) -> Result<VideoStreamSession, AppRelayError>;
    fn stop_stream(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError>;
    fn reconnect_stream(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError>;
    fn negotiate_stream(
        &mut self,
        request: NegotiateVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError>;
    fn record_resize(&mut self, request: &ResizeSessionRequest);
    fn stream_status(&self, stream_id: &str) -> Result<VideoStreamSession, AppRelayError>;
}

pub trait WindowCaptureBackend {
    fn start_capture(
        &self,
        stream_id: &str,
        session: &ApplicationSession,
    ) -> Result<WindowCaptureStart, AppRelayError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowCaptureStart {
    pub source: VideoCaptureSource,
    pub signaling: VideoStreamSignaling,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryVideoEncodingPipeline;

impl InMemoryVideoEncodingPipeline {
    const MAX_FPS: u32 = 30;
    const KEYFRAME_INTERVAL_FRAMES: u32 = 60;
    const MIN_DIMENSION: u32 = 2;
    const MAX_WIDTH: u32 = 1920;
    const MAX_HEIGHT: u32 = 1080;
    const MAX_PIXELS: u32 = Self::MAX_WIDTH * Self::MAX_HEIGHT;

    pub fn configured(viewport: apprelay_protocol::ViewportSize) -> VideoEncodingPipeline {
        let adaptation = Self::adapt_resolution(viewport);

        VideoEncodingPipeline {
            contract: VideoEncodingContract {
                codec: VideoCodec::H264,
                pixel_format: VideoPixelFormat::Rgba,
                hardware_acceleration: VideoHardwareAcceleration::None,
                target: VideoEncodingTarget {
                    target_bitrate_kbps: Self::target_bitrate_kbps(&adaptation.current_target),
                    resolution: adaptation.current_target.clone(),
                    max_fps: Self::MAX_FPS,
                    keyframe_interval_frames: Self::KEYFRAME_INTERVAL_FRAMES,
                },
                adaptation,
            },
            state: VideoEncodingPipelineState::Configured,
            output: VideoEncodingOutput {
                frames_submitted: 0,
                frames_encoded: 0,
                keyframes_encoded: 0,
                bytes_produced: 0,
                last_frame: None,
            },
        }
    }

    pub fn encode_next_frame(pipeline: &mut VideoEncodingPipeline) {
        let next_sequence = pipeline.output.frames_encoded + 1;
        let keyframe = next_sequence == 1
            || (next_sequence - 1)
                .is_multiple_of(u64::from(pipeline.contract.target.keyframe_interval_frames));
        let byte_length = Self::encoded_frame_bytes(
            &pipeline.contract.target.resolution,
            pipeline.contract.target.target_bitrate_kbps,
            keyframe,
        );

        pipeline.state = VideoEncodingPipelineState::Encoding;
        pipeline.output.frames_submitted += 1;
        pipeline.output.frames_encoded = next_sequence;
        pipeline.output.keyframes_encoded += if keyframe { 1 } else { 0 };
        pipeline.output.bytes_produced += u64::from(byte_length);
        pipeline.output.last_frame = Some(apprelay_protocol::EncodedVideoFrame {
            sequence: next_sequence,
            timestamp_ms: (next_sequence - 1) * 1_000 / u64::from(Self::MAX_FPS),
            byte_length,
            keyframe,
        });
    }

    pub fn reconfigure(
        pipeline: &mut VideoEncodingPipeline,
        viewport: apprelay_protocol::ViewportSize,
        preserve_encoding_state: bool,
    ) {
        let state =
            if preserve_encoding_state && pipeline.state == VideoEncodingPipelineState::Encoding {
                VideoEncodingPipelineState::Encoding
            } else {
                VideoEncodingPipelineState::Configured
            };

        let adaptation = Self::adapt_resolution(viewport);
        pipeline.contract.target.resolution = adaptation.current_target.clone();
        pipeline.contract.target.target_bitrate_kbps =
            Self::target_bitrate_kbps(&pipeline.contract.target.resolution);
        pipeline.contract.adaptation = adaptation;
        pipeline.state = state;
    }

    pub fn reset_for_reconnect(pipeline: &mut VideoEncodingPipeline) {
        pipeline.state = VideoEncodingPipelineState::Configured;
        pipeline.output = VideoEncodingOutput {
            frames_submitted: 0,
            frames_encoded: 0,
            keyframes_encoded: 0,
            bytes_produced: 0,
            last_frame: None,
        };
    }

    pub fn drain(pipeline: &mut VideoEncodingPipeline) {
        pipeline.state = VideoEncodingPipelineState::Drained;
    }

    fn target_bitrate_kbps(viewport: &apprelay_protocol::ViewportSize) -> u32 {
        let pixels_per_second =
            u64::from(viewport.width) * u64::from(viewport.height) * u64::from(Self::MAX_FPS);
        (pixels_per_second / 10_000)
            .max(250)
            .min(u64::from(u32::MAX)) as u32
    }

    fn adapt_resolution(
        requested_viewport: apprelay_protocol::ViewportSize,
    ) -> VideoResolutionAdaptation {
        let current_target = Self::cap_to_limits(&requested_viewport);
        let reason = if current_target == requested_viewport {
            VideoResolutionAdaptationReason::MatchesViewport
        } else {
            VideoResolutionAdaptationReason::CappedToLimits
        };

        VideoResolutionAdaptation {
            requested_viewport,
            current_target,
            limits: VideoResolutionLimits {
                max_width: Self::MAX_WIDTH,
                max_height: Self::MAX_HEIGHT,
                max_pixels: Self::MAX_PIXELS,
            },
            reason,
        }
    }

    fn cap_to_limits(
        viewport: &apprelay_protocol::ViewportSize,
    ) -> apprelay_protocol::ViewportSize {
        let width = u64::from(viewport.width);
        let height = u64::from(viewport.height);

        if width == 0 || height == 0 {
            return apprelay_protocol::ViewportSize::new(Self::MIN_DIMENSION, Self::MIN_DIMENSION);
        }

        let pixels = width.saturating_mul(height);

        if width <= u64::from(Self::MAX_WIDTH)
            && height <= u64::from(Self::MAX_HEIGHT)
            && pixels <= u64::from(Self::MAX_PIXELS)
        {
            return viewport.clone();
        }

        let width_scale = f64::from(Self::MAX_WIDTH) / viewport.width as f64;
        let height_scale = f64::from(Self::MAX_HEIGHT) / viewport.height as f64;
        let pixel_scale = (f64::from(Self::MAX_PIXELS) / pixels as f64).sqrt();
        let scale = width_scale.min(height_scale).min(pixel_scale);

        apprelay_protocol::ViewportSize::new(
            Self::floor_to_even((viewport.width as f64 * scale).floor() as u64),
            Self::floor_to_even((viewport.height as f64 * scale).floor() as u64),
        )
    }

    fn floor_to_even(value: u64) -> u32 {
        let even = if value > u64::from(Self::MIN_DIMENSION) {
            value - (value % 2)
        } else {
            u64::from(Self::MIN_DIMENSION)
        };
        even.min(u64::from(u32::MAX)) as u32
    }

    fn encoded_frame_bytes(
        viewport: &apprelay_protocol::ViewportSize,
        target_bitrate_kbps: u32,
        keyframe: bool,
    ) -> u32 {
        let frame_budget = (u64::from(target_bitrate_kbps) * 1_000 / 8) / u64::from(Self::MAX_FPS);
        let floor = u64::from(viewport.width) * u64::from(viewport.height) / 200;
        let multiplier = if keyframe { 2 } else { 1 };
        ((frame_budget.max(floor)) * multiplier).min(u64::from(u32::MAX)) as u32
    }
}

fn server_ice_candidates(candidates: &[WebRtcIceCandidate]) -> Vec<WebRtcIceCandidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.candidate.starts_with("candidate:apprelay "))
        .cloned()
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowCaptureBackendService {
    LinuxSelectedWindow,
    Unsupported { platform: Platform },
}

impl WindowCaptureBackend for WindowCaptureBackendService {
    fn start_capture(
        &self,
        stream_id: &str,
        session: &ApplicationSession,
    ) -> Result<WindowCaptureStart, AppRelayError> {
        match self {
            Self::LinuxSelectedWindow => Ok(WindowCaptureStart {
                source: VideoCaptureSource {
                    scope: VideoCaptureScope::SelectedWindow,
                    selected_window_id: session.selected_window.id.clone(),
                    application_id: session.selected_window.application_id.clone(),
                    title: session.selected_window.title.clone(),
                },
                signaling: VideoStreamSignaling {
                    kind: VideoStreamSignalingKind::WebRtcOffer,
                    negotiation_state: VideoStreamNegotiationState::AwaitingAnswer,
                    offer: Some(WebRtcSessionDescription {
                        sdp_type: WebRtcSdpType::Offer,
                        sdp: format!(
                            "apprelay-webrtc-offer:{stream_id}:{}",
                            session.selected_window.id
                        ),
                    }),
                    answer: None,
                    ice_candidates: vec![WebRtcIceCandidate {
                        candidate: format!(
                            "candidate:apprelay {stream_id} {} typ host",
                            session.selected_window.id
                        ),
                        sdp_mid: Some("video".to_string()),
                        sdp_m_line_index: Some(0),
                    }],
                },
            }),
            Self::Unsupported { platform } => Err(AppRelayError::unsupported(
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
    const MIN_VIEWPORT_WIDTH: u32 = 320;
    const MIN_VIEWPORT_HEIGHT: u32 = 240;
    const MAX_VIEWPORT_WIDTH: u32 = 7680;
    const MAX_VIEWPORT_HEIGHT: u32 = 4320;

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

    fn accepts_resize_viewport(viewport: &apprelay_protocol::ViewportSize) -> bool {
        viewport.width >= Self::MIN_VIEWPORT_WIDTH
            && viewport.height >= Self::MIN_VIEWPORT_HEIGHT
            && viewport.width <= Self::MAX_VIEWPORT_WIDTH
            && viewport.height <= Self::MAX_VIEWPORT_HEIGHT
    }
}

impl Default for InMemoryVideoStreamService {
    fn default() -> Self {
        Self::new(WindowCaptureBackendService::LinuxSelectedWindow)
    }
}

impl VideoStreamService for InMemoryVideoStreamService {
    fn start_stream(
        &mut self,
        request: StartVideoStreamRequest,
        session: &ApplicationSession,
    ) -> Result<VideoStreamSession, AppRelayError> {
        if session.id != request.session_id || session.state == SessionState::Closed {
            return Err(AppRelayError::NotFound(format!(
                "session {} was not found",
                request.session_id
            )));
        }

        let stream_id = self.next_stream_id();
        let capture = self.capture_backend.start_capture(&stream_id, session)?;
        let stream = VideoStreamSession {
            id: stream_id,
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            viewport: session.viewport.clone(),
            capture_source: capture.source,
            encoding: InMemoryVideoEncodingPipeline::configured(session.viewport.clone()),
            signaling: capture.signaling,
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
    ) -> Result<VideoStreamSession, AppRelayError> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| {
                stream.id == request.stream_id && stream.state != VideoStreamState::Stopped
            })
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;

        stream.state = VideoStreamState::Stopped;
        InMemoryVideoEncodingPipeline::drain(&mut stream.encoding);
        stream.health = VideoStreamHealth {
            healthy: false,
            message: Some("stream stopped by client".to_string()),
        };
        Ok(stream.clone())
    }

    fn reconnect_stream(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| stream.id == request.stream_id)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;

        if stream.state == VideoStreamState::Stopped {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} has been stopped",
                request.stream_id
            )));
        }

        stream.state = VideoStreamState::Starting;
        InMemoryVideoEncodingPipeline::reset_for_reconnect(&mut stream.encoding);
        stream.signaling.negotiation_state = VideoStreamNegotiationState::AwaitingAnswer;
        stream.signaling.answer = None;
        stream.signaling.ice_candidates = server_ice_candidates(&stream.signaling.ice_candidates);
        stream.stats.frames_encoded = 0;
        stream.stats.bitrate_kbps = stream.encoding.contract.target.target_bitrate_kbps;
        stream.stats.latency_ms = 0;
        stream.stats.reconnect_attempts += 1;
        stream.health = VideoStreamHealth {
            healthy: true,
            message: Some("reconnect requested".to_string()),
        };
        Ok(stream.clone())
    }

    fn negotiate_stream(
        &mut self,
        request: NegotiateVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        if request.client_answer.sdp_type != WebRtcSdpType::Answer {
            return Err(AppRelayError::InvalidRequest(
                "video stream negotiation requires a WebRTC answer".to_string(),
            ));
        }

        let stream = self
            .streams
            .iter_mut()
            .find(|stream| stream.id == request.stream_id)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;

        if stream.state == VideoStreamState::Stopped {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} has been stopped",
                request.stream_id
            )));
        }

        if stream.signaling.negotiation_state == VideoStreamNegotiationState::Negotiated {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} has already been negotiated",
                request.stream_id
            )));
        }

        stream.signaling.answer = Some(request.client_answer);
        stream
            .signaling
            .ice_candidates
            .extend(request.client_ice_candidates);
        stream.signaling.negotiation_state = VideoStreamNegotiationState::Negotiated;
        InMemoryVideoEncodingPipeline::encode_next_frame(&mut stream.encoding);
        stream.stats.frames_encoded = stream.encoding.output.frames_encoded;
        stream.stats.bitrate_kbps = stream.encoding.contract.target.target_bitrate_kbps;
        stream.stats.latency_ms = 1_000 / stream.encoding.contract.target.max_fps;
        stream.state = VideoStreamState::Streaming;
        stream.health = VideoStreamHealth {
            healthy: true,
            message: Some("WebRTC negotiation completed".to_string()),
        };

        Ok(stream.clone())
    }

    fn record_resize(&mut self, request: &ResizeSessionRequest) {
        if !Self::accepts_resize_viewport(&request.viewport) {
            return;
        }

        for stream in self.streams.iter_mut().filter(|stream| {
            stream.session_id == request.session_id
                && stream.state != VideoStreamState::Stopped
                && stream.state != VideoStreamState::Failed
        }) {
            stream.viewport = request.viewport.clone();
            InMemoryVideoEncodingPipeline::reconfigure(
                &mut stream.encoding,
                request.viewport.clone(),
                stream.state == VideoStreamState::Streaming,
            );
            stream.stats.bitrate_kbps = stream.encoding.contract.target.target_bitrate_kbps;
            if stream.state == VideoStreamState::Streaming {
                stream.stats.latency_ms = 1_000 / stream.encoding.contract.target.max_fps;
            }
            stream.health = VideoStreamHealth {
                healthy: true,
                message: Some("stream viewport updated".to_string()),
            };
        }
    }

    fn stream_status(&self, stream_id: &str) -> Result<VideoStreamSession, AppRelayError> {
        self.streams
            .iter()
            .find(|stream| stream.id == stream_id)
            .cloned()
            .ok_or_else(|| AppRelayError::NotFound(format!("stream {stream_id} was not found")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApplicationSessionService, InMemoryApplicationSessionService};
    use apprelay_protocol::{CreateSessionRequest, ViewportSize};

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
        assert_eq!(
            stream.capture_source.scope,
            VideoCaptureScope::SelectedWindow
        );
        assert_eq!(
            stream.capture_source.selected_window_id,
            session.selected_window.id
        );
        assert_eq!(
            stream.capture_source.application_id,
            session.selected_window.application_id
        );
        assert_eq!(stream.viewport, ViewportSize::new(1280, 720));
        assert_eq!(stream.state, VideoStreamState::Starting);
        assert_eq!(
            stream.encoding.contract.target.resolution,
            ViewportSize::new(1280, 720)
        );
        assert_eq!(
            stream.encoding.contract.adaptation.current_target,
            ViewportSize::new(1280, 720)
        );
        assert_eq!(
            stream.encoding.contract.adaptation.reason,
            VideoResolutionAdaptationReason::MatchesViewport
        );
        assert_eq!(stream.encoding.contract.codec, VideoCodec::H264);
        assert_eq!(
            stream.encoding.contract.hardware_acceleration,
            VideoHardwareAcceleration::None
        );
        assert_eq!(
            stream.encoding.state,
            VideoEncodingPipelineState::Configured
        );
        assert_eq!(stream.encoding.output.frames_encoded, 0);
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
        assert_eq!(stopped.encoding.state, VideoEncodingPipelineState::Drained);
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
        assert_eq!(
            reconnected.signaling.negotiation_state,
            VideoStreamNegotiationState::AwaitingAnswer
        );
        assert_eq!(reconnected.stats.reconnect_attempts, 1);
        assert_eq!(
            reconnected.health.message.as_deref(),
            Some("reconnect requested")
        );
    }

    #[test]
    fn video_stream_service_negotiates_webrtc_answer() {
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

        let negotiated = stream_service
            .negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: vec![WebRtcIceCandidate {
                    candidate: "candidate:client stream-1 typ host".to_string(),
                    sdp_mid: Some("video".to_string()),
                    sdp_m_line_index: Some(0),
                }],
            })
            .expect("negotiate stream");

        assert_eq!(negotiated.state, VideoStreamState::Streaming);
        assert_eq!(
            negotiated.encoding.state,
            VideoEncodingPipelineState::Encoding
        );
        assert_eq!(negotiated.encoding.output.frames_submitted, 1);
        assert_eq!(negotiated.encoding.output.frames_encoded, 1);
        assert_eq!(negotiated.encoding.output.keyframes_encoded, 1);
        assert_eq!(
            negotiated.encoding.output.last_frame,
            Some(apprelay_protocol::EncodedVideoFrame {
                sequence: 1,
                timestamp_ms: 0,
                byte_length: 23032,
                keyframe: true,
            })
        );
        assert_eq!(negotiated.stats.frames_encoded, 1);
        assert_eq!(negotiated.stats.bitrate_kbps, 2764);
        assert_eq!(negotiated.stats.latency_ms, 33);
        assert_eq!(
            negotiated.signaling.negotiation_state,
            VideoStreamNegotiationState::Negotiated
        );
        assert_eq!(
            negotiated.signaling.answer,
            Some(WebRtcSessionDescription {
                sdp_type: WebRtcSdpType::Answer,
                sdp: "client-answer".to_string(),
            })
        );
        assert_eq!(negotiated.signaling.ice_candidates.len(), 2);
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
            status.encoding.contract.target.resolution,
            ViewportSize::new(1440, 900)
        );
        assert_eq!(status.encoding.contract.target.target_bitrate_kbps, 3888);
        assert_eq!(
            status.encoding.state,
            VideoEncodingPipelineState::Configured
        );
        assert_eq!(
            status.health.message.as_deref(),
            Some("stream viewport updated")
        );
    }

    #[test]
    fn video_stream_service_caps_non_negotiated_encoding_to_adaptive_limits() {
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
            viewport: ViewportSize::new(2560, 1440),
        });

        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.viewport, ViewportSize::new(2560, 1440));
        assert_eq!(
            status.encoding.contract.target.resolution,
            ViewportSize::new(1920, 1080)
        );
        assert_eq!(
            status.encoding.contract.adaptation.requested_viewport,
            ViewportSize::new(2560, 1440)
        );
        assert_eq!(
            status.encoding.contract.adaptation.current_target,
            ViewportSize::new(1920, 1080)
        );
        assert_eq!(
            status.encoding.contract.adaptation.limits,
            VideoResolutionLimits {
                max_width: 1920,
                max_height: 1080,
                max_pixels: 2_073_600,
            }
        );
        assert_eq!(
            status.encoding.contract.adaptation.reason,
            VideoResolutionAdaptationReason::CappedToLimits
        );
        assert_eq!(status.encoding.contract.target.target_bitrate_kbps, 6220);
        assert_eq!(
            status.encoding.state,
            VideoEncodingPipelineState::Configured
        );
    }

    #[test]
    fn video_stream_service_ignores_invalid_direct_resize_viewports() {
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

        for viewport in [
            ViewportSize::new(0, 720),
            ViewportSize::new(100, 100),
            ViewportSize::new(8000, 4500),
        ] {
            stream_service.record_resize(&ResizeSessionRequest {
                session_id: session.id.clone(),
                viewport,
            });
        }

        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.viewport, ViewportSize::new(1280, 720));
        assert_eq!(
            status.encoding.contract.adaptation.requested_viewport,
            ViewportSize::new(1280, 720)
        );
        assert_eq!(
            status.encoding.contract.target.resolution,
            ViewportSize::new(1280, 720)
        );
        assert_eq!(
            status.encoding.contract.adaptation.current_target,
            ViewportSize::new(1280, 720)
        );
        assert_eq!(
            status.encoding.contract.adaptation.reason,
            VideoResolutionAdaptationReason::MatchesViewport
        );
        assert_eq!(status.encoding.contract.target.target_bitrate_kbps, 2764);
        assert_eq!(status.health.message, None);
    }

    #[test]
    fn video_stream_service_keeps_streaming_encoding_coherent_after_resize() {
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

        stream_service
            .negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            })
            .expect("negotiate stream");

        stream_service.record_resize(&ResizeSessionRequest {
            session_id: session.id,
            viewport: ViewportSize::new(2560, 1440),
        });

        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.state, VideoStreamState::Streaming);
        assert_eq!(status.encoding.state, VideoEncodingPipelineState::Encoding);
        assert_eq!(status.viewport, ViewportSize::new(2560, 1440));
        assert_eq!(
            status.encoding.contract.target.resolution,
            ViewportSize::new(1920, 1080)
        );
        assert_eq!(
            status.encoding.contract.adaptation.reason,
            VideoResolutionAdaptationReason::CappedToLimits
        );
        assert_eq!(status.stats.frames_encoded, 1);
        assert_eq!(status.stats.bitrate_kbps, 6220);
        assert_eq!(status.stats.latency_ms, 33);
    }

    #[test]
    fn video_stream_service_reconnect_renegotiation_starts_with_fresh_keyframe() {
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

        stream_service
            .negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "first-client-answer".to_string(),
                },
                client_ice_candidates: vec![WebRtcIceCandidate {
                    candidate: "candidate:first-client stream-1 typ host".to_string(),
                    sdp_mid: Some("video".to_string()),
                    sdp_m_line_index: Some(0),
                }],
            })
            .expect("first negotiate stream");

        let reconnected = stream_service
            .reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: stream.id.clone(),
            })
            .expect("reconnect stream");
        assert_eq!(
            reconnected.encoding.state,
            VideoEncodingPipelineState::Configured
        );
        assert_eq!(reconnected.encoding.output.frames_encoded, 0);
        assert_eq!(reconnected.encoding.output.last_frame, None);
        assert_eq!(
            reconnected.signaling.ice_candidates,
            vec![WebRtcIceCandidate {
                candidate: "candidate:apprelay stream-1 window-session-1 typ host".to_string(),
                sdp_mid: Some("video".to_string()),
                sdp_m_line_index: Some(0),
            }]
        );

        let renegotiated = stream_service
            .negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: stream.id,
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "second-client-answer".to_string(),
                },
                client_ice_candidates: vec![WebRtcIceCandidate {
                    candidate: "candidate:second-client stream-1 typ host".to_string(),
                    sdp_mid: Some("video".to_string()),
                    sdp_m_line_index: Some(0),
                }],
            })
            .expect("second negotiate stream");

        assert_eq!(
            renegotiated.encoding.output.last_frame,
            Some(apprelay_protocol::EncodedVideoFrame {
                sequence: 1,
                timestamp_ms: 0,
                byte_length: 23032,
                keyframe: true,
            })
        );
        assert_eq!(renegotiated.encoding.output.keyframes_encoded, 1);
        assert_eq!(renegotiated.stats.frames_encoded, 1);
        assert_eq!(renegotiated.stats.reconnect_attempts, 1);
        assert_eq!(renegotiated.signaling.ice_candidates.len(), 2);
        assert!(renegotiated
            .signaling
            .ice_candidates
            .iter()
            .any(|candidate| candidate.candidate
                == "candidate:apprelay stream-1 window-session-1 typ host"));
        assert!(renegotiated
            .signaling
            .ice_candidates
            .iter()
            .any(|candidate| candidate.candidate == "candidate:second-client stream-1 typ host"));
        assert!(!renegotiated
            .signaling
            .ice_candidates
            .iter()
            .any(|candidate| candidate.candidate == "candidate:first-client stream-1 typ host"));
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
            Err(AppRelayError::unsupported(
                Platform::Linux,
                Feature::WindowVideoStream
            ))
        );
    }
}
