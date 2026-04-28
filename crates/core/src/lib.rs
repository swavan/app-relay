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

pub trait ConnectionProfileRepository {
    fn list(&self) -> Result<Vec<ConnectionProfile>, ProfileStoreError>;
    fn save(&self, profile: ConnectionProfile) -> Result<(), ProfileStoreError>;
    fn remove(&self, id: &str) -> Result<(), ProfileStoreError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionProfile {
    pub id: String,
    pub label: String,
    pub ssh_user: String,
    pub ssh_host: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub auth_token: String,
}

impl ConnectionProfile {
    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        if self.id.trim().is_empty() {
            return Err(ProfileValidationError::MissingId);
        }

        if self.label.trim().is_empty() {
            return Err(ProfileValidationError::MissingLabel);
        }

        if self.ssh_user.trim().is_empty() {
            return Err(ProfileValidationError::MissingSshUser);
        }

        if self.ssh_host.trim().is_empty() {
            return Err(ProfileValidationError::MissingSshHost);
        }

        if self.local_port == 0 || self.remote_port == 0 {
            return Err(ProfileValidationError::InvalidSshPort);
        }

        if self.auth_token.trim().is_empty() {
            return Err(ProfileValidationError::MissingAuthToken);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileValidationError {
    MissingId,
    MissingLabel,
    MissingSshUser,
    MissingSshHost,
    InvalidSshPort,
    MissingAuthToken,
}

#[derive(Debug)]
pub enum ProfileStoreError {
    InvalidProfile(ProfileValidationError),
    Io(std::io::Error),
    CorruptedStore,
}

impl PartialEq for ProfileStoreError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidProfile(left), Self::InvalidProfile(right)) => left == right,
            (Self::CorruptedStore, Self::CorruptedStore) => true,
            (Self::Io(left), Self::Io(right)) => left.kind() == right.kind(),
            _ => false,
        }
    }
}

impl Eq for ProfileStoreError {}

impl From<std::io::Error> for ProfileStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ProfileValidationError> for ProfileStoreError {
    fn from(error: ProfileValidationError) -> Self {
        Self::InvalidProfile(error)
    }
}

#[derive(Clone, Debug)]
pub struct FileConnectionProfileRepository {
    path: PathBuf,
}

impl FileConnectionProfileRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn read_profiles(&self) -> Result<Vec<ConnectionProfile>, ProfileStoreError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };

        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(decode_profile)
            .collect()
    }

    fn write_profiles(&self, profiles: &[ConnectionProfile]) -> Result<(), ProfileStoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut contents = String::new();
        for profile in profiles {
            contents.push_str(&encode_profile(profile));
            contents.push('\n');
        }

        fs::write(&self.path, contents)?;
        Ok(())
    }
}

impl ConnectionProfileRepository for FileConnectionProfileRepository {
    fn list(&self) -> Result<Vec<ConnectionProfile>, ProfileStoreError> {
        let mut profiles = self.read_profiles()?;
        profiles.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });

        Ok(profiles)
    }

    fn save(&self, profile: ConnectionProfile) -> Result<(), ProfileStoreError> {
        profile.validate()?;

        let mut profiles = self.read_profiles()?;
        profiles.retain(|existing| existing.id != profile.id);
        profiles.push(profile);
        profiles.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        self.write_profiles(&profiles)
    }

    fn remove(&self, id: &str) -> Result<(), ProfileStoreError> {
        let mut profiles = self.read_profiles()?;
        profiles.retain(|profile| profile.id != id);
        self.write_profiles(&profiles)
    }
}

fn encode_profile(profile: &ConnectionProfile) -> String {
    [
        encode_field(&profile.id),
        encode_field(&profile.label),
        encode_field(&profile.ssh_user),
        encode_field(&profile.ssh_host),
        profile.local_port.to_string(),
        profile.remote_port.to_string(),
        encode_field(&profile.auth_token),
    ]
    .join("\t")
}

fn decode_profile(line: &str) -> Result<ConnectionProfile, ProfileStoreError> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 7 {
        return Err(ProfileStoreError::CorruptedStore);
    }

    let profile = ConnectionProfile {
        id: decode_field(fields[0])?,
        label: decode_field(fields[1])?,
        ssh_user: decode_field(fields[2])?,
        ssh_host: decode_field(fields[3])?,
        local_port: fields[4]
            .parse()
            .map_err(|_| ProfileStoreError::CorruptedStore)?,
        remote_port: fields[5]
            .parse()
            .map_err(|_| ProfileStoreError::CorruptedStore)?,
        auth_token: decode_field(fields[6])?,
    };

    profile.validate()?;
    Ok(profile)
}

fn encode_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn decode_field(value: &str) -> Result<String, ProfileStoreError> {
    let mut decoded = String::new();
    let mut chars = value.chars();

    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }

        match chars.next() {
            Some('\\') => decoded.push('\\'),
            Some('t') => decoded.push('\t'),
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            _ => return Err(ProfileStoreError::CorruptedStore),
        }
    }

    Ok(decoded)
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
        let app_discovery = match self.platform {
            Platform::Linux => {
                PlatformCapability::supported(self.platform, Feature::AppDiscovery)
            }
            Platform::Macos | Platform::Windows => PlatformCapability::unsupported(
                self.platform,
                Feature::AppDiscovery,
                "desktop application discovery backend is not implemented for this platform yet",
            ),
            Platform::Android | Platform::Ios => PlatformCapability::unsupported(
                self.platform,
                Feature::AppDiscovery,
                "mobile platforms are client targets and do not expose desktop application discovery",
            ),
            Platform::Unknown => PlatformCapability::unsupported(
                self.platform,
                Feature::AppDiscovery,
                "unknown platform cannot expose desktop application discovery",
            ),
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
    fn desktop_capabilities_mark_missing_backends_as_not_implemented() {
        let service = DefaultCapabilityService::new(Platform::Windows);
        let capabilities = service.platform_capabilities();
        let app_discovery = capabilities
            .iter()
            .find(|capability| capability.feature == Feature::AppDiscovery)
            .expect("app discovery capability");

        assert!(!app_discovery.supported);
        assert_eq!(
            app_discovery.reason.as_deref(),
            Some("desktop application discovery backend is not implemented for this platform yet")
        );
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

    #[test]
    fn connection_profile_validation_rejects_missing_token() {
        let mut profile = test_profile("local", "Local workstation");
        profile.auth_token = " ".to_string();

        assert_eq!(
            profile.validate(),
            Err(ProfileValidationError::MissingAuthToken)
        );
    }

    #[test]
    fn file_connection_profile_repository_persists_profiles() {
        let root = unique_test_dir("connection-profile-store");
        let repository = FileConnectionProfileRepository::new(root.join("profiles.tsv"));

        repository
            .save(test_profile("z", "Zed workstation"))
            .expect("save zed profile");
        repository
            .save(test_profile("a", "Alpha workstation"))
            .expect("save alpha profile");

        let profiles = repository.list().expect("list profiles");
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "z"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_connection_profile_repository_replaces_profiles_by_id() {
        let root = unique_test_dir("connection-profile-replace");
        let repository = FileConnectionProfileRepository::new(root.join("profiles.tsv"));

        repository
            .save(test_profile("local", "Local workstation"))
            .expect("save original profile");
        repository
            .save(test_profile("local", "Updated workstation"))
            .expect("replace profile");

        assert_eq!(
            repository.list().expect("list profiles"),
            vec![test_profile("local", "Updated workstation")]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_connection_profile_repository_removes_profiles_by_id() {
        let root = unique_test_dir("connection-profile-remove");
        let repository = FileConnectionProfileRepository::new(root.join("profiles.tsv"));

        repository
            .save(test_profile("local", "Local workstation"))
            .expect("save profile");
        repository.remove("local").expect("remove profile");

        assert_eq!(repository.list().expect("list profiles"), Vec::new());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_connection_profile_repository_reports_corruption() {
        let root = unique_test_dir("connection-profile-corrupt");
        let path = root.join("profiles.tsv");
        fs::create_dir_all(&root).expect("create profile store dir");
        fs::write(&path, "bad data").expect("write corrupted profile store");

        let repository = FileConnectionProfileRepository::new(path);

        assert_eq!(repository.list(), Err(ProfileStoreError::CorruptedStore));

        let _ = fs::remove_dir_all(root);
    }

    fn test_profile(id: &str, label: &str) -> ConnectionProfile {
        ConnectionProfile {
            id: id.to_string(),
            label: label.to_string(),
            ssh_user: "biplab".to_string(),
            ssh_host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7676,
            auth_token: "token".to_string(),
        }
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
    }
}
