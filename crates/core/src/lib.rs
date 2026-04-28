//! Core service contracts for Swavan AppRelay.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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

pub trait ServerConfigRepository {
    fn load(&self) -> Result<ServerConfig, ConfigStoreError>;
    fn save(&self, config: &ServerConfig) -> Result<(), ConfigStoreError>;
}

pub trait EventSink {
    fn record(&mut self, event: ServerEvent);
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
    decode_escaped_field(value).map_err(|_| ProfileStoreError::CorruptedStore)
}

fn decode_escaped_field(value: &str) -> Result<String, ()> {
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
            _ => return Err(()),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshTunnelCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl SshTunnelCommand {
    pub fn from_config(config: &SshTunnelConfig) -> Result<Self, ConfigError> {
        config.validate()?;

        Ok(Self {
            program: "ssh".to_string(),
            args: vec![
                "-N".to_string(),
                "-L".to_string(),
                format!("{}:127.0.0.1:{}", config.local_port, config.remote_port),
                format!("{}@{}", config.user, config.host),
            ],
        })
    }
}

pub trait ManagedSshTunnel {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> std::io::Result<Option<i32>>;
    fn kill(&mut self) -> std::io::Result<()>;
    fn wait(&mut self) -> std::io::Result<i32>;
}

pub trait SshTunnelSpawner {
    type Tunnel: ManagedSshTunnel;

    fn spawn(&self, command: &SshTunnelCommand) -> std::io::Result<Self::Tunnel>;
}

#[derive(Clone, Debug, Default)]
pub struct SystemSshTunnelSpawner;

impl SshTunnelSpawner for SystemSshTunnelSpawner {
    type Tunnel = Child;

    fn spawn(&self, command: &SshTunnelCommand) -> std::io::Result<Self::Tunnel> {
        Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }
}

impl ManagedSshTunnel for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }

    fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
        Child::try_wait(self).map(|status| status.map(|status| status.code().unwrap_or_default()))
    }

    fn kill(&mut self) -> std::io::Result<()> {
        Child::kill(self)
    }

    fn wait(&mut self) -> std::io::Result<i32> {
        Child::wait(self).map(|status| status.code().unwrap_or_default())
    }
}

#[derive(Debug)]
pub struct SshTunnelSupervisor<S>
where
    S: SshTunnelSpawner,
{
    spawner: S,
    tunnel: Option<S::Tunnel>,
}

impl<S> SshTunnelSupervisor<S>
where
    S: SshTunnelSpawner,
{
    pub fn new(spawner: S) -> Self {
        Self {
            spawner,
            tunnel: None,
        }
    }

    pub fn start(&mut self, config: &SshTunnelConfig) -> Result<u32, SshTunnelProcessError> {
        if self.is_running()? {
            return Err(SshTunnelProcessError::AlreadyRunning);
        }

        let command =
            SshTunnelCommand::from_config(config).map_err(SshTunnelProcessError::InvalidConfig)?;
        let tunnel = self
            .spawner
            .spawn(&command)
            .map_err(SshTunnelProcessError::Io)?;
        let process_id = tunnel.id();
        self.tunnel = Some(tunnel);

        Ok(process_id)
    }

    pub fn stop(&mut self) -> Result<(), SshTunnelProcessError> {
        let Some(mut tunnel) = self.tunnel.take() else {
            return Err(SshTunnelProcessError::NotRunning);
        };

        if tunnel
            .try_wait()
            .map_err(SshTunnelProcessError::Io)?
            .is_some()
        {
            return Err(SshTunnelProcessError::NotRunning);
        }

        tunnel.kill().map_err(SshTunnelProcessError::Io)?;
        tunnel.wait().map_err(SshTunnelProcessError::Io)?;
        Ok(())
    }

    pub fn is_running(&mut self) -> Result<bool, SshTunnelProcessError> {
        let Some(tunnel) = &mut self.tunnel else {
            return Ok(false);
        };

        match tunnel.try_wait().map_err(SshTunnelProcessError::Io)? {
            Some(_) => {
                self.tunnel = None;
                Ok(false)
            }
            None => Ok(true),
        }
    }
}

#[derive(Debug)]
pub enum SshTunnelProcessError {
    InvalidConfig(ConfigError),
    Io(std::io::Error),
    AlreadyRunning,
    NotRunning,
}

impl PartialEq for SshTunnelProcessError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidConfig(left), Self::InvalidConfig(right)) => left == right,
            (Self::AlreadyRunning, Self::AlreadyRunning) => true,
            (Self::NotRunning, Self::NotRunning) => true,
            (Self::Io(left), Self::Io(right)) => left.kind() == right.kind(),
            _ => false,
        }
    }
}

impl Eq for SshTunnelProcessError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerEvent {
    ControlPlaneStarted { bind_address: String, port: u16 },
    ControlPlaneStopped,
    SshTunnelStarted { process_id: u32 },
    SshTunnelStopped,
    SshTunnelFailed { reason: String },
    RequestAuthorized { operation: String },
    RequestRejected { operation: String },
    ConfigLoaded { path: PathBuf },
    ConfigSaved { path: PathBuf },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemoryEventSink {
    events: Vec<ServerEvent>,
}

impl InMemoryEventSink {
    pub fn events(&self) -> &[ServerEvent] {
        &self.events
    }
}

impl EventSink for InMemoryEventSink {
    fn record(&mut self, event: ServerEvent) {
        self.events.push(event);
    }
}

#[derive(Clone, Debug)]
pub struct FileEventSink {
    path: PathBuf,
}

impl FileEventSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn record_event(&self, event: &ServerEvent) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", format_event(event))
    }
}

impl EventSink for FileEventSink {
    fn record(&mut self, event: ServerEvent) {
        let _ = self.record_event(&event);
    }
}

fn format_event(event: &ServerEvent) -> String {
    match event {
        ServerEvent::ControlPlaneStarted { bind_address, port } => {
            format!("event=control_plane_started bind_address={bind_address} port={port}")
        }
        ServerEvent::ControlPlaneStopped => "event=control_plane_stopped".to_string(),
        ServerEvent::SshTunnelStarted { process_id } => {
            format!("event=ssh_tunnel_started process_id={process_id}")
        }
        ServerEvent::SshTunnelStopped => "event=ssh_tunnel_stopped".to_string(),
        ServerEvent::SshTunnelFailed { reason } => {
            format!("event=ssh_tunnel_failed reason={}", encode_field(reason))
        }
        ServerEvent::RequestAuthorized { operation } => {
            format!("event=request_authorized operation={operation}")
        }
        ServerEvent::RequestRejected { operation } => {
            format!("event=request_rejected operation={operation}")
        }
        ServerEvent::ConfigLoaded { path } => {
            format!("event=config_loaded path={}", path.display())
        }
        ServerEvent::ConfigSaved { path } => {
            format!("event=config_saved path={}", path.display())
        }
    }
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

#[derive(Debug)]
pub enum ConfigStoreError {
    InvalidConfig(ConfigError),
    Io(std::io::Error),
    CorruptedStore,
}

impl PartialEq for ConfigStoreError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidConfig(left), Self::InvalidConfig(right)) => left == right,
            (Self::CorruptedStore, Self::CorruptedStore) => true,
            (Self::Io(left), Self::Io(right)) => left.kind() == right.kind(),
            _ => false,
        }
    }
}

impl Eq for ConfigStoreError {}

impl From<std::io::Error> for ConfigStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ConfigError> for ConfigStoreError {
    fn from(error: ConfigError) -> Self {
        Self::InvalidConfig(error)
    }
}

#[derive(Clone, Debug)]
pub struct FileServerConfigRepository {
    path: PathBuf,
}

impl FileServerConfigRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl ServerConfigRepository for FileServerConfigRepository {
    fn load(&self) -> Result<ServerConfig, ConfigStoreError> {
        let contents = fs::read_to_string(&self.path)?;
        let config = decode_server_config(&contents)?;

        config.validate()?;
        Ok(config)
    }

    fn save(&self, config: &ServerConfig) -> Result<(), ConfigStoreError> {
        config.validate()?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.path, encode_server_config(config))?;
        Ok(())
    }
}

fn encode_server_config(config: &ServerConfig) -> String {
    [
        format!("bind_address={}", encode_field(&config.bind_address)),
        format!("control_port={}", config.control_port),
        format!("auth_token={}", encode_field(&config.auth_token)),
        format!(
            "heartbeat_interval_millis={}",
            config.heartbeat_interval_millis
        ),
        format!("ssh_user={}", encode_field(&config.ssh_tunnel.user)),
        format!("ssh_host={}", encode_field(&config.ssh_tunnel.host)),
        format!("ssh_local_port={}", config.ssh_tunnel.local_port),
        format!("ssh_remote_port={}", config.ssh_tunnel.remote_port),
    ]
    .join("\n")
        + "\n"
}

fn decode_server_config(contents: &str) -> Result<ServerConfig, ConfigStoreError> {
    let mut bind_address = None;
    let mut control_port = None;
    let mut auth_token = None;
    let mut heartbeat_interval_millis = None;
    let mut ssh_user = None;
    let mut ssh_host = None;
    let mut ssh_local_port = None;
    let mut ssh_remote_port = None;

    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((key, value)) = line.split_once('=') else {
            return Err(ConfigStoreError::CorruptedStore);
        };

        match key {
            "bind_address" => bind_address = Some(decode_config_field(value)?),
            "control_port" => control_port = Some(parse_config_number(value)?),
            "auth_token" => auth_token = Some(decode_config_field(value)?),
            "heartbeat_interval_millis" => {
                heartbeat_interval_millis = Some(parse_config_number(value)?)
            }
            "ssh_user" => ssh_user = Some(decode_config_field(value)?),
            "ssh_host" => ssh_host = Some(decode_config_field(value)?),
            "ssh_local_port" => ssh_local_port = Some(parse_config_number(value)?),
            "ssh_remote_port" => ssh_remote_port = Some(parse_config_number(value)?),
            _ => return Err(ConfigStoreError::CorruptedStore),
        }
    }

    Ok(ServerConfig {
        bind_address: bind_address.ok_or(ConfigStoreError::CorruptedStore)?,
        control_port: control_port.ok_or(ConfigStoreError::CorruptedStore)?,
        auth_token: auth_token.ok_or(ConfigStoreError::CorruptedStore)?,
        heartbeat_interval_millis: heartbeat_interval_millis
            .ok_or(ConfigStoreError::CorruptedStore)?,
        ssh_tunnel: SshTunnelConfig {
            user: ssh_user.ok_or(ConfigStoreError::CorruptedStore)?,
            host: ssh_host.ok_or(ConfigStoreError::CorruptedStore)?,
            local_port: ssh_local_port.ok_or(ConfigStoreError::CorruptedStore)?,
            remote_port: ssh_remote_port.ok_or(ConfigStoreError::CorruptedStore)?,
        },
    })
}

fn parse_config_number<T>(value: &str) -> Result<T, ConfigStoreError>
where
    T: std::str::FromStr,
{
    value.parse().map_err(|_| ConfigStoreError::CorruptedStore)
}

fn decode_config_field(value: &str) -> Result<String, ConfigStoreError> {
    decode_escaped_field(value).map_err(|_| ConfigStoreError::CorruptedStore)
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
            Platform::Linux | Platform::Macos => {
                PlatformCapability::supported(self.platform, Feature::AppDiscovery)
            }
            Platform::Windows => PlatformCapability::unsupported(
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

#[derive(Clone, Debug)]
pub struct MacosApplicationDiscovery {
    roots: Vec<PathBuf>,
}

impl MacosApplicationDiscovery {
    pub fn macos_defaults() -> Self {
        let mut roots = vec![PathBuf::from("/Applications")];

        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
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
            if path.extension().and_then(|extension| extension.to_str()) != Some("app") {
                continue;
            }

            if let Some(application) = parse_macos_app_bundle(&path) {
                applications.push(application);
            }
        }
    }
}

impl ApplicationDiscovery for MacosApplicationDiscovery {
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

fn parse_macos_app_bundle(path: &Path) -> Option<ApplicationSummary> {
    let info_plist = path.join("Contents/Info.plist");
    let contents = fs::read_to_string(info_plist).ok()?;
    let id = plist_string_value(&contents, "CFBundleIdentifier").or_else(|| {
        path.file_stem()
            .map(|value| value.to_string_lossy().into_owned())
    })?;
    let name = plist_string_value(&contents, "CFBundleDisplayName")
        .or_else(|| plist_string_value(&contents, "CFBundleName"))
        .or_else(|| {
            path.file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })?;

    if id.trim().is_empty() || name.trim().is_empty() {
        return None;
    }

    Some(ApplicationSummary {
        id,
        name,
        icon: None,
    })
}

fn plist_string_value(contents: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let key_start = contents.find(&key_tag)?;
    let after_key = &contents[key_start + key_tag.len()..];
    let string_start = after_key.find("<string>")? + "<string>".len();
    let after_string_start = &after_key[string_start..];
    let string_end = after_string_start.find("</string>")?;
    let value = after_string_start[..string_end].trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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
        let service = DefaultCapabilityService::new(Platform::Windows);
        let capabilities = service.platform_capabilities();

        assert_eq!(capabilities.len(), 7);
        assert!(capabilities.iter().all(|capability| !capability.supported));
        assert!(capabilities
            .iter()
            .all(|capability| capability.platform == Platform::Windows));
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
    fn macos_capabilities_support_application_discovery() {
        let service = DefaultCapabilityService::new(Platform::Macos);
        let capabilities = service.platform_capabilities();

        assert!(capabilities
            .iter()
            .any(|capability| capability.feature == Feature::AppDiscovery && capability.supported));
    }

    #[test]
    fn windows_capabilities_mark_missing_backend_as_not_implemented() {
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
    fn file_server_config_repository_persists_config() {
        let root = unique_test_dir("server-config-store");
        let repository = FileServerConfigRepository::new(root.join("server.conf"));
        let mut config = ServerConfig::local("test-token");
        config.bind_address = "127.0.0.2".to_string();
        config.control_port = 7878;
        config.heartbeat_interval_millis = 2_500;
        config.ssh_tunnel.user = "biplab".to_string();
        config.ssh_tunnel.host = "workstation.local".to_string();
        config.ssh_tunnel.local_port = 8787;
        config.ssh_tunnel.remote_port = 9797;

        repository.save(&config).expect("save server config");

        assert_eq!(repository.load().expect("load server config"), config);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_server_config_repository_rejects_invalid_config() {
        let root = unique_test_dir("server-config-invalid");
        let repository = FileServerConfigRepository::new(root.join("server.conf"));
        let config = ServerConfig::local(" ");

        assert_eq!(
            repository.save(&config),
            Err(ConfigStoreError::InvalidConfig(
                ConfigError::MissingAuthToken
            ))
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_server_config_repository_reports_corruption() {
        let root = unique_test_dir("server-config-corrupt");
        let path = root.join("server.conf");
        fs::create_dir_all(&root).expect("create config store dir");
        fs::write(&path, "bad config").expect("write corrupted config");

        let repository = FileServerConfigRepository::new(path);

        assert_eq!(repository.load(), Err(ConfigStoreError::CorruptedStore));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ssh_tunnel_command_uses_validated_config() {
        let config = SshTunnelConfig {
            user: "biplab".to_string(),
            host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7677,
        };

        assert_eq!(
            SshTunnelCommand::from_config(&config),
            Ok(SshTunnelCommand {
                program: "ssh".to_string(),
                args: vec![
                    "-N".to_string(),
                    "-L".to_string(),
                    "7676:127.0.0.1:7677".to_string(),
                    "biplab@workstation.local".to_string(),
                ],
            })
        );
    }

    #[test]
    fn ssh_tunnel_command_rejects_invalid_config() {
        let config = SshTunnelConfig {
            user: " ".to_string(),
            host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7677,
        };

        assert_eq!(
            SshTunnelCommand::from_config(&config),
            Err(ConfigError::MissingSshUser)
        );
    }

    #[test]
    fn ssh_tunnel_supervisor_starts_and_stops_tunnel() {
        let spawner = FakeSshTunnelSpawner::default();
        let mut supervisor = SshTunnelSupervisor::new(spawner);
        let config = SshTunnelConfig {
            user: "biplab".to_string(),
            host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7677,
        };

        assert_eq!(supervisor.start(&config), Ok(42));
        assert_eq!(supervisor.is_running(), Ok(true));
        assert_eq!(supervisor.stop(), Ok(()));
        assert_eq!(supervisor.is_running(), Ok(false));
    }

    #[test]
    fn ssh_tunnel_supervisor_rejects_double_start() {
        let spawner = FakeSshTunnelSpawner::default();
        let mut supervisor = SshTunnelSupervisor::new(spawner);
        let config = SshTunnelConfig {
            user: "biplab".to_string(),
            host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7677,
        };

        assert_eq!(supervisor.start(&config), Ok(42));
        assert_eq!(
            supervisor.start(&config),
            Err(SshTunnelProcessError::AlreadyRunning)
        );
    }

    #[test]
    fn ssh_tunnel_supervisor_rejects_invalid_config() {
        let spawner = FakeSshTunnelSpawner::default();
        let mut supervisor = SshTunnelSupervisor::new(spawner);
        let config = SshTunnelConfig {
            user: " ".to_string(),
            host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7677,
        };

        assert_eq!(
            supervisor.start(&config),
            Err(SshTunnelProcessError::InvalidConfig(
                ConfigError::MissingSshUser
            ))
        );
    }

    #[test]
    fn ssh_tunnel_supervisor_clears_exited_tunnel() {
        let spawner = FakeSshTunnelSpawner {
            exited_on_spawn: true,
        };
        let mut supervisor = SshTunnelSupervisor::new(spawner);
        let config = SshTunnelConfig {
            user: "biplab".to_string(),
            host: "workstation.local".to_string(),
            local_port: 7676,
            remote_port: 7677,
        };

        assert_eq!(supervisor.start(&config), Ok(42));
        assert_eq!(supervisor.is_running(), Ok(false));
        assert_eq!(supervisor.stop(), Err(SshTunnelProcessError::NotRunning));
    }

    #[test]
    fn in_memory_event_sink_records_events() {
        let mut sink = InMemoryEventSink::default();

        sink.record(ServerEvent::ControlPlaneStarted {
            bind_address: "127.0.0.1".to_string(),
            port: 7676,
        });
        sink.record(ServerEvent::ControlPlaneStopped);

        assert_eq!(
            sink.events(),
            &[
                ServerEvent::ControlPlaneStarted {
                    bind_address: "127.0.0.1".to_string(),
                    port: 7676,
                },
                ServerEvent::ControlPlaneStopped,
            ]
        );
    }

    #[test]
    fn file_event_sink_writes_structured_events() {
        let root = unique_test_dir("file-event-sink");
        let path = root.join("server.log");
        let mut sink = FileEventSink::new(&path);

        sink.record(ServerEvent::ControlPlaneStarted {
            bind_address: "127.0.0.1".to_string(),
            port: 7676,
        });
        sink.record(ServerEvent::RequestAuthorized {
            operation: "health".to_string(),
        });

        let contents = fs::read_to_string(&path).expect("read event log");
        assert_eq!(
            contents,
            "event=control_plane_started bind_address=127.0.0.1 port=7676\n\
event=request_authorized operation=health\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[derive(Clone, Debug, Default)]
    struct FakeSshTunnelSpawner {
        exited_on_spawn: bool,
    }

    impl SshTunnelSpawner for FakeSshTunnelSpawner {
        type Tunnel = FakeManagedSshTunnel;

        fn spawn(&self, _command: &SshTunnelCommand) -> std::io::Result<Self::Tunnel> {
            Ok(FakeManagedSshTunnel {
                running: !self.exited_on_spawn,
            })
        }
    }

    #[derive(Debug)]
    struct FakeManagedSshTunnel {
        running: bool,
    }

    impl ManagedSshTunnel for FakeManagedSshTunnel {
        fn id(&self) -> u32 {
            42
        }

        fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
            if self.running {
                Ok(None)
            } else {
                Ok(Some(0))
            }
        }

        fn kill(&mut self) -> std::io::Result<()> {
            self.running = false;
            Ok(())
        }

        fn wait(&mut self) -> std::io::Result<i32> {
            self.running = false;
            Ok(0)
        }
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
    fn macos_application_discovery_returns_app_bundles() {
        let root = unique_test_dir("macos-app-discovery");
        let app_contents = root.join("Visible.app/Contents");
        fs::create_dir_all(&app_contents).expect("create app bundle");
        fs::write(
            app_contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>dev.swavan.visible</string>
  <key>CFBundleDisplayName</key>
  <string>Visible Mac App</string>
</dict>
</plist>
"#,
        )
        .expect("write info plist");
        fs::create_dir_all(root.join("Ignored.txt")).expect("create ignored non-app directory");

        let discovery = MacosApplicationDiscovery::new(vec![root.clone()]);
        let applications = discovery
            .available_applications()
            .expect("discover macOS applications");

        assert_eq!(
            applications,
            vec![ApplicationSummary {
                id: "dev.swavan.visible".to_string(),
                name: "Visible Mac App".to_string(),
                icon: None,
            }]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn macos_application_discovery_falls_back_to_bundle_name() {
        let root = unique_test_dir("macos-app-fallback");
        let app_contents = root.join("Fallback.app/Contents");
        fs::create_dir_all(&app_contents).expect("create app bundle");
        fs::write(
            app_contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Fallback Name</string>
</dict>
</plist>
"#,
        )
        .expect("write info plist");

        let discovery = MacosApplicationDiscovery::new(vec![root.clone()]);
        let applications = discovery
            .available_applications()
            .expect("discover macOS applications");

        assert_eq!(
            applications,
            vec![ApplicationSummary {
                id: "Fallback".to_string(),
                name: "Fallback Name".to_string(),
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
