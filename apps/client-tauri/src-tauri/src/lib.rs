use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

mod audio_stream;
mod signaling;
mod video_stream;

use apprelay_core::{
    ApplicationPermission, ApplicationPermissionRepository, AuthorizedClient, ConnectionProfile,
    ConnectionProfileRepository, FileApplicationPermissionRepository,
    FileConnectionProfileRepository, FileEventSink, ServerConfig,
};
use apprelay_protocol::{
    ActiveInputFocus, AppIcon, AppRelayError, ApplicationLaunch, ApplicationLaunchIntent,
    ApplicationSession, ControlAuth, CreateSessionRequest, Feature, ForwardInputRequest,
    InputDelivery,
    LaunchIntentStatus, Platform, PlatformCapability, ResizeIntentStatus, ResizeSessionRequest,
    SelectedWindow, SessionState, ViewportSize, WindowResizeIntent, WindowSelectionMethod,
};
use apprelay_server::{ServerControlPlane, ServerServices};

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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPermissionDto {
    pub application_id: String,
    pub label: String,
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
    pub data_url: Option<String>,
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
    pub application_id: String,
    pub title: String,
    pub selection_method: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSessionDto {
    pub id: String,
    pub application_id: String,
    pub selected_window: SelectedWindowDto,
    pub launch_intent: Option<ApplicationLaunchIntentDto>,
    pub viewport: ViewportSizeDto,
    pub resize_intent: Option<WindowResizeIntentDto>,
    pub state: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationLaunchIntentDto {
    pub session_id: String,
    pub application_id: String,
    pub launch: Option<ApplicationLaunchDto>,
    pub status: String,
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
fn list_application_permissions() -> Result<Vec<ApplicationPermissionDto>, String> {
    permission_repository()
        .list()
        .map(|permissions| permissions.into_iter().map(Into::into).collect())
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn save_application_permission(permission: ApplicationPermissionDto) -> Result<(), String> {
    permission_repository()
        .save(permission.into())
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn remove_application_permission(application_id: String) -> Result<(), String> {
    permission_repository()
        .remove(&application_id)
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
fn active_application_sessions(
    auth_token: String,
    client_id: String,
) -> Result<Vec<ApplicationSessionDto>, String> {
    with_control_plane(|control_plane| {
        control_plane
            .active_sessions(&paired_auth(auth_token, client_id))
            .map(|sessions| sessions.into_iter().map(Into::into).collect())
    })
}

#[tauri::command]
fn create_application_session(
    auth_token: String,
    client_id: String,
    request: CreateSessionRequestDto,
) -> Result<ApplicationSessionDto, String> {
    ensure_application_allowed(&request.application_id)?;

    with_control_plane(|control_plane| {
        control_plane
            .create_session(&paired_auth(auth_token, client_id), request.into())
            .map(Into::into)
    })
}

#[tauri::command]
fn resize_application_session(
    auth_token: String,
    client_id: String,
    request: ResizeSessionRequestDto,
) -> Result<ApplicationSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .resize_session(&paired_auth(auth_token, client_id), request.into())
            .map(Into::into)
    })
}

#[tauri::command]
fn close_application_session(
    auth_token: String,
    client_id: String,
    session_id: String,
) -> Result<ApplicationSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .close_session(&paired_auth(auth_token, client_id), &session_id)
            .map(Into::into)
    })
}

#[tauri::command]
fn forward_input(
    auth_token: String,
    client_id: String,
    request: ForwardInputRequest,
) -> Result<InputDelivery, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.forward_input_with_audit(&paired_auth(auth_token, client_id), request, events)
    })
}

#[tauri::command]
fn active_input_focus(
    auth_token: String,
    client_id: String,
) -> Result<Option<ActiveInputFocus>, String> {
    with_control_plane(|control_plane| {
        control_plane.active_input_focus(&paired_auth(auth_token, client_id))
    })
}

fn profile_repository() -> FileConnectionProfileRepository {
    FileConnectionProfileRepository::new(data_dir().join("connection-profiles.tsv"))
}

fn permission_repository() -> FileApplicationPermissionRepository {
    FileApplicationPermissionRepository::new(data_dir().join("application-permissions.tsv"))
}

fn ensure_application_allowed(application_id: &str) -> Result<(), String> {
    let permissions = permission_repository()
        .list()
        .map_err(|error| format!("{error:?}"))?;

    if permissions
        .iter()
        .any(|permission| permission.application_id == application_id)
    {
        Ok(())
    } else {
        Err(format!(
            "{:?}",
            AppRelayError::PermissionDenied(format!("application {application_id} is not allowed"))
        ))
    }
}

pub(crate) fn with_control_plane<T>(
    action: impl FnOnce(&mut ServerControlPlane) -> apprelay_protocol::ControlResult<T>,
) -> Result<T, String> {
    let mut control_plane = control_plane()
        .lock()
        .map_err(|_| "control plane lock poisoned".to_string())?;

    action(&mut control_plane).map_err(|error| format!("{error:?}"))
}

pub(crate) fn with_control_plane_events<T>(
    action: impl FnOnce(
        &mut ServerControlPlane,
        &mut FileEventSink,
    ) -> apprelay_protocol::ControlResult<T>,
) -> Result<T, String> {
    let mut control_plane = control_plane()
        .lock()
        .map_err(|_| "control plane lock poisoned".to_string())?;
    let mut events = FileEventSink::new(data_dir().join("client-events.log"));

    action(&mut control_plane, &mut events).map_err(|error| format!("{error:?}"))
}

fn control_plane() -> &'static Mutex<ServerControlPlane> {
    CONTROL_PLANE.get_or_init(|| Mutex::new(new_control_plane()))
}

fn new_control_plane() -> ServerControlPlane {
    let auth_token =
        std::env::var("APPRELAY_TOKEN").unwrap_or_else(|_| "local-dev-token".to_string());
    let mut config = ServerConfig::local(auth_token);
    config.authorized_clients = local_authorized_clients();

    ServerControlPlane::new(ServerServices::for_current_platform(), config)
}

fn paired_auth(auth_token: String, client_id: String) -> ControlAuth {
    ControlAuth::with_client_id(auth_token, client_id)
}

fn local_authorized_clients() -> Vec<AuthorizedClient> {
    let mut clients = profile_repository()
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|profile| AuthorizedClient::new(profile.id, profile.label))
        .collect::<Vec<_>>();

    if clients.iter().all(|client| client.id != "local-dev-client") {
        clients.push(AuthorizedClient::new(
            "local-dev-client",
            "Local Dev Client",
        ));
    }

    clients
}

fn data_dir() -> PathBuf {
    std::env::var_os("APPRELAY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("apprelay"))
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

impl From<ApplicationPermissionDto> for ApplicationPermission {
    fn from(permission: ApplicationPermissionDto) -> Self {
        Self {
            application_id: permission.application_id,
            label: permission.label,
        }
    }
}

impl From<ApplicationPermission> for ApplicationPermissionDto {
    fn from(permission: ApplicationPermission) -> Self {
        Self {
            application_id: permission.application_id,
            label: permission.label,
        }
    }
}

impl From<apprelay_protocol::HealthStatus> for HealthStatusDto {
    fn from(status: apprelay_protocol::HealthStatus) -> Self {
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

impl From<apprelay_protocol::ApplicationSummary> for AppSummaryDto {
    fn from(application: apprelay_protocol::ApplicationSummary) -> Self {
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
        let (mime_type, data_url) = icon_data_url(&icon)
            .map(|data_url| (normalized_icon_mime_type(&icon).to_string(), Some(data_url)))
            .unwrap_or_else(|| (icon.mime_type.clone(), None));

        Self {
            mime_type,
            data_url,
            source: icon.source,
        }
    }
}

fn icon_data_url(icon: &AppIcon) -> Option<String> {
    if icon.bytes.is_empty() {
        return None;
    }

    if icon.mime_type.eq_ignore_ascii_case("image/icns") {
        if let Some(png_bytes) = extract_icns_png_payload(&icon.bytes) {
            return Some(format!(
                "data:image/png;base64,{}",
                base64_encode(png_bytes)
            ));
        }
    }

    Some(format!(
        "data:{};base64,{}",
        icon.mime_type,
        base64_encode(&icon.bytes)
    ))
}

fn normalized_icon_mime_type(icon: &AppIcon) -> &str {
    if icon.mime_type.eq_ignore_ascii_case("image/icns")
        && extract_icns_png_payload(&icon.bytes).is_some()
    {
        "image/png"
    } else {
        icon.mime_type.as_str()
    }
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
            application_id: window.application_id,
            title: window.title,
            selection_method: window_selection_method_name(&window.selection_method).to_string(),
        }
    }
}

impl From<ApplicationSession> for ApplicationSessionDto {
    fn from(session: ApplicationSession) -> Self {
        Self {
            id: session.id,
            application_id: session.application_id,
            selected_window: session.selected_window.into(),
            launch_intent: session.launch_intent.map(Into::into),
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

impl From<ApplicationLaunchIntent> for ApplicationLaunchIntentDto {
    fn from(intent: ApplicationLaunchIntent) -> Self {
        Self {
            session_id: intent.session_id,
            application_id: intent.application_id,
            launch: intent.launch.map(Into::into),
            status: launch_intent_status_name(&intent.status).to_string(),
        }
    }
}

fn launch_intent_status_name(status: &LaunchIntentStatus) -> &'static str {
    match status {
        LaunchIntentStatus::Recorded => "recorded",
        LaunchIntentStatus::Attached => "attached",
        LaunchIntentStatus::Unsupported => "unsupported",
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

fn window_selection_method_name(method: &WindowSelectionMethod) -> &'static str {
    match method {
        WindowSelectionMethod::LaunchIntent => "launchIntent",
        WindowSelectionMethod::ExistingWindow => "existingWindow",
        WindowSelectionMethod::NativeWindow => "nativeWindow",
        WindowSelectionMethod::Synthetic => "synthetic",
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
        Feature::ApplicationLaunch => "applicationLaunch",
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
            list_application_permissions,
            save_application_permission,
            remove_application_permission,
            server_health,
            server_capabilities,
            server_applications,
            active_application_sessions,
            create_application_session,
            resize_application_session,
            close_application_session,
            forward_input,
            active_input_focus,
            audio_stream::active_audio_streams,
            audio_stream::start_audio_stream,
            audio_stream::stop_audio_stream,
            audio_stream::update_audio_stream,
            audio_stream::audio_stream_status,
            video_stream::active_video_streams,
            video_stream::start_video_stream,
            video_stream::stop_video_stream,
            video_stream::reconnect_video_stream,
            video_stream::negotiate_video_stream,
            video_stream::video_stream_status,
            signaling::submit_sdp_offer,
            signaling::submit_sdp_answer,
            signaling::submit_ice_candidate,
            signaling::signal_end_of_candidates,
            signaling::poll_signaling
        ])
        .run(tauri::generate_context!())
        .expect("failed to run AppRelay client");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_window_dto_maps_native_window_selection_method() {
        let dto = SelectedWindowDto::from(SelectedWindow {
            id: "macos-window-session-1-88".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            title: "Native Fake Window".to_string(),
            selection_method: WindowSelectionMethod::NativeWindow,
        });

        assert_eq!(dto.selection_method, "nativeWindow");
    }

    #[test]
    fn icon_data_url_encodes_icon_bytes() {
        let icon = AppIcon {
            mime_type: "image/png".to_string(),
            bytes: vec![0x89, 0x50, 0x4e, 0x47],
            source: Some("test.png".to_string()),
        };

        assert_eq!(
            icon_data_url(&icon),
            Some("data:image/png;base64,iVBORw==".to_string())
        );
    }

    #[test]
    fn icon_data_url_extracts_png_from_icns_icon_bytes() {
        let png_bytes = b"\x89PNG\r\n\x1a\npng payload";
        let mut icns_bytes = Vec::new();
        icns_bytes.extend_from_slice(b"icns");
        icns_bytes.extend_from_slice(&((8 + 8 + png_bytes.len()) as u32).to_be_bytes());
        icns_bytes.extend_from_slice(b"ic10");
        icns_bytes.extend_from_slice(&((8 + png_bytes.len()) as u32).to_be_bytes());
        icns_bytes.extend_from_slice(png_bytes);

        let dto = AppIconDto::from(AppIcon {
            mime_type: "image/icns".to_string(),
            bytes: icns_bytes,
            source: Some("Contents/Resources/Test.icns".to_string()),
        });

        assert_eq!(dto.mime_type, "image/png");
        assert_eq!(
            dto.data_url,
            Some(format!(
                "data:image/png;base64,{}",
                base64_encode(png_bytes)
            ))
        );
    }

    #[test]
    fn icon_data_url_preserves_icns_fallback_for_malformed_or_no_png_bytes() {
        let malformed_icon = AppIcon {
            mime_type: "image/icns".to_string(),
            bytes: b"icns\0\0\0\x20bad".to_vec(),
            source: Some("Malformed.icns".to_string()),
        };
        let no_png_icns = b"icns\0\0\0\x10TOC \0\0\0\x08".to_vec();
        let no_png_icon = AppIcon {
            mime_type: "image/icns".to_string(),
            bytes: no_png_icns.clone(),
            source: Some("NoPng.icns".to_string()),
        };

        let malformed_dto = AppIconDto::from(malformed_icon);
        let no_png_dto = AppIconDto::from(no_png_icon);

        assert_eq!(malformed_dto.mime_type, "image/icns");
        assert_eq!(
            malformed_dto.data_url,
            Some("data:image/icns;base64,aWNucwAAACBiYWQ=".to_string())
        );
        assert_eq!(no_png_dto.mime_type, "image/icns");
        assert_eq!(
            no_png_dto.data_url,
            Some(format!(
                "data:image/icns;base64,{}",
                base64_encode(&no_png_icns)
            ))
        );
    }

    #[test]
    fn icon_data_url_preserves_icns_fallback_for_oversized_chunk_length() {
        let oversized_chunk_icns = b"icns\0\0\0\x10ic10\xff\xff\xff\xff".to_vec();
        let dto = AppIconDto::from(AppIcon {
            mime_type: "image/icns".to_string(),
            bytes: oversized_chunk_icns.clone(),
            source: Some("Oversized.icns".to_string()),
        });

        assert_eq!(dto.mime_type, "image/icns");
        assert_eq!(
            dto.data_url,
            Some(format!(
                "data:image/icns;base64,{}",
                base64_encode(&oversized_chunk_icns)
            ))
        );
    }

    #[test]
    fn icon_data_url_omits_empty_icon_bytes() {
        let icon = AppIcon {
            mime_type: "application/x-icon-theme-name".to_string(),
            bytes: Vec::new(),
            source: Some("utilities-terminal".to_string()),
        };

        assert_eq!(icon_data_url(&icon), None);
    }

    #[test]
    fn paired_auth_carries_client_id_separately_from_token() {
        let auth = paired_auth("token".to_string(), "profile-1".to_string());

        assert_eq!(auth.token(), "token");
        assert_eq!(auth.client_id(), Some("profile-1"));
    }
}
