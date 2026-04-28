//! Core service contracts for Swavan AppRelay.

use std::fs;
use std::path::{Path, PathBuf};

use swavan_protocol::{
    ApplicationSummary, Feature, HealthStatus, Platform, PlatformCapability, SwavanError,
};

pub trait HealthService {
    fn status(&self) -> HealthStatus;
}

pub trait CapabilityService {
    fn platform_capabilities(&self) -> Vec<PlatformCapability>;
}

pub trait ApplicationDiscovery {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub bind_address: String,
    pub control_port: u16,
    pub auth_token: String,
    pub heartbeat_interval_millis: u64,
    pub ssh_tunnel: SshTunnelConfig,
}

impl ServerConfig {
    pub fn local(auth_token: impl Into<String>) -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            control_port: 7676,
            auth_token: auth_token.into(),
            heartbeat_interval_millis: 5_000,
            ssh_tunnel: SshTunnelConfig::localhost(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.auth_token.trim().is_empty() {
            return Err(ConfigError::MissingAuthToken);
        }

        if self.control_port == 0 {
            return Err(ConfigError::InvalidControlPort);
        }

        if self.heartbeat_interval_millis == 0 {
            return Err(ConfigError::InvalidHeartbeatInterval);
        }

        self.ssh_tunnel.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshTunnelConfig {
    pub user: String,
    pub host: String,
    pub local_port: u16,
    pub remote_port: u16,
}

impl SshTunnelConfig {
    pub fn localhost() -> Self {
        Self {
            user: "local".to_string(),
            host: "localhost".to_string(),
            local_port: 7676,
            remote_port: 7676,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.user.trim().is_empty() {
            return Err(ConfigError::MissingSshUser);
        }

        if self.host.trim().is_empty() {
            return Err(ConfigError::MissingSshHost);
        }

        if self.local_port == 0 || self.remote_port == 0 {
            return Err(ConfigError::InvalidSshPort);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    MissingAuthToken,
    InvalidControlPort,
    InvalidHeartbeatInterval,
    MissingSshUser,
    MissingSshHost,
    InvalidSshPort,
}

#[derive(Clone, Debug)]
pub struct StaticHealthService {
    service: String,
    version: String,
}

impl StaticHealthService {
    pub fn new(service: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            version: version.into(),
        }
    }
}

impl HealthService for StaticHealthService {
    fn status(&self) -> HealthStatus {
        HealthStatus::healthy(self.service.clone(), self.version.clone())
    }
}

#[derive(Clone, Debug)]
pub struct DefaultCapabilityService {
    platform: Platform,
}

impl DefaultCapabilityService {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }
}

impl CapabilityService for DefaultCapabilityService {
    fn platform_capabilities(&self) -> Vec<PlatformCapability> {
        let unsupported_reason = "feature planned but not implemented in Phase 1";
        let app_discovery = if self.platform == Platform::Linux {
            PlatformCapability::supported(self.platform, Feature::AppDiscovery)
        } else {
            PlatformCapability::unsupported(
                self.platform,
                Feature::AppDiscovery,
                unsupported_reason,
            )
        };

        vec![
            app_discovery,
            PlatformCapability::unsupported(
                self.platform,
                Feature::WindowResize,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::WindowVideoStream,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::SystemAudioStream,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::ClientMicrophoneInput,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::KeyboardInput,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(self.platform, Feature::MouseInput, unsupported_reason),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct UnsupportedApplicationDiscovery {
    platform: Platform,
}

impl UnsupportedApplicationDiscovery {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }
}

impl ApplicationDiscovery for UnsupportedApplicationDiscovery {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError> {
        Err(SwavanError::unsupported(
            self.platform,
            Feature::AppDiscovery,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct DesktopEntryApplicationDiscovery {
    roots: Vec<PathBuf>,
}

impl DesktopEntryApplicationDiscovery {
    pub fn linux_defaults() -> Self {
        let mut roots = vec![PathBuf::from("/usr/share/applications")];

        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".local/share/applications"));
        }

        Self { roots }
    }

    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    fn discover_root(root: &Path, applications: &mut Vec<ApplicationSummary>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("desktop") {
                continue;
            }

            if let Some(application) = parse_desktop_entry(&path) {
                applications.push(application);
            }
        }
    }
}

impl ApplicationDiscovery for DesktopEntryApplicationDiscovery {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError> {
        let mut applications = Vec::new();

        for root in &self.roots {
            Self::discover_root(root, &mut applications);
        }

        applications.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        applications.dedup_by(|left, right| left.id == right.id);

        Ok(applications)
    }
}

fn parse_desktop_entry(path: &Path) -> Option<ApplicationSummary> {
    let contents = fs::read_to_string(path).ok()?;
    let mut in_desktop_entry = false;
    let mut is_application = false;
    let mut hidden = false;
    let mut no_display = false;
    let mut name = None;

    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "Type" => is_application = value == "Application",
            "Hidden" => hidden = value == "true",
            "NoDisplay" => no_display = value == "true",
            "Name" => name = Some(value.trim().to_string()),
            _ => {}
        }
    }

    if !is_application || hidden || no_display {
        return None;
    }

    let name = name.filter(|value| !value.is_empty())?;
    let id = path.file_stem()?.to_string_lossy().into_owned();

    Some(ApplicationSummary {
        id,
        name,
        icon: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_health_service_returns_configured_status() {
        let service = StaticHealthService::new("swavan-server", "0.1.0");

        assert_eq!(
            service.status(),
            HealthStatus::healthy("swavan-server", "0.1.0")
        );
    }

    #[test]
    fn default_capabilities_are_explicitly_unsupported() {
        let service = DefaultCapabilityService::new(Platform::Macos);
        let capabilities = service.platform_capabilities();

        assert_eq!(capabilities.len(), 7);
        assert!(capabilities.iter().all(|capability| !capability.supported));
        assert!(capabilities
            .iter()
            .all(|capability| capability.platform == Platform::Macos));
    }

    #[test]
    fn linux_capabilities_support_application_discovery() {
        let service = DefaultCapabilityService::new(Platform::Linux);
        let capabilities = service.platform_capabilities();

        assert!(capabilities
            .iter()
            .any(|capability| capability.feature == Feature::AppDiscovery && capability.supported));
    }

    #[test]
    fn unsupported_application_discovery_returns_typed_error() {
        let discovery = UnsupportedApplicationDiscovery::new(Platform::Windows);

        assert_eq!(
            discovery.available_applications(),
            Err(SwavanError::unsupported(
                Platform::Windows,
                Feature::AppDiscovery
            ))
        );
    }

    #[test]
    fn server_config_requires_auth_token() {
        let config = ServerConfig::local(" ");

        assert_eq!(config.validate(), Err(ConfigError::MissingAuthToken));
    }

    #[test]
    fn server_config_accepts_local_defaults() {
        let config = ServerConfig::local("test-token");

        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn desktop_entry_discovery_returns_visible_applications() {
        let root = unique_test_dir("desktop-entry-discovery");
        fs::create_dir_all(&root).expect("create test applications directory");
        fs::write(
            root.join("visible.desktop"),
            "[Desktop Entry]\nType=Application\nName=Visible App\nExec=visible\n",
        )
        .expect("write visible desktop entry");
        fs::write(
            root.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden App\nHidden=true\n",
        )
        .expect("write hidden desktop entry");
        fs::write(
            root.join("folder.desktop"),
            "[Desktop Entry]\nType=Directory\nName=Folder\n",
        )
        .expect("write non-application desktop entry");

        let discovery = DesktopEntryApplicationDiscovery::new(vec![root.clone()]);
        let applications = discovery
            .available_applications()
            .expect("discover applications");

        assert_eq!(
            applications,
            vec![ApplicationSummary {
                id: "visible".to_string(),
                name: "Visible App".to_string(),
                icon: None,
            }]
        );

        let _ = fs::remove_dir_all(root);
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
    }
}
