//! Shared protocol types for Swavan AppRelay.
//!
//! Phase 1 intentionally keeps these types transport-neutral. The server can
//! expose them over SSH-tunneled HTTP, WebSocket, gRPC, or another control
//! protocol without changing the domain model.

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

#[derive(Clone, Debug, Eq, PartialEq)]
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
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionState {
    Starting,
    Ready,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwavanError {
    UnsupportedPlatform {
        platform: Platform,
        feature: Feature,
    },
    ServiceUnavailable(String),
    InvalidRequest(String),
    PermissionDenied(String),
    NotFound(String),
}

impl SwavanError {
    pub fn unsupported(platform: Platform, feature: Feature) -> Self {
        Self::UnsupportedPlatform { platform, feature }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ControlAuth {
    token: String,
}

impl std::fmt::Debug for ControlAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlAuth")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl ControlAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlError {
    Unauthorized,
    Service(SwavanError),
}

impl From<SwavanError> for ControlError {
    fn from(error: SwavanError) -> Self {
        Self::Service(error)
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
    fn control_auth_keeps_token_private() {
        let auth = ControlAuth::new("secret");

        assert_eq!(auth.token(), "secret");
        assert_eq!(format!("{auth:?}"), "ControlAuth { token: \"<redacted>\" }");
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
}
