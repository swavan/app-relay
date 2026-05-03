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
    ActiveInputFocus, AppRelayError, ApplicationSession, ApplicationSummary, ControlAuth,
    CreateSessionRequest, ForwardInputRequest, HealthStatus, InputDelivery, PlatformCapability,
    ResizeSessionRequest,
};
use apprelay_server::{ServerControlPlane, ServerServices};

static CONTROL_PLANE: OnceLock<Mutex<ServerControlPlane>> = OnceLock::new();

#[tauri::command]
fn list_connection_profiles() -> Result<Vec<ConnectionProfile>, String> {
    profile_repository().list().map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn save_connection_profile(profile: ConnectionProfile) -> Result<(), String> {
    profile_repository()
        .save(profile)
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn remove_connection_profile(id: String) -> Result<(), String> {
    profile_repository()
        .remove(&id)
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn list_application_permissions() -> Result<Vec<ApplicationPermission>, String> {
    permission_repository()
        .list()
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn save_application_permission(permission: ApplicationPermission) -> Result<(), String> {
    permission_repository()
        .save(permission)
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn remove_application_permission(application_id: String) -> Result<(), String> {
    permission_repository()
        .remove(&application_id)
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn server_health(auth_token: String) -> Result<HealthStatus, String> {
    with_control_plane(|control_plane| control_plane.health(&ControlAuth::new(auth_token)))
}

#[tauri::command]
fn server_capabilities(auth_token: String) -> Result<Vec<PlatformCapability>, String> {
    with_control_plane(|control_plane| control_plane.capabilities(&ControlAuth::new(auth_token)))
}

#[tauri::command]
fn server_applications(auth_token: String) -> Result<Vec<ApplicationSummary>, String> {
    with_control_plane(|control_plane| {
        control_plane.available_applications(&ControlAuth::new(auth_token))
    })
}

#[tauri::command]
fn active_application_sessions(
    auth_token: String,
    client_id: String,
) -> Result<Vec<ApplicationSession>, String> {
    with_control_plane(|control_plane| {
        control_plane.active_sessions(&paired_auth(auth_token, client_id))
    })
}

#[tauri::command]
fn create_application_session(
    auth_token: String,
    client_id: String,
    request: CreateSessionRequest,
) -> Result<ApplicationSession, String> {
    // PUNT: this allow-list lookup duplicates app policy that ought to live in
    // the server's `SessionPolicy` (so every host enforces the same rules).
    // Keeping it in the shell preserves current behaviour where permissions
    // can be added/revoked without restarting the control plane; moving it
    // means making the permission repository injectable into the server.
    ensure_application_allowed(&request.application_id)?;

    with_control_plane(|control_plane| {
        control_plane.create_session(&paired_auth(auth_token, client_id), request)
    })
}

#[tauri::command]
fn resize_application_session(
    auth_token: String,
    client_id: String,
    request: ResizeSessionRequest,
) -> Result<ApplicationSession, String> {
    with_control_plane(|control_plane| {
        control_plane.resize_session(&paired_auth(auth_token, client_id), request)
    })
}

#[tauri::command]
fn close_application_session(
    auth_token: String,
    client_id: String,
    session_id: String,
) -> Result<ApplicationSession, String> {
    with_control_plane(|control_plane| {
        control_plane.close_session(&paired_auth(auth_token, client_id), &session_id)
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

pub(crate) fn paired_auth(auth_token: String, client_id: String) -> ControlAuth {
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
    fn paired_auth_carries_client_id_separately_from_token() {
        let auth = paired_auth("token".to_string(), "profile-1".to_string());

        assert_eq!(auth.token(), "token");
        assert_eq!(auth.client_id(), Some("profile-1"));
    }
}
