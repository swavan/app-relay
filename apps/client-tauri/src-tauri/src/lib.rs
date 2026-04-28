use std::path::PathBuf;

use swavan_core::{
    ConnectionProfile, ConnectionProfileRepository, FileConnectionProfileRepository, ServerConfig,
};
use swavan_protocol::{ControlAuth, Feature, Platform, PlatformCapability};
use swavan_server::{ServerControlPlane, ServerServices};

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
    control_plane()
        .health(&ControlAuth::new(auth_token))
        .map(Into::into)
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn server_capabilities(auth_token: String) -> Result<Vec<CapabilityDto>, String> {
    control_plane()
        .capabilities(&ControlAuth::new(auth_token))
        .map(|capabilities| capabilities.into_iter().map(Into::into).collect())
        .map_err(|error| format!("{error:?}"))
}

#[tauri::command]
fn server_applications(auth_token: String) -> Result<Vec<AppSummaryDto>, String> {
    control_plane()
        .available_applications(&ControlAuth::new(auth_token))
        .map(|applications| applications.into_iter().map(Into::into).collect())
        .map_err(|error| format!("{error:?}"))
}

fn profile_repository() -> FileConnectionProfileRepository {
    FileConnectionProfileRepository::new(data_dir().join("connection-profiles.tsv"))
}

fn control_plane() -> ServerControlPlane {
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
        }
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
            server_applications
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Swavan AppRelay client");
}
