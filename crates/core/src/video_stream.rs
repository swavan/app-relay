use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use apprelay_protocol::{
    AppRelayError, ApplicationSession, CapturedVideoFrame, Feature, NegotiateVideoStreamRequest,
    Platform, ReconnectVideoStreamRequest, ResizeSessionRequest, SelectedWindow, SessionState,
    StartVideoStreamRequest, StopVideoStreamRequest, VideoCaptureRuntimeState,
    VideoCaptureRuntimeStatus, VideoCaptureScope, VideoCaptureSource, VideoCodec,
    VideoEncodingContract, VideoEncodingOutput, VideoEncodingPipeline, VideoEncodingPipelineState,
    VideoEncodingTarget, VideoHardwareAcceleration, VideoPixelFormat, VideoResolutionAdaptation,
    VideoResolutionAdaptationReason, VideoResolutionLimits, VideoStreamFailure,
    VideoStreamFailureKind, VideoStreamHealth, VideoStreamNegotiationState, VideoStreamRecovery,
    VideoStreamRecoveryAction, VideoStreamSession, VideoStreamSignaling, VideoStreamSignalingKind,
    VideoStreamState, VideoStreamStats, ViewportSize, WebRtcIceCandidate, WebRtcSdpType,
    WebRtcSessionDescription, WindowSelectionMethod,
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
    fn record_session_closed(&mut self, session_id: &str);
    fn active_streams(&mut self) -> Vec<VideoStreamSession>;
    fn stream_status(&mut self, stream_id: &str) -> Result<VideoStreamSession, AppRelayError>;
}

pub trait WindowCaptureBackend {
    fn start_capture(
        &self,
        stream_id: &str,
        session: &ApplicationSession,
    ) -> Result<WindowCaptureStart, AppRelayError>;
    fn stop_capture(&self, stream_id: &str);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowCaptureStart {
    pub source: VideoCaptureSource,
    pub signaling: VideoStreamSignaling,
}

pub trait MacosWindowCaptureRuntime: std::fmt::Debug + Send + Sync {
    fn start(&self, request: MacosWindowCaptureStartRequest) -> Result<(), AppRelayError>;
    fn resize(&self, request: MacosWindowCaptureResizeRequest) -> Result<(), AppRelayError>;
    fn stop(&self, stream_id: &str);
    fn snapshot(&self, stream_id: &str) -> Option<VideoCaptureRuntimeStatus>;

    /// Latest H.264 Annex-B payload for the given stream, if a real
    /// hardware encoder is bridged into the capture runtime. The
    /// default implementation returns `None` so capture-only runtimes
    /// (and every non-macOS or non-VideoToolbox build) keep emitting
    /// payload-free `EncodedVideoFrame`s without any code change.
    fn latest_encoded_payload(&self, _stream_id: &str) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacosWindowCaptureStartRequest {
    pub stream_id: String,
    pub selected_window_id: String,
    pub application_id: String,
    pub title: String,
    pub target_viewport: ViewportSize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacosWindowCaptureResizeRequest {
    pub stream_id: String,
    pub selected_window_id: String,
    pub target_viewport: ViewportSize,
}

#[derive(Clone, Debug, Default)]
pub struct ControlPlaneMacosWindowCaptureRuntime {
    calls: Arc<Mutex<MacosWindowCaptureRuntimeCalls>>,
    snapshots: Arc<Mutex<HashMap<String, VideoCaptureRuntimeStatus>>>,
}

impl MacosWindowCaptureRuntime for ControlPlaneMacosWindowCaptureRuntime {
    fn start(&self, request: MacosWindowCaptureStartRequest) -> Result<(), AppRelayError> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .starts
            .push(request.clone());
        self.snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                request.stream_id,
                VideoCaptureRuntimeStatus {
                    state: VideoCaptureRuntimeState::Starting,
                    frames_delivered: 0,
                    last_frame: None,
                    message: None,
                },
            );
        Ok(())
    }

    fn resize(&self, request: MacosWindowCaptureResizeRequest) -> Result<(), AppRelayError> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .resizes
            .push(request);
        Ok(())
    }

    fn stop(&self, stream_id: &str) {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stops
            .push(stream_id.to_string());
        self.snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(stream_id);
    }

    fn snapshot(&self, stream_id: &str) -> Option<VideoCaptureRuntimeStatus> {
        self.snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(stream_id)
            .cloned()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacosWindowCaptureRuntimeCalls {
    pub starts: Vec<MacosWindowCaptureStartRequest>,
    pub resizes: Vec<MacosWindowCaptureResizeRequest>,
    pub stops: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FakeMacosWindowCaptureRuntime {
    calls: Arc<Mutex<MacosWindowCaptureRuntimeCalls>>,
    snapshots: Arc<Mutex<HashMap<String, VideoCaptureRuntimeStatus>>>,
    start_failures: Arc<Mutex<Vec<String>>>,
    resize_failures: Arc<Mutex<Vec<String>>>,
}

impl FakeMacosWindowCaptureRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fail_next_start(&self, message: impl Into<String>) {
        self.start_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(message.into());
    }

    pub fn fail_next_resize(&self, message: impl Into<String>) {
        self.resize_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(message.into());
    }

    pub fn deliver_frame(
        &self,
        stream_id: &str,
        size: ViewportSize,
        timestamp_ms: u64,
    ) -> VideoCaptureRuntimeStatus {
        let mut snapshots = self
            .snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = snapshots.get(stream_id).cloned().unwrap_or_default();
        let sequence = previous.frames_delivered + 1;
        let snapshot = VideoCaptureRuntimeStatus {
            state: VideoCaptureRuntimeState::Delivering,
            frames_delivered: sequence,
            last_frame: Some(CapturedVideoFrame {
                sequence,
                timestamp_ms,
                size,
            }),
            message: None,
        };
        snapshots.insert(stream_id.to_string(), snapshot.clone());
        snapshot
    }

    pub fn fail_stream(&self, stream_id: &str, message: impl Into<String>) {
        self.record_terminal_snapshot(stream_id, VideoCaptureRuntimeState::Failed, message);
    }

    pub fn deny_permission(&self, stream_id: &str, message: impl Into<String>) {
        self.record_terminal_snapshot(
            stream_id,
            VideoCaptureRuntimeState::PermissionDenied,
            message,
        );
    }

    pub fn calls(&self) -> MacosWindowCaptureRuntimeCalls {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn record_terminal_snapshot(
        &self,
        stream_id: &str,
        state: VideoCaptureRuntimeState,
        message: impl Into<String>,
    ) {
        let mut snapshots = self
            .snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = snapshots.get(stream_id).cloned().unwrap_or_default();
        snapshots.insert(
            stream_id.to_string(),
            VideoCaptureRuntimeStatus {
                state,
                frames_delivered: previous.frames_delivered,
                last_frame: previous.last_frame,
                message: Some(message.into()),
            },
        );
    }
}

impl MacosWindowCaptureRuntime for FakeMacosWindowCaptureRuntime {
    fn start(&self, request: MacosWindowCaptureStartRequest) -> Result<(), AppRelayError> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .starts
            .push(request.clone());

        if let Some(message) = self
            .start_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop()
        {
            Err(AppRelayError::InvalidRequest(message))
        } else {
            self.snapshots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    request.stream_id,
                    VideoCaptureRuntimeStatus {
                        state: VideoCaptureRuntimeState::Starting,
                        frames_delivered: 0,
                        last_frame: None,
                        message: None,
                    },
                );
            Ok(())
        }
    }

    fn resize(&self, request: MacosWindowCaptureResizeRequest) -> Result<(), AppRelayError> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .resizes
            .push(request);

        if let Some(message) = self
            .resize_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop()
        {
            Err(AppRelayError::InvalidRequest(message))
        } else {
            Ok(())
        }
    }

    fn stop(&self, stream_id: &str) {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stops
            .push(stream_id.to_string());
        self.snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(stream_id);
    }

    fn snapshot(&self, stream_id: &str) -> Option<VideoCaptureRuntimeStatus> {
        self.snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(stream_id)
            .cloned()
    }
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
            // The in-memory pipeline never produces real bytes; it stays
            // payload-free so existing tests that compare encoded frames
            // structurally keep matching. Real H.264 bytes are populated
            // by `H264VideoEncoder` implementations such as the macOS
            // VideoToolbox encoder behind the `macos-videotoolbox`
            // feature.
            payload: Vec::new(),
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

#[derive(Clone, Debug)]
pub enum WindowCaptureBackendService {
    LinuxSelectedWindow,
    MacosSelectedWindow {
        runtime: Arc<dyn MacosWindowCaptureRuntime>,
    },
    FailingSelectedWindow {
        message: String,
    },
    FailsOnceSelectedWindow {
        failed: Arc<AtomicBool>,
        message: String,
    },
    Unsupported {
        platform: Platform,
    },
}

impl WindowCaptureBackendService {
    pub fn macos_selected_window() -> Self {
        Self::MacosSelectedWindow {
            runtime: Arc::new(ControlPlaneMacosWindowCaptureRuntime::default()),
        }
    }

    pub fn macos_selected_window_with_runtime(runtime: Arc<dyn MacosWindowCaptureRuntime>) -> Self {
        Self::MacosSelectedWindow { runtime }
    }

    pub fn fails_once(message: impl Into<String>) -> Self {
        Self::FailsOnceSelectedWindow {
            failed: Arc::new(AtomicBool::new(false)),
            message: message.into(),
        }
    }

    pub fn resize_capture(
        &self,
        stream_id: &str,
        selected_window_id: &str,
        target_viewport: ViewportSize,
    ) -> Result<(), AppRelayError> {
        match self {
            Self::MacosSelectedWindow { runtime } => {
                runtime.resize(MacosWindowCaptureResizeRequest {
                    stream_id: stream_id.to_string(),
                    selected_window_id: selected_window_id.to_string(),
                    target_viewport,
                })
            }
            Self::LinuxSelectedWindow | Self::FailsOnceSelectedWindow { .. } => Ok(()),
            Self::FailingSelectedWindow { message } => {
                Err(AppRelayError::InvalidRequest(message.clone()))
            }
            Self::Unsupported { platform } => Err(AppRelayError::unsupported(
                *platform,
                Feature::WindowVideoStream,
            )),
        }
    }

    pub fn capture_snapshot(&self, stream_id: &str) -> Option<VideoCaptureRuntimeStatus> {
        match self {
            Self::MacosSelectedWindow { runtime } => runtime.snapshot(stream_id),
            Self::LinuxSelectedWindow
            | Self::FailingSelectedWindow { .. }
            | Self::FailsOnceSelectedWindow { .. }
            | Self::Unsupported { .. } => None,
        }
    }

    /// Latest H.264 Annex-B payload for `stream_id`, if a real
    /// hardware encoder is bridged into the capture runtime. Always
    /// `None` for capture-only and non-macOS backends.
    pub fn latest_encoded_payload(&self, stream_id: &str) -> Option<Vec<u8>> {
        match self {
            Self::MacosSelectedWindow { runtime } => runtime.latest_encoded_payload(stream_id),
            Self::LinuxSelectedWindow
            | Self::FailingSelectedWindow { .. }
            | Self::FailsOnceSelectedWindow { .. }
            | Self::Unsupported { .. } => None,
        }
    }
}

impl WindowCaptureBackend for WindowCaptureBackendService {
    fn start_capture(
        &self,
        stream_id: &str,
        session: &ApplicationSession,
    ) -> Result<WindowCaptureStart, AppRelayError> {
        match self {
            Self::LinuxSelectedWindow => Ok(WindowCaptureStart {
                source: InMemoryVideoStreamService::capture_source_from_session(session),
                signaling: InMemoryVideoStreamService::signaling_for_stream(
                    stream_id,
                    &session.selected_window.id,
                ),
            }),
            Self::MacosSelectedWindow { runtime } => {
                runtime.start(MacosWindowCaptureStartRequest {
                    stream_id: stream_id.to_string(),
                    selected_window_id: session.selected_window.id.clone(),
                    application_id: session.selected_window.application_id.clone(),
                    title: session.selected_window.title.clone(),
                    target_viewport: session.viewport.clone(),
                })?;
                Ok(WindowCaptureStart {
                    source: InMemoryVideoStreamService::capture_source_from_session(session),
                    signaling: InMemoryVideoStreamService::signaling_for_stream(
                        stream_id,
                        &session.selected_window.id,
                    ),
                })
            }
            Self::FailingSelectedWindow { message } => {
                Err(AppRelayError::InvalidRequest(message.clone()))
            }
            Self::FailsOnceSelectedWindow { failed, message } => {
                if failed.swap(true, Ordering::Relaxed) {
                    Ok(WindowCaptureStart {
                        source: InMemoryVideoStreamService::capture_source_from_session(session),
                        signaling: InMemoryVideoStreamService::signaling_for_stream(
                            stream_id,
                            &session.selected_window.id,
                        ),
                    })
                } else {
                    Err(AppRelayError::InvalidRequest(message.clone()))
                }
            }
            Self::Unsupported { platform } => Err(AppRelayError::unsupported(
                *platform,
                Feature::WindowVideoStream,
            )),
        }
    }

    fn stop_capture(&self, stream_id: &str) {
        if let Self::MacosSelectedWindow { runtime } = self {
            runtime.stop(stream_id);
        }
    }
}

#[derive(Clone, Debug)]
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

    /// Test-only: drive the in-memory encoding pipeline forward by one
    /// frame for `stream_id`, populating `encoding.output.last_frame`
    /// with a fresh `EncodedVideoFrame`. Real builds advance this
    /// pipeline through capture-runtime callbacks; this helper lets
    /// tests exercise the encoded-frame pump deterministically without
    /// reaching into private state. Returns the new sequence on
    /// success, or `None` if the stream was not found.
    #[doc(hidden)]
    pub fn advance_encoded_frame_for_test(&mut self, stream_id: &str) -> Option<u64> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| stream.id == stream_id)?;
        InMemoryVideoEncodingPipeline::encode_next_frame(&mut stream.encoding);
        stream.stats.frames_encoded = stream.encoding.output.frames_encoded;
        stream
            .encoding
            .output
            .last_frame
            .as_ref()
            .map(|frame| frame.sequence)
    }

    fn accepts_resize_viewport(viewport: &apprelay_protocol::ViewportSize) -> bool {
        viewport.width >= Self::MIN_VIEWPORT_WIDTH
            && viewport.height >= Self::MIN_VIEWPORT_HEIGHT
            && viewport.width <= Self::MAX_VIEWPORT_WIDTH
            && viewport.height <= Self::MAX_VIEWPORT_HEIGHT
    }

    fn capture_source_from_session(session: &ApplicationSession) -> VideoCaptureSource {
        VideoCaptureSource {
            scope: VideoCaptureScope::SelectedWindow,
            selected_window_id: session.selected_window.id.clone(),
            application_id: session.selected_window.application_id.clone(),
            title: session.selected_window.title.clone(),
        }
    }

    fn signaling_for_stream(stream_id: &str, selected_window_id: &str) -> VideoStreamSignaling {
        VideoStreamSignaling {
            kind: VideoStreamSignalingKind::WebRtcOffer,
            negotiation_state: VideoStreamNegotiationState::AwaitingAnswer,
            offer: Some(WebRtcSessionDescription {
                sdp_type: WebRtcSdpType::Offer,
                sdp: format!("apprelay-webrtc-offer:{stream_id}:{selected_window_id}"),
            }),
            answer: None,
            ice_candidates: vec![WebRtcIceCandidate {
                candidate: format!("candidate:apprelay {stream_id} {selected_window_id} typ host"),
                sdp_mid: Some("video".to_string()),
                sdp_m_line_index: Some(0),
            }],
        }
    }

    fn capture_failure(message: impl Into<String>) -> VideoStreamFailure {
        let message = message.into();
        VideoStreamFailure {
            kind: VideoStreamFailureKind::CaptureFailed,
            message: message.clone(),
            recovery: VideoStreamRecovery {
                action: VideoStreamRecoveryAction::ReconnectStream,
                message: "fix the capture backend and reconnect the stream".to_string(),
                retryable: true,
            },
        }
    }

    fn capture_error_message(error: &AppRelayError) -> String {
        match error {
            AppRelayError::ServiceUnavailable(message)
            | AppRelayError::InvalidRequest(message)
            | AppRelayError::PermissionDenied(message)
            | AppRelayError::NotFound(message) => message.clone(),
            AppRelayError::UnsupportedPlatform { platform, feature } => {
                format!("{feature:?} is unsupported on {platform:?}")
            }
        }
    }

    fn capture_runtime_failure_status(message: impl Into<String>) -> VideoCaptureRuntimeStatus {
        VideoCaptureRuntimeStatus {
            state: VideoCaptureRuntimeState::Failed,
            frames_delivered: 0,
            last_frame: None,
            message: Some(message.into()),
        }
    }

    fn app_closed_failure(session_id: &str) -> VideoStreamFailure {
        let message = format!("application session {session_id} closed");
        VideoStreamFailure {
            kind: VideoStreamFailureKind::AppClosed,
            message: message.clone(),
            recovery: VideoStreamRecovery {
                action: VideoStreamRecoveryAction::RestartApplicationSession,
                message: "start a new application session before streaming again".to_string(),
                retryable: false,
            },
        }
    }

    fn apply_failure(stream: &mut VideoStreamSession, failure: VideoStreamFailure) {
        stream.state = VideoStreamState::Failed;
        InMemoryVideoEncodingPipeline::drain(&mut stream.encoding);
        stream.stats.frames_encoded = stream.encoding.output.frames_encoded;
        stream.stats.bitrate_kbps = 0;
        stream.stats.latency_ms = 0;
        stream.health = VideoStreamHealth {
            healthy: false,
            message: Some(failure.message.clone()),
        };
        stream.failure_reason = Some(failure.message.clone());
        match failure.kind {
            VideoStreamFailureKind::CaptureFailed => {
                stream.capture_runtime =
                    Self::capture_runtime_failure_status(failure.message.clone());
            }
            VideoStreamFailureKind::AppClosed => {
                stream.capture_runtime = VideoCaptureRuntimeStatus::default();
            }
        }
        stream.failure = Some(failure);
    }

    fn clear_failure(stream: &mut VideoStreamSession) {
        stream.failure = None;
        stream.failure_reason = None;
    }

    fn session_snapshot_from_stream(stream: &VideoStreamSession) -> ApplicationSession {
        ApplicationSession {
            id: stream.session_id.clone(),
            application_id: stream.capture_source.application_id.clone(),
            selected_window: SelectedWindow {
                id: stream.selected_window_id.clone(),
                application_id: stream.capture_source.application_id.clone(),
                title: stream.capture_source.title.clone(),
                selection_method: WindowSelectionMethod::ExistingWindow,
            },
            launch_intent: None,
            viewport: stream.viewport.clone(),
            resize_intent: None,
            state: SessionState::Ready,
        }
    }

    fn reconcile_capture_runtime_snapshot(
        capture_backend: &WindowCaptureBackendService,
        stream: &mut VideoStreamSession,
    ) {
        if stream.state == VideoStreamState::Stopped {
            stream.capture_runtime = VideoCaptureRuntimeStatus::default();
            return;
        }

        let Some(snapshot) = capture_backend.capture_snapshot(&stream.id) else {
            // Even capture-only backends might not surface a snapshot
            // (e.g. Linux). Still attempt to surface any encoded
            // payload that an encoder bridge has produced — for the
            // default backends this is a `None` short-circuit.
            Self::reconcile_encoded_payload(capture_backend, stream);
            return;
        };

        let runtime_failed = matches!(
            snapshot.state,
            VideoCaptureRuntimeState::Failed | VideoCaptureRuntimeState::PermissionDenied
        );
        stream.capture_runtime = snapshot.clone();

        if runtime_failed && stream.state != VideoStreamState::Failed {
            let message = snapshot.message.clone().unwrap_or_else(|| {
                "macOS selected-window capture runtime reported failure".to_string()
            });
            let failure = Self::capture_failure(message);
            Self::apply_failure(stream, failure);
            stream.capture_runtime = snapshot;
            return;
        }

        Self::reconcile_encoded_payload(capture_backend, stream);
    }

    /// Pull any hardware-encoded H.264 payload the capture backend has
    /// staged for this stream and pin it to the most recent
    /// `EncodedVideoFrame`. With the default in-memory pipeline the
    /// backend always returns `None`, so the existing payload-free
    /// behaviour is preserved.
    fn reconcile_encoded_payload(
        capture_backend: &WindowCaptureBackendService,
        stream: &mut VideoStreamSession,
    ) {
        let Some(payload) = capture_backend.latest_encoded_payload(&stream.id) else {
            return;
        };
        if payload.is_empty() {
            return;
        }
        let Some(frame) = stream.encoding.output.last_frame.as_mut() else {
            return;
        };
        // Only refresh when the payload actually changed; otherwise we
        // would clone a `Vec<u8>` on every status poll. This keeps
        // serialisation costs predictable when the runtime is idle.
        if frame.payload != payload {
            frame.byte_length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
            frame.payload = payload;
        }
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
        let capture = self.capture_backend.start_capture(&stream_id, session);
        let (capture_source, signaling, failure) = match capture {
            Ok(capture) => (capture.source, capture.signaling, None),
            Err(error @ AppRelayError::UnsupportedPlatform { .. }) => return Err(error),
            Err(error) => (
                Self::capture_source_from_session(session),
                Self::signaling_for_stream(&stream_id, &session.selected_window.id),
                Some(Self::capture_failure(Self::capture_error_message(&error))),
            ),
        };

        let mut stream = VideoStreamSession {
            id: stream_id,
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            viewport: session.viewport.clone(),
            capture_source,
            capture_runtime: VideoCaptureRuntimeStatus::default(),
            encoding: InMemoryVideoEncodingPipeline::configured(session.viewport.clone()),
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
            failure: None,
            state: VideoStreamState::Starting,
            failure_reason: None,
        };

        if let Some(failure) = failure {
            Self::apply_failure(&mut stream, failure);
        } else {
            Self::reconcile_capture_runtime_snapshot(&self.capture_backend, &mut stream);
        }

        self.streams.push(stream.clone());
        Ok(stream)
    }

    fn stop_stream(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        let capture_backend = self.capture_backend.clone();
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| {
                stream.id == request.stream_id && stream.state != VideoStreamState::Stopped
            })
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;

        if stream.state != VideoStreamState::Failed {
            capture_backend.stop_capture(&stream.id);
        }
        stream.state = VideoStreamState::Stopped;
        InMemoryVideoEncodingPipeline::drain(&mut stream.encoding);
        stream.capture_runtime = VideoCaptureRuntimeStatus::default();
        stream.health = VideoStreamHealth {
            healthy: false,
            message: Some("stream stopped by client".to_string()),
        };
        Self::clear_failure(stream);
        Ok(stream.clone())
    }

    fn reconnect_stream(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        let capture_backend = self.capture_backend.clone();
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| stream.id == request.stream_id)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("stream {} was not found", request.stream_id))
            })?;
        Self::reconcile_capture_runtime_snapshot(&capture_backend, stream);

        if stream.state == VideoStreamState::Stopped {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} has been stopped",
                request.stream_id
            )));
        }

        if matches!(
            stream.failure.as_ref().map(|failure| &failure.kind),
            Some(VideoStreamFailureKind::AppClosed)
        ) {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} cannot reconnect because its application session closed",
                request.stream_id
            )));
        }

        let requires_capture_retry = matches!(
            stream.failure.as_ref(),
            Some(failure)
                if failure.kind == VideoStreamFailureKind::CaptureFailed
                    && failure.recovery.retryable
        );

        if requires_capture_retry {
            let session = Self::session_snapshot_from_stream(stream);
            match capture_backend.start_capture(&stream.id, &session) {
                Ok(capture) => {
                    stream.capture_source = capture.source;
                    stream.signaling = capture.signaling;
                }
                Err(error) => {
                    stream.stats.reconnect_attempts += 1;
                    let failure = Self::capture_failure(Self::capture_error_message(&error));
                    Self::apply_failure(stream, failure);
                    return Ok(stream.clone());
                }
            }
        }

        stream.state = VideoStreamState::Starting;
        InMemoryVideoEncodingPipeline::reset_for_reconnect(&mut stream.encoding);
        stream.signaling.negotiation_state = VideoStreamNegotiationState::AwaitingAnswer;
        stream.signaling.answer = None;
        stream.signaling.ice_candidates = server_ice_candidates(&stream.signaling.ice_candidates);
        stream.stats.frames_encoded = 0;
        stream.stats.bitrate_kbps = 0;
        stream.stats.latency_ms = 0;
        stream.stats.reconnect_attempts += 1;
        stream.health = VideoStreamHealth {
            healthy: true,
            message: Some("reconnect requested".to_string()),
        };
        Self::clear_failure(stream);
        Self::reconcile_capture_runtime_snapshot(&capture_backend, stream);
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
        Self::reconcile_capture_runtime_snapshot(&self.capture_backend, stream);

        if stream.state == VideoStreamState::Stopped {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} has been stopped",
                request.stream_id
            )));
        }

        if stream.state == VideoStreamState::Failed {
            return Err(AppRelayError::InvalidRequest(format!(
                "stream {} is failed and must be reconnected before negotiation",
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
        Self::clear_failure(stream);

        Ok(stream.clone())
    }

    fn record_resize(&mut self, request: &ResizeSessionRequest) {
        if !Self::accepts_resize_viewport(&request.viewport) {
            return;
        }

        let capture_backend = self.capture_backend.clone();
        for stream in self.streams.iter_mut().filter(|stream| {
            stream.session_id == request.session_id && stream.state != VideoStreamState::Stopped
        }) {
            Self::reconcile_capture_runtime_snapshot(&capture_backend, stream);
            if stream.state == VideoStreamState::Failed {
                continue;
            }

            if let Err(error) = capture_backend.resize_capture(
                &stream.id,
                &stream.selected_window_id,
                request.viewport.clone(),
            ) {
                capture_backend.stop_capture(&stream.id);
                let failure = Self::capture_failure(Self::capture_error_message(&error));
                Self::apply_failure(stream, failure);
                continue;
            }
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
            Self::clear_failure(stream);
        }
    }

    fn record_session_closed(&mut self, session_id: &str) {
        let capture_backend = self.capture_backend.clone();
        for stream in self.streams.iter_mut().filter(|stream| {
            stream.session_id == session_id && stream.state != VideoStreamState::Stopped
        }) {
            if stream.state != VideoStreamState::Failed {
                capture_backend.stop_capture(&stream.id);
            }
            let failure = Self::app_closed_failure(session_id);
            Self::apply_failure(stream, failure);
        }
    }

    fn active_streams(&mut self) -> Vec<VideoStreamSession> {
        let capture_backend = self.capture_backend.clone();
        self.streams
            .iter_mut()
            .filter_map(|stream| {
                Self::reconcile_capture_runtime_snapshot(&capture_backend, stream);
                (stream.state != VideoStreamState::Stopped).then(|| stream.clone())
            })
            .collect()
    }

    fn stream_status(&mut self, stream_id: &str) -> Result<VideoStreamSession, AppRelayError> {
        let capture_backend = self.capture_backend.clone();
        self.streams
            .iter_mut()
            .find(|stream| stream.id == stream_id)
            .map(|stream| {
                Self::reconcile_capture_runtime_snapshot(&capture_backend, stream);
                stream.clone()
            })
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
    fn macos_selected_window_capture_starts_metadata_backed_stream() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );

        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start macOS stream");

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
        assert_eq!(stream.capture_source.title, session.selected_window.title);
        assert_eq!(stream.signaling.kind, VideoStreamSignalingKind::WebRtcOffer);
        assert_eq!(
            stream.signaling.offer,
            Some(WebRtcSessionDescription {
                sdp_type: WebRtcSdpType::Offer,
                sdp: "apprelay-webrtc-offer:stream-1:macos-window-session-1-88".to_string(),
            })
        );
        assert_eq!(
            stream.signaling.ice_candidates,
            vec![WebRtcIceCandidate {
                candidate: "candidate:apprelay stream-1 macos-window-session-1-88 typ host"
                    .to_string(),
                sdp_mid: Some("video".to_string()),
                sdp_m_line_index: Some(0),
            }]
        );
        assert_eq!(
            runtime.calls().starts,
            vec![MacosWindowCaptureStartRequest {
                stream_id: "stream-1".to_string(),
                selected_window_id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                target_viewport: ViewportSize::new(1280, 720),
            }]
        );
        assert_eq!(runtime.calls().stops, Vec::<String>::new());
        assert_eq!(
            stream.capture_runtime.state,
            VideoCaptureRuntimeState::Starting
        );
        assert_eq!(stream.capture_runtime.frames_delivered, 0);
    }

    #[test]
    fn macos_capture_runtime_delivered_frame_snapshot_updates_capture_status_only() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start macOS stream");

        runtime.deliver_frame(&stream.id, ViewportSize::new(1280, 720), 33);
        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");

        assert_eq!(
            status.capture_runtime.state,
            VideoCaptureRuntimeState::Delivering
        );
        assert_eq!(status.capture_runtime.frames_delivered, 1);
        assert_eq!(
            status.capture_runtime.last_frame,
            Some(CapturedVideoFrame {
                sequence: 1,
                timestamp_ms: 33,
                size: ViewportSize::new(1280, 720),
            })
        );
        assert_eq!(status.encoding.output.frames_encoded, 0);
        assert_eq!(status.stats.frames_encoded, 0);
        assert_eq!(status.state, VideoStreamState::Starting);
    }

    #[test]
    fn macos_capture_runtime_failure_creates_retryable_capture_failure() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        runtime.fail_next_start("ScreenCaptureKit boundary failed");
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );

        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream records failure");

        assert_eq!(failed.state, VideoStreamState::Failed);
        assert_eq!(
            failed.capture_runtime.state,
            VideoCaptureRuntimeState::Failed
        );
        assert_eq!(
            failed.capture_runtime.message.as_deref(),
            Some("ScreenCaptureKit boundary failed")
        );
        assert_eq!(
            failed.failure.as_ref().map(|failure| &failure.kind),
            Some(&VideoStreamFailureKind::CaptureFailed)
        );
        assert_eq!(
            failed
                .failure
                .as_ref()
                .map(|failure| &failure.recovery.action),
            Some(&VideoStreamRecoveryAction::ReconnectStream)
        );
        assert!(failed.failure.as_ref().unwrap().recovery.retryable);
        assert_eq!(
            failed.failure_reason.as_deref(),
            Some("ScreenCaptureKit boundary failed")
        );
        assert_eq!(runtime.calls().starts.len(), 1);
    }

    #[test]
    fn macos_capture_runtime_failure_snapshot_becomes_stream_failure() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        runtime.deliver_frame(&stream.id, ViewportSize::new(1280, 720), 33);
        runtime.deny_permission(&stream.id, "Screen Recording permission is required");
        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");

        assert_eq!(status.state, VideoStreamState::Failed);
        assert_eq!(
            status.capture_runtime.state,
            VideoCaptureRuntimeState::PermissionDenied
        );
        assert_eq!(
            status.capture_runtime.message.as_deref(),
            Some("Screen Recording permission is required")
        );
        assert_eq!(status.capture_runtime.frames_delivered, 1);
        assert_eq!(
            status.failure.as_ref().map(|failure| &failure.kind),
            Some(&VideoStreamFailureKind::CaptureFailed)
        );
        assert_eq!(
            status.health.message.as_deref(),
            Some("Screen Recording permission is required")
        );
        assert_eq!(status.encoding.output.frames_encoded, 0);
    }

    #[test]
    fn macos_capture_runtime_failure_snapshot_blocks_negotiation() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");
        runtime.fail_stream(&stream.id, "ScreenCaptureKit stream failed");

        let error = stream_service
            .negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            })
            .expect_err("runtime failure blocks negotiation");

        assert_eq!(
            error,
            AppRelayError::InvalidRequest(
                "stream stream-1 is failed and must be reconnected before negotiation".to_string()
            )
        );
        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.state, VideoStreamState::Failed);
        assert_eq!(status.encoding.output.frames_encoded, 0);
    }

    #[test]
    fn macos_capture_runtime_failure_snapshot_reconnect_restarts_runtime() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");
        runtime.fail_stream(&stream.id, "ScreenCaptureKit stream failed");

        let reconnected = stream_service
            .reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: stream.id.clone(),
            })
            .expect("reconnect after runtime failure");

        assert_eq!(reconnected.state, VideoStreamState::Starting);
        assert_eq!(reconnected.failure, None);
        assert_eq!(
            reconnected.capture_runtime.state,
            VideoCaptureRuntimeState::Starting
        );
        assert_eq!(runtime.calls().starts.len(), 2);
    }

    #[test]
    fn macos_capture_runtime_failure_snapshot_skips_resize() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");
        runtime.fail_stream(&stream.id, "ScreenCaptureKit stream failed");

        stream_service.record_resize(&ResizeSessionRequest {
            session_id: session.id,
            viewport: ViewportSize::new(1440, 900),
        });

        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.state, VideoStreamState::Failed);
        assert_eq!(status.viewport, ViewportSize::new(1280, 720));
        assert_eq!(runtime.calls().resizes, Vec::new());
    }

    #[test]
    fn macos_capture_runtime_reconnect_retries_after_failure_and_clears_it() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        runtime.fail_next_start("ScreenCaptureKit boundary failed");
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream records failure");

        let reconnected = stream_service
            .reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: failed.id,
            })
            .expect("reconnect stream");

        assert_eq!(reconnected.state, VideoStreamState::Starting);
        assert_eq!(reconnected.failure, None);
        assert_eq!(reconnected.failure_reason, None);
        assert!(reconnected.health.healthy);
        assert_eq!(reconnected.stats.reconnect_attempts, 1);
        assert_eq!(runtime.calls().starts.len(), 2);
    }

    #[test]
    fn macos_capture_runtime_receives_resize_for_active_stream() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
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

        assert_eq!(
            runtime.calls().resizes,
            vec![MacosWindowCaptureResizeRequest {
                stream_id: stream.id.clone(),
                selected_window_id: "macos-window-session-1-88".to_string(),
                target_viewport: ViewportSize::new(1440, 900),
            }]
        );
        assert_eq!(
            stream_service
                .stream_status(&stream.id)
                .expect("stream status")
                .viewport,
            ViewportSize::new(1440, 900)
        );
    }

    #[test]
    fn macos_capture_runtime_does_not_receive_invalid_resize() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        stream_service.record_resize(&ResizeSessionRequest {
            session_id: session.id,
            viewport: ViewportSize::new(100, 100),
        });

        assert_eq!(runtime.calls().resizes, Vec::new());
    }

    #[test]
    fn macos_capture_runtime_does_not_resize_stopped_or_failed_streams() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        runtime.fail_next_start("ScreenCaptureKit boundary failed");
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start failed stream");
        assert_eq!(failed.state, VideoStreamState::Failed);
        let active = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start active stream");
        stream_service
            .stop_stream(StopVideoStreamRequest {
                stream_id: active.id,
            })
            .expect("stop stream");

        stream_service.record_resize(&ResizeSessionRequest {
            session_id: session.id,
            viewport: ViewportSize::new(1440, 900),
        });

        assert_eq!(runtime.calls().resizes, Vec::new());
    }

    #[test]
    fn macos_capture_runtime_resize_failure_marks_stream_failed() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        runtime.fail_next_resize("ScreenCaptureKit resize failed");
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
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
        assert_eq!(status.state, VideoStreamState::Failed);
        assert_eq!(
            status.failure.as_ref().map(|failure| &failure.kind),
            Some(&VideoStreamFailureKind::CaptureFailed)
        );
        assert_eq!(
            status
                .failure
                .as_ref()
                .map(|failure| &failure.recovery.action),
            Some(&VideoStreamRecoveryAction::ReconnectStream)
        );
        assert!(status.failure.as_ref().unwrap().recovery.retryable);
        assert_eq!(
            status.failure_reason.as_deref(),
            Some("ScreenCaptureKit resize failed")
        );
        assert_eq!(
            status.capture_runtime.message.as_deref(),
            Some("ScreenCaptureKit resize failed")
        );
        assert_eq!(runtime.calls().resizes.len(), 1);
        assert_eq!(runtime.calls().stops, vec![stream.id]);
    }

    #[test]
    fn macos_capture_runtime_stop_and_session_close_stop_active_streams() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: "macos-window-session-1-88".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            launch_intent: None,
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let runtime = FakeMacosWindowCaptureRuntime::new();
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::macos_selected_window_with_runtime(Arc::new(
                runtime.clone(),
            )),
        );
        let stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        let stopped = stream_service
            .stop_stream(StopVideoStreamRequest {
                stream_id: stream.id.clone(),
            })
            .expect("stop stream");
        assert_eq!(
            stopped.capture_runtime,
            VideoCaptureRuntimeStatus::default()
        );
        assert_eq!(runtime.snapshot(&stream.id), None);

        let second_stream = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start second stream");
        stream_service.record_session_closed(&session.id);

        assert_eq!(
            runtime.calls().stops,
            vec!["stream-1".to_string(), second_stream.id]
        );
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
    fn video_stream_service_lists_active_streams() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::fails_once("initial capture failed"),
        );

        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start failed stream");
        let streaming = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");
        let streaming = stream_service
            .negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: streaming.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            })
            .expect("negotiate stream");
        let stopped = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stopped stream");
        stream_service
            .stop_stream(StopVideoStreamRequest {
                stream_id: stopped.id,
            })
            .expect("stop stream");
        let starting = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start active stream");

        let active_streams = stream_service.active_streams();

        assert_eq!(
            active_streams
                .iter()
                .map(|stream| (&stream.id, &stream.state))
                .collect::<Vec<_>>(),
            vec![
                (&failed.id, &VideoStreamState::Failed),
                (&streaming.id, &VideoStreamState::Streaming),
                (&starting.id, &VideoStreamState::Starting),
            ]
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
                payload: Vec::new(),
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
                payload: Vec::new(),
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
    fn video_stream_service_marks_active_stream_failed_when_session_closes() {
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

        stream_service.record_session_closed(&session.id);

        let status = stream_service
            .stream_status(&stream.id)
            .expect("stream status");
        assert_eq!(status.state, VideoStreamState::Failed);
        assert_eq!(status.encoding.state, VideoEncodingPipelineState::Drained);
        assert_eq!(
            status.stats.frames_encoded,
            status.encoding.output.frames_encoded
        );
        assert_eq!(status.stats.bitrate_kbps, 0);
        assert_eq!(status.stats.latency_ms, 0);
        assert_eq!(
            status.failure.as_ref().map(|failure| &failure.kind),
            Some(&VideoStreamFailureKind::AppClosed)
        );
        assert_eq!(
            status
                .failure
                .as_ref()
                .map(|failure| &failure.recovery.action),
            Some(&VideoStreamRecoveryAction::RestartApplicationSession)
        );
        assert!(!status.failure.as_ref().unwrap().recovery.retryable);
    }

    #[test]
    fn video_stream_service_rejects_reconnect_after_session_close() {
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
        stream_service.record_session_closed(&session.id);

        assert_eq!(
            stream_service.reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: stream.id,
            }),
            Err(AppRelayError::InvalidRequest(
                "stream stream-1 cannot reconnect because its application session closed"
                    .to_string()
            ))
        );
    }

    #[test]
    fn video_stream_service_returns_actionable_capture_failure_state() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service =
            InMemoryVideoStreamService::new(WindowCaptureBackendService::FailingSelectedWindow {
                message: "capture backend failed".to_string(),
            });

        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        assert_eq!(failed.state, VideoStreamState::Failed);
        assert_eq!(failed.encoding.state, VideoEncodingPipelineState::Drained);
        assert!(!failed.health.healthy);
        assert_eq!(
            failed.failure.as_ref().map(|failure| &failure.kind),
            Some(&VideoStreamFailureKind::CaptureFailed)
        );
        assert_eq!(
            failed
                .failure
                .as_ref()
                .map(|failure| &failure.recovery.action),
            Some(&VideoStreamRecoveryAction::ReconnectStream)
        );
        assert!(failed.failure.as_ref().unwrap().recovery.retryable);
        assert_eq!(
            failed.failure_reason.as_deref(),
            Some("capture backend failed")
        );
        assert_eq!(
            failed.stats.frames_encoded,
            failed.encoding.output.frames_encoded
        );
    }

    #[test]
    fn video_stream_service_reconnect_keeps_retryable_capture_failure_when_recapture_fails() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service =
            InMemoryVideoStreamService::new(WindowCaptureBackendService::FailingSelectedWindow {
                message: "capture backend failed".to_string(),
            });
        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        let reconnected = stream_service
            .reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: failed.id,
            })
            .expect("reconnect stream");

        assert_eq!(reconnected.state, VideoStreamState::Failed);
        assert_eq!(
            reconnected.encoding.state,
            VideoEncodingPipelineState::Drained
        );
        assert_eq!(
            reconnected.failure.as_ref().map(|failure| &failure.kind),
            Some(&VideoStreamFailureKind::CaptureFailed)
        );
        assert_eq!(
            reconnected.failure_reason.as_deref(),
            Some("capture backend failed")
        );
        assert!(!reconnected.health.healthy);
        assert_eq!(reconnected.stats.bitrate_kbps, 0);
        assert_eq!(reconnected.stats.latency_ms, 0);
        assert_eq!(reconnected.stats.reconnect_attempts, 1);
        assert_eq!(
            stream_service.negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: reconnected.id,
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            }),
            Err(AppRelayError::InvalidRequest(
                "stream stream-1 is failed and must be reconnected before negotiation".to_string()
            ))
        );
    }

    #[test]
    fn video_stream_service_reconnect_clears_capture_failure_after_successful_recapture() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryVideoStreamService::new(
            WindowCaptureBackendService::fails_once("capture backend failed"),
        );
        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        let reconnected = stream_service
            .reconnect_stream(ReconnectVideoStreamRequest {
                stream_id: failed.id,
            })
            .expect("reconnect stream");

        assert_eq!(reconnected.state, VideoStreamState::Starting);
        assert_eq!(
            reconnected.encoding.state,
            VideoEncodingPipelineState::Configured
        );
        assert_eq!(reconnected.failure, None);
        assert_eq!(reconnected.failure_reason, None);
        assert!(reconnected.health.healthy);
        assert_eq!(
            reconnected.health.message.as_deref(),
            Some("reconnect requested")
        );
        assert_eq!(reconnected.stats.frames_encoded, 0);
        assert_eq!(reconnected.stats.bitrate_kbps, 0);
        assert_eq!(reconnected.stats.latency_ms, 0);
        assert_eq!(reconnected.stats.reconnect_attempts, 1);
        assert_eq!(
            reconnected.signaling.ice_candidates,
            vec![WebRtcIceCandidate {
                candidate: "candidate:apprelay stream-1 window-session-1 typ host".to_string(),
                sdp_mid: Some("video".to_string()),
                sdp_m_line_index: Some(0),
            }]
        );
    }

    #[test]
    fn video_stream_service_rejects_negotiation_while_failed() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service =
            InMemoryVideoStreamService::new(WindowCaptureBackendService::FailingSelectedWindow {
                message: "capture backend failed".to_string(),
            });
        let failed = stream_service
            .start_stream(
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &session,
            )
            .expect("start stream");

        assert_eq!(
            stream_service.negotiate_stream(NegotiateVideoStreamRequest {
                stream_id: failed.id,
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            }),
            Err(AppRelayError::InvalidRequest(
                "stream stream-1 is failed and must be reconnected before negotiation".to_string()
            ))
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
            Err(AppRelayError::unsupported(
                Platform::Linux,
                Feature::WindowVideoStream
            ))
        );
    }
}
