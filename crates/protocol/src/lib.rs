//! Shared protocol types for AppRelay.
//!
//! Phase 1 intentionally keeps these types transport-neutral. The server can
//! expose them over SSH-tunneled HTTP, WebSocket, gRPC, or another control
//! protocol without changing the domain model.

mod audio_stream;
mod input;
mod video_stream;

pub use audio_stream::{
    AudioBackendContract, AudioBackendFailure, AudioBackendFailureKind, AudioBackendKind,
    AudioBackendLeg, AudioBackendMediaStats, AudioBackendReadiness, AudioBackendStatus,
    AudioCapability, AudioCaptureScope, AudioDeviceSelection, AudioMuteState, AudioSource,
    AudioStreamCapabilities, AudioStreamHealth, AudioStreamSession, AudioStreamState,
    AudioStreamStats, MicrophoneInjectionState, MicrophoneMode, StartAudioStreamRequest,
    StopAudioStreamRequest, UpdateAudioStreamRequest,
};
pub use input::{
    ButtonAction, ClientPoint, ForwardInputRequest, InputBackendKind, InputDelivery,
    InputDeliveryStatus, InputEvent, KeyAction, KeyModifiers, MappedInputEvent, PointerButton,
    ServerPoint,
};
pub use video_stream::{
    EncodedVideoFrame, NegotiateVideoStreamRequest, ReconnectVideoStreamRequest,
    StartVideoStreamRequest, StopVideoStreamRequest, VideoCaptureScope, VideoCaptureSource,
    VideoCodec, VideoEncodingContract, VideoEncodingOutput, VideoEncodingPipeline,
    VideoEncodingPipelineState, VideoEncodingTarget, VideoHardwareAcceleration, VideoPixelFormat,
    VideoResolutionAdaptation, VideoResolutionAdaptationReason, VideoResolutionLimits,
    VideoStreamFailure, VideoStreamFailureKind, VideoStreamHealth, VideoStreamNegotiationState,
    VideoStreamRecovery, VideoStreamRecoveryAction, VideoStreamSession, VideoStreamSignaling,
    VideoStreamSignalingKind, VideoStreamState, VideoStreamStats, WebRtcIceCandidate,
    WebRtcSdpType, WebRtcSessionDescription,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthStatus {
    pub service: String,
    pub healthy: bool,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerVersion {
    pub service: String,
    pub version: String,
    pub platform: Platform,
}

impl ServerVersion {
    pub fn new(service: impl Into<String>, version: impl Into<String>, platform: Platform) -> Self {
        Self {
            service: service.into(),
            version: version.into(),
            platform,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeartbeatStatus {
    pub healthy: bool,
    pub sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticsBundle {
    pub format_version: u16,
    pub telemetry_enabled: bool,
    pub secrets_redacted: bool,
    pub service: String,
    pub version: String,
    pub platform: Platform,
    pub bind_address: String,
    pub control_port: u16,
    pub heartbeat_interval_millis: u64,
    pub supported_capabilities: usize,
    pub total_capabilities: usize,
    pub active_sessions: usize,
}

impl HealthStatus {
    pub fn healthy(service: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            healthy: true,
            version: version.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Android,
    Ios,
    Linux,
    Macos,
    Windows,
    Unknown,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unknown
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Android => "Android",
            Self::Ios => "iOS",
            Self::Linux => "Linux",
            Self::Macos => "macOS",
            Self::Windows => "Windows",
            Self::Unknown => "this platform",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Feature {
    AppDiscovery,
    ApplicationLaunch,
    WindowResize,
    WindowVideoStream,
    SystemAudioStream,
    ClientMicrophoneInput,
    KeyboardInput,
    MouseInput,
}

impl Feature {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AppDiscovery => "application discovery",
            Self::ApplicationLaunch => "application launch",
            Self::WindowResize => "window resize",
            Self::WindowVideoStream => "window video streaming",
            Self::SystemAudioStream => "system audio streaming",
            Self::ClientMicrophoneInput => "client microphone input",
            Self::KeyboardInput => "keyboard input",
            Self::MouseInput => "mouse input",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformCapability {
    pub platform: Platform,
    pub feature: Feature,
    pub supported: bool,
    pub reason: Option<String>,
}

impl PlatformCapability {
    pub fn supported(platform: Platform, feature: Feature) -> Self {
        Self {
            platform,
            feature,
            supported: true,
            reason: None,
        }
    }

    pub fn supported_with_reason(
        platform: Platform,
        feature: Feature,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            feature,
            supported: true,
            reason: Some(reason.into()),
        }
    }

    pub fn unsupported(platform: Platform, feature: Feature, reason: impl Into<String>) -> Self {
        Self {
            platform,
            feature,
            supported: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationSummary {
    pub id: String,
    pub name: String,
    pub icon: Option<AppIcon>,
    pub launch: Option<ApplicationLaunch>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppIcon {
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationLaunch {
    DesktopCommand { command: String },
    MacosBundle { bundle_path: String },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportSize {
    pub width: u32,
    pub height: u32,
}

impl ViewportSize {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionRequest {
    pub application_id: String,
    pub viewport: ViewportSize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResizeSessionRequest {
    pub session_id: String,
    pub viewport: ViewportSize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowResizeIntent {
    pub session_id: String,
    pub selected_window_id: String,
    pub viewport: ViewportSize,
    pub status: ResizeIntentStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResizeIntentStatus {
    Recorded,
    Applied,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationSession {
    pub id: String,
    pub application_id: String,
    pub selected_window: SelectedWindow,
    pub launch_intent: Option<ApplicationLaunchIntent>,
    pub viewport: ViewportSize,
    pub resize_intent: Option<WindowResizeIntent>,
    pub state: SessionState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationLaunchIntent {
    pub session_id: String,
    pub application_id: String,
    pub launch: Option<ApplicationLaunch>,
    pub status: LaunchIntentStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchIntentStatus {
    Recorded,
    Attached,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedWindow {
    pub id: String,
    pub application_id: String,
    pub title: String,
    pub selection_method: WindowSelectionMethod,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowSelectionMethod {
    LaunchIntent,
    ExistingWindow,
    NativeWindow,
    Synthetic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionState {
    Starting,
    Ready,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppRelayError {
    UnsupportedPlatform {
        platform: Platform,
        feature: Feature,
    },
    ServiceUnavailable(String),
    InvalidRequest(String),
    PermissionDenied(String),
    NotFound(String),
}

impl AppRelayError {
    pub fn unsupported(platform: Platform, feature: Feature) -> Self {
        Self::UnsupportedPlatform { platform, feature }
    }

    pub fn user_message(&self) -> String {
        match self {
            Self::UnsupportedPlatform { platform, feature } => {
                format!("{} is unsupported on {}", feature.label(), platform.label())
            }
            Self::ServiceUnavailable(message)
            | Self::InvalidRequest(message)
            | Self::PermissionDenied(message)
            | Self::NotFound(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for AppRelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.user_message())
    }
}

impl std::error::Error for AppRelayError {}

#[derive(Clone, Eq, PartialEq)]
pub struct ControlAuth {
    token: String,
    client_id: Option<String>,
}

impl std::fmt::Debug for ControlAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlAuth")
            .field("token", &"<redacted>")
            .field("client_id", &self.client_id)
            .finish()
    }
}

impl ControlAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            client_id: None,
        }
    }

    pub fn with_client_id(token: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            client_id: Some(client_id.into()),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlClientIdentity {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingRequest {
    pub client: ControlClientIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PairingRequestStatus {
    PendingUserApproval,
    Approved,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingPairing {
    pub request_id: String,
    pub client: ControlClientIdentity,
    pub status: PairingRequestStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovePairingRequest {
    pub request_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlError {
    Unauthorized,
    Service(AppRelayError),
}

impl From<AppRelayError> for ControlError {
    fn from(error: AppRelayError) -> Self {
        Self::Service(error)
    }
}

impl ControlError {
    pub fn user_message(&self) -> String {
        match self {
            Self::Unauthorized => "request is unauthorized".to_string(),
            Self::Service(error) => error.user_message(),
        }
    }
}

pub type ControlResult<T> = Result<T, ControlError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_marks_service_healthy() {
        let status = HealthStatus::healthy("server", "0.1.0");

        assert_eq!(status.service, "server");
        assert!(status.healthy);
        assert_eq!(status.version, "0.1.0");
    }

    #[test]
    fn unsupported_capability_keeps_reason() {
        let capability = PlatformCapability::unsupported(
            Platform::Linux,
            Feature::WindowVideoStream,
            "capture backend not implemented",
        );

        assert!(!capability.supported);
        assert_eq!(
            capability.reason.as_deref(),
            Some("capture backend not implemented")
        );
    }

    #[test]
    fn unsupported_errors_expose_user_facing_messages() {
        let error = AppRelayError::unsupported(Platform::Macos, Feature::WindowVideoStream);

        assert_eq!(
            error.user_message(),
            "window video streaming is unsupported on macOS"
        );
        assert_eq!(error.to_string(), error.user_message());
    }

    #[test]
    fn control_errors_expose_user_facing_messages() {
        assert_eq!(
            ControlError::Unauthorized.user_message(),
            "request is unauthorized"
        );
        assert_eq!(
            ControlError::Service(AppRelayError::unsupported(
                Platform::Windows,
                Feature::AppDiscovery
            ))
            .user_message(),
            "application discovery is unsupported on Windows"
        );
    }

    #[test]
    fn control_auth_keeps_token_private() {
        let auth = ControlAuth::new("secret");

        assert_eq!(auth.token(), "secret");
        assert_eq!(auth.client_id(), None);
        assert_eq!(
            format!("{auth:?}"),
            "ControlAuth { token: \"<redacted>\", client_id: None }"
        );
    }

    #[test]
    fn control_auth_can_carry_client_identity_separately_from_token() {
        let auth = ControlAuth::with_client_id("secret", "client-1");

        assert_eq!(auth.token(), "secret");
        assert_eq!(auth.client_id(), Some("client-1"));
    }

    #[test]
    fn viewport_size_keeps_requested_dimensions() {
        assert_eq!(
            ViewportSize::new(1280, 720),
            ViewportSize {
                width: 1280,
                height: 720,
            }
        );
    }

    #[test]
    fn diagnostics_bundle_keeps_redaction_and_telemetry_flags_explicit() {
        let bundle = DiagnosticsBundle {
            format_version: 1,
            telemetry_enabled: false,
            secrets_redacted: true,
            service: "apprelay-server".to_string(),
            version: "0.1.0".to_string(),
            platform: Platform::Linux,
            bind_address: "127.0.0.1".to_string(),
            control_port: 7676,
            heartbeat_interval_millis: 5_000,
            supported_capabilities: 4,
            total_capabilities: 8,
            active_sessions: 0,
        };

        assert!(!bundle.telemetry_enabled);
        assert!(bundle.secrets_redacted);
        assert_eq!(bundle.format_version, 1);
    }
}
