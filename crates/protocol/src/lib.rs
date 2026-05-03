//! Shared protocol types for AppRelay.
//!
//! Phase 1 intentionally keeps these types transport-neutral. The server can
//! expose them over SSH-tunneled HTTP, WebSocket, gRPC, or another control
//! protocol without changing the domain model.

mod audio_stream;
mod input;
mod signaling;
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
    ActiveInputFocus, ButtonAction, ClientPoint, ForwardInputRequest, InputBackendKind,
    InputDelivery, InputDeliveryStatus, InputEvent, KeyAction, KeyModifiers, MappedInputEvent,
    PointerButton, ServerPoint,
};
pub use signaling::{
    IceCandidatePayload, PollSignalingRequest, SdpRole, SignalingDirection, SignalingEnvelope,
    SignalingMessage, SignalingPoll, SignalingSubmitAck, SubmitSignalingRequest,
    MAX_SIGNALING_PAYLOAD_BASE64_BYTES, MAX_SIGNALING_PAYLOAD_DECODED_BYTES,
};
pub use video_stream::{
    CapturedVideoFrame, EncodedVideoFrame, NegotiateVideoStreamRequest,
    ReconnectVideoStreamRequest, StartVideoStreamRequest, StopVideoStreamRequest,
    VideoCaptureRuntimeState, VideoCaptureRuntimeStatus, VideoCaptureScope, VideoCaptureSource,
    VideoCodec, VideoEncodingContract, VideoEncodingOutput, VideoEncodingPipeline,
    VideoEncodingPipelineState, VideoEncodingTarget, VideoHardwareAcceleration, VideoPixelFormat,
    VideoResolutionAdaptation, VideoResolutionAdaptationReason, VideoResolutionLimits,
    VideoStreamFailure, VideoStreamFailureKind, VideoStreamHealth, VideoStreamNegotiationState,
    VideoStreamRecovery, VideoStreamRecoveryAction, VideoStreamSession, VideoStreamSignaling,
    VideoStreamSignalingKind, VideoStreamState, VideoStreamStats, WebRtcIceCandidate,
    WebRtcSdpType, WebRtcSessionDescription,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSummary {
    pub id: String,
    pub name: String,
    pub icon: Option<AppIcon>,
    pub launch: Option<ApplicationLaunch>,
}

/// Wire-shaped application icon. The on-the-wire representation is
/// `{ mimeType, dataUrl, source }`; the raw `bytes` are kept private to the
/// protocol crate because they only exist to be encoded into the data URL.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIcon {
    pub mime_type: String,
    pub data_url: Option<String>,
    pub source: Option<String>,
}

impl AppIcon {
    /// Build an [`AppIcon`] from raw image bytes, performing the same
    /// `bytes → data URL` conversion (including ICNS → embedded PNG
    /// extraction) that the Tauri shell used to apply at the wire boundary.
    /// Empty `bytes` produces an icon whose `data_url` is `None` (e.g. for
    /// XDG icon-theme name references).
    pub fn from_bytes(
        mime_type: impl Into<String>,
        bytes: Vec<u8>,
        source: Option<String>,
    ) -> Self {
        let mime_type = mime_type.into();
        let (mime_type, data_url) = encode_icon(&mime_type, &bytes);
        Self {
            mime_type,
            data_url,
            source,
        }
    }
}

fn encode_icon(mime_type: &str, bytes: &[u8]) -> (String, Option<String>) {
    if bytes.is_empty() {
        return (mime_type.to_string(), None);
    }

    if mime_type.eq_ignore_ascii_case("image/icns") {
        if let Some(png_bytes) = extract_icns_png_payload(bytes) {
            return (
                "image/png".to_string(),
                Some(format!(
                    "data:image/png;base64,{}",
                    base64_encode(png_bytes)
                )),
            );
        }
    }

    (
        mime_type.to_string(),
        Some(format!(
            "data:{};base64,{}",
            mime_type,
            base64_encode(bytes)
        )),
    )
}

fn extract_icns_png_payload(bytes: &[u8]) -> Option<&[u8]> {
    const ICNS_HEADER_LEN: usize = 8;
    const CHUNK_HEADER_LEN: usize = 8;
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    if bytes.len() < ICNS_HEADER_LEN || &bytes[..4] != b"icns" {
        return None;
    }

    let declared_len = u32::from_be_bytes(bytes[4..8].try_into().ok()?) as usize;
    if declared_len < ICNS_HEADER_LEN || declared_len > bytes.len() {
        return None;
    }

    let mut png_payload = None;
    let mut offset = ICNS_HEADER_LEN;
    while offset < declared_len {
        if declared_len - offset < CHUNK_HEADER_LEN {
            return None;
        }

        let chunk_len = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let chunk_end = offset.checked_add(chunk_len)?;
        if chunk_len < CHUNK_HEADER_LEN || chunk_end > declared_len {
            return None;
        }

        let payload_start = offset.checked_add(CHUNK_HEADER_LEN)?;
        let payload = &bytes[payload_start..chunk_end];
        if png_payload.is_none() && payload.starts_with(PNG_SIGNATURE) {
            png_payload = Some(payload);
        }

        offset = chunk_end;
    }

    png_payload
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);

        encoded.push(ALPHABET[(first >> 2) as usize] as char);
        encoded.push(ALPHABET[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(ALPHABET[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(ALPHABET[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
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

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub application_id: String,
    pub viewport: ViewportSize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeSessionRequest {
    pub session_id: String,
    pub viewport: ViewportSize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowResizeIntent {
    pub session_id: String,
    pub selected_window_id: String,
    pub viewport: ViewportSize,
    pub status: ResizeIntentStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResizeIntentStatus {
    Recorded,
    Applied,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSession {
    pub id: String,
    pub application_id: String,
    pub selected_window: SelectedWindow,
    pub launch_intent: Option<ApplicationLaunchIntent>,
    pub viewport: ViewportSize,
    pub resize_intent: Option<WindowResizeIntent>,
    pub state: SessionState,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationLaunchIntent {
    pub session_id: String,
    pub application_id: String,
    pub launch: Option<ApplicationLaunch>,
    pub status: LaunchIntentStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchIntentStatus {
    Recorded,
    Attached,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedWindow {
    pub id: String,
    pub application_id: String,
    pub title: String,
    pub selection_method: WindowSelectionMethod,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowSelectionMethod {
    LaunchIntent,
    ExistingWindow,
    NativeWindow,
    Synthetic,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
pub struct RevokeClientRequest {
    pub client_id: String,
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
    fn app_icon_from_bytes_encodes_png_payload_inline() {
        let icon = AppIcon::from_bytes("image/png", vec![0x89, 0x50, 0x4e, 0x47], None);

        assert_eq!(icon.mime_type, "image/png");
        assert_eq!(
            icon.data_url.as_deref(),
            Some("data:image/png;base64,iVBORw==")
        );
    }

    #[test]
    fn app_icon_from_bytes_extracts_png_from_icns_payload() {
        let png_bytes = b"\x89PNG\r\n\x1a\npng payload";
        let mut icns_bytes = Vec::new();
        icns_bytes.extend_from_slice(b"icns");
        icns_bytes.extend_from_slice(&((8 + 8 + png_bytes.len()) as u32).to_be_bytes());
        icns_bytes.extend_from_slice(b"ic10");
        icns_bytes.extend_from_slice(&((8 + png_bytes.len()) as u32).to_be_bytes());
        icns_bytes.extend_from_slice(png_bytes);

        let icon = AppIcon::from_bytes("image/icns", icns_bytes, None);

        assert_eq!(icon.mime_type, "image/png");
        assert_eq!(
            icon.data_url,
            Some(format!(
                "data:image/png;base64,{}",
                base64_encode(png_bytes)
            ))
        );
    }

    #[test]
    fn app_icon_from_bytes_falls_back_to_icns_for_malformed_or_no_png_payload() {
        let malformed = AppIcon::from_bytes("image/icns", b"icns\0\0\0\x20bad".to_vec(), None);
        assert_eq!(malformed.mime_type, "image/icns");
        assert_eq!(
            malformed.data_url,
            Some("data:image/icns;base64,aWNucwAAACBiYWQ=".to_string())
        );

        let no_png_bytes = b"icns\0\0\0\x10TOC \0\0\0\x08".to_vec();
        let no_png = AppIcon::from_bytes("image/icns", no_png_bytes.clone(), None);
        assert_eq!(no_png.mime_type, "image/icns");
        assert_eq!(
            no_png.data_url,
            Some(format!(
                "data:image/icns;base64,{}",
                base64_encode(&no_png_bytes)
            ))
        );
    }

    #[test]
    fn app_icon_from_bytes_falls_back_to_icns_when_chunk_length_overflows() {
        let bytes = b"icns\0\0\0\x10ic10\xff\xff\xff\xff".to_vec();
        let icon = AppIcon::from_bytes("image/icns", bytes.clone(), None);

        assert_eq!(icon.mime_type, "image/icns");
        assert_eq!(
            icon.data_url,
            Some(format!("data:image/icns;base64,{}", base64_encode(&bytes)))
        );
    }

    #[test]
    fn app_icon_from_bytes_omits_data_url_for_empty_bytes() {
        let icon = AppIcon::from_bytes(
            "application/x-icon-theme-name",
            Vec::new(),
            Some("utilities-terminal".to_string()),
        );

        assert_eq!(icon.data_url, None);
        assert_eq!(icon.mime_type, "application/x-icon-theme-name");
        assert_eq!(icon.source.as_deref(), Some("utilities-terminal"));
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
