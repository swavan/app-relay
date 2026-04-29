use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use swavan_core::{
    ConnectionProfile, ConnectionProfileRepository, FileConnectionProfileRepository, ServerConfig,
};
use swavan_protocol::{
    AppIcon, ApplicationLaunch, ApplicationSession, ControlAuth, CreateSessionRequest, Feature,
    Platform, PlatformCapability, ResizeIntentStatus, ResizeSessionRequest, SelectedWindow,
    SessionState, ViewportSize, WindowResizeIntent,
};
use swavan_server::{ServerControlPlane, ServerServices};

static CONTROL_PLANE: OnceLock<Mutex<ServerControlPlane>> = OnceLock::new();

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfileDto {
    pub id: String,
    pub label: String,
    pub ssh_user: String,
    pub ssh_host: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub auth_token: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatusDto {
    pub service: String,
    pub healthy: bool,
    pub version: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDto {
    pub platform: String,
    pub feature: String,
    pub supported: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummaryDto {
    pub id: String,
    pub name: String,
    pub icon: Option<AppIconDto>,
    pub launch: Option<ApplicationLaunchDto>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconDto {
    pub mime_type: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationLaunchDto {
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportSizeDto {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequestDto {
    pub application_id: String,
    pub viewport: ViewportSizeDto,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeSessionRequestDto {
    pub session_id: String,
    pub viewport: ViewportSizeDto,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedWindowDto {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSessionDto {
    pub id: String,
    pub application_id: String,
    pub selected_window: SelectedWindowDto,
    pub viewport: ViewportSizeDto,
    pub resize_intent: Option<WindowResizeIntentDto>,
    pub state: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowResizeIntentDto {
    pub session_id: String,
    pub selected_window_id: String,
    pub viewport: ViewportSizeDto,
    pub status: String,
}

#[tauri::command]
fn list_connection_profiles() -> Result<Vec<ConnectionProfileDto>, String> {
    profile_repository()
        .list()
        .map(|profiles| profiles.into_iter().map(Into::into).collect())
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn save_connection_profile(profile: ConnectionProfileDto) -> Result<(), String> {
    profile_repository()
        .save(profile.into())
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn remove_connection_profile(id: String) -> Result<(), String> {
    profile_repository()
        .remove(&id)
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn server_health(auth_token: String) -> Result<HealthStatusDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .health(&ControlAuth::new(auth_token))
            .map(Into::into)
    })
}

#[tauri::command]
fn server_capabilities(auth_token: String) -> Result<Vec<CapabilityDto>, String> {
    with_control_plane(|control_plane| {
        control_plane
            .capabilities(&ControlAuth::new(auth_token))
            .map(|capabilities| capabilities.into_iter().map(Into::into).collect())
    })
}

#[tauri::command]
fn server_applications(auth_token: String) -> Result<Vec<AppSummaryDto>, String> {
    with_control_plane(|control_plane| {
        control_plane
            .available_applications(&ControlAuth::new(auth_token))
            .map(|applications| applications.into_iter().map(Into::into).collect())
    })
}

#[tauri::command]
fn create_application_session(
    auth_token: String,
    request: CreateSessionRequestDto,
) -> Result<ApplicationSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .create_session(&ControlAuth::new(auth_token), request.into())
            .map(Into::into)
    })
}

#[tauri::command]
fn resize_application_session(
    auth_token: String,
    request: ResizeSessionRequestDto,
) -> Result<ApplicationSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .resize_session(&ControlAuth::new(auth_token), request.into())
            .map(Into::into)
    })
}

#[tauri::command]
fn close_application_session(
    auth_token: String,
    session_id: String,
) -> Result<ApplicationSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .close_session(&ControlAuth::new(auth_token), &session_id)
            .map(Into::into)
    })
}

fn profile_repository() -> FileConnectionProfileRepository {
    FileConnectionProfileRepository::new(data_dir().join("connection-profiles.tsv"))
}

fn with_control_plane<T>(
    action: impl FnOnce(&mut ServerControlPlane) -> swavan_protocol::ControlResult<T>,
) -> Result<T, String> {
    let mut control_plane = control_plane()
        .lock()
        .map_err(|_| "control plane lock poisoned".to_string())?;

    action(&mut control_plane).map_err(|error| format!("{error:?}"))
}

fn control_plane() -> &'static Mutex<ServerControlPlane> {
    CONTROL_PLANE.get_or_init(|| Mutex::new(new_control_plane()))
}

fn new_control_plane() -> ServerControlPlane {
    let auth_token =
        std::env::var("SWAVAN_APP_RELAY_TOKEN").unwrap_or_else(|_| "local-dev-token".to_string());

    ServerControlPlane::new(
        ServerServices::for_current_platform(),
        ServerConfig::local(auth_token),
    )
}

fn data_dir() -> PathBuf {
    std::env::var_os("SWAVAN_APP_RELAY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("swavan-app-relay"))
}

impl From<ConnectionProfileDto> for ConnectionProfile {
    fn from(profile: ConnectionProfileDto) -> Self {
        Self {
            id: profile.id,
            label: profile.label,
            ssh_user: profile.ssh_user,
            ssh_host: profile.ssh_host,
            local_port: profile.local_port,
            remote_port: profile.remote_port,
            auth_token: profile.auth_token,
        }
    }
}

impl From<ConnectionProfile> for ConnectionProfileDto {
    fn from(profile: ConnectionProfile) -> Self {
        Self {
            id: profile.id,
            label: profile.label,
            ssh_user: profile.ssh_user,
            ssh_host: profile.ssh_host,
            local_port: profile.local_port,
            remote_port: profile.remote_port,
            auth_token: profile.auth_token,
        }
    }
}

impl From<swavan_protocol::HealthStatus> for HealthStatusDto {
    fn from(status: swavan_protocol::HealthStatus) -> Self {
        Self {
            service: status.service,
            healthy: status.healthy,
            version: status.version,
        }
    }
}

impl From<PlatformCapability> for CapabilityDto {
    fn from(capability: PlatformCapability) -> Self {
        Self {
            platform: platform_name(capability.platform).to_string(),
            feature: feature_name(&capability.feature).to_string(),
            supported: capability.supported,
            reason: capability.reason,
        }
    }
}

impl From<swavan_protocol::ApplicationSummary> for AppSummaryDto {
    fn from(application: swavan_protocol::ApplicationSummary) -> Self {
        Self {
            id: application.id,
            name: application.name,
            icon: application.icon.map(Into::into),
            launch: application.launch.map(Into::into),
        }
    }
}

impl From<AppIcon> for AppIconDto {
    fn from(icon: AppIcon) -> Self {
        Self {
            mime_type: icon.mime_type,
            source: icon.source,
        }
    }
}

impl From<ApplicationLaunch> for ApplicationLaunchDto {
    fn from(launch: ApplicationLaunch) -> Self {
        match launch {
            ApplicationLaunch::DesktopCommand { command } => Self {
                kind: "desktopCommand".to_string(),
                value: command,
            },
            ApplicationLaunch::MacosBundle { bundle_path } => Self {
                kind: "macosBundle".to_string(),
                value: bundle_path,
            },
        }
    }
}

impl From<ViewportSizeDto> for ViewportSize {
    fn from(viewport: ViewportSizeDto) -> Self {
        Self::new(viewport.width, viewport.height)
    }
}

impl From<ViewportSize> for ViewportSizeDto {
    fn from(viewport: ViewportSize) -> Self {
        Self {
            width: viewport.width,
            height: viewport.height,
        }
    }
}

impl From<CreateSessionRequestDto> for CreateSessionRequest {
    fn from(request: CreateSessionRequestDto) -> Self {
        Self {
            application_id: request.application_id,
            viewport: request.viewport.into(),
        }
    }
}

impl From<ResizeSessionRequestDto> for ResizeSessionRequest {
    fn from(request: ResizeSessionRequestDto) -> Self {
        Self {
            session_id: request.session_id,
            viewport: request.viewport.into(),
        }
    }
}

impl From<SelectedWindow> for SelectedWindowDto {
    fn from(window: SelectedWindow) -> Self {
        Self {
            id: window.id,
            title: window.title,
        }
    }
}

impl From<ApplicationSession> for ApplicationSessionDto {
    fn from(session: ApplicationSession) -> Self {
        Self {
            id: session.id,
            application_id: session.application_id,
            selected_window: session.selected_window.into(),
            viewport: session.viewport.into(),
            resize_intent: session.resize_intent.map(Into::into),
            state: session_state_name(&session.state).to_string(),
        }
    }
}

impl From<WindowResizeIntent> for WindowResizeIntentDto {
    fn from(intent: WindowResizeIntent) -> Self {
        Self {
            session_id: intent.session_id,
            selected_window_id: intent.selected_window_id,
            viewport: intent.viewport.into(),
            status: resize_intent_status_name(&intent.status).to_string(),
        }
    }
}

fn resize_intent_status_name(status: &ResizeIntentStatus) -> &'static str {
    match status {
        ResizeIntentStatus::Recorded => "recorded",
        ResizeIntentStatus::Applied => "applied",
        ResizeIntentStatus::Unsupported => "unsupported",
    }
}

fn session_state_name(state: &SessionState) -> &'static str {
    match state {
        SessionState::Starting => "starting",
        SessionState::Ready => "ready",
        SessionState::Closed => "closed",
    }
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Android => "android",
        Platform::Ios => "ios",
        Platform::Linux => "linux",
        Platform::Macos => "macos",
        Platform::Windows => "windows",
        Platform::Unknown => "unknown",
    }
}

fn feature_name(feature: &Feature) -> &'static str {
    match feature {
        Feature::AppDiscovery => "appDiscovery",
        Feature::WindowResize => "windowResize",
        Feature::WindowVideoStream => "windowVideoStream",
        Feature::SystemAudioStream => "systemAudioStream",
        Feature::ClientMicrophoneInput => "clientMicrophoneInput",
        Feature::KeyboardInput => "keyboardInput",
        Feature::MouseInput => "mouseInput",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_connection_profiles,
            save_connection_profile,
            remove_connection_profile,
            server_health,
            server_capabilities,
            server_applications,
            create_application_session,
            resize_application_session,
            close_application_session
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Swavan AppRelay client");
}
