//! Core service contracts for AppRelay.

mod audio_stream;
mod input;
mod video_stream;

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
pub use audio_stream::PipeWireCaptureCommandConfig;
pub use audio_stream::{
    AudioBackendNativeReadiness, AudioBackendService, AudioStreamService,
    InMemoryAudioStreamService,
};
pub use input::{
    map_point, InMemoryInputForwardingService, InputBackend, InputBackendService,
    InputForwardingService,
};
pub use video_stream::{
    InMemoryVideoStreamService, VideoStreamService, WindowCaptureBackend,
    WindowCaptureBackendService,
};

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use apprelay_protocol::{
    AppIcon, AppRelayError, ApplicationLaunch, ApplicationLaunchIntent, ApplicationSession,
    ApplicationSummary, ApprovePairingRequest, ControlClientIdentity, CreateSessionRequest,
    Feature, HealthStatus, LaunchIntentStatus, PairingRequest, PairingRequestStatus,
    PendingPairing, Platform, PlatformCapability, ResizeIntentStatus, ResizeSessionRequest,
    SelectedWindow, SessionState, ViewportSize, WindowResizeIntent, WindowSelectionMethod,
};

pub trait HealthService {
    fn status(&self) -> HealthStatus;
}

pub trait CapabilityService {
    fn platform_capabilities(&self) -> Vec<PlatformCapability>;
}

pub trait ApplicationDiscovery {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, AppRelayError>;
}

pub trait ApplicationSessionService {
    fn create_session(
        &mut self,
        request: CreateSessionRequest,
    ) -> Result<ApplicationSession, AppRelayError>;
    fn resize_session(
        &mut self,
        request: ResizeSessionRequest,
    ) -> Result<ApplicationSession, AppRelayError>;
    fn close_session(&mut self, session_id: &str) -> Result<ApplicationSession, AppRelayError>;
    fn active_sessions(&self) -> Vec<ApplicationSession>;
}

pub trait ApplicationLaunchBackend {
    fn prepare_launch(
        &self,
        application: &ApplicationSummary,
        session_id: &str,
    ) -> Result<ApplicationLaunchIntent, AppRelayError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationLaunchBackendService {
    RecordOnly,
    LinuxNative,
    MacosNative { open_command: PathBuf },
    Unsupported { platform: Platform },
}

impl ApplicationLaunchBackend for ApplicationLaunchBackendService {
    fn prepare_launch(
        &self,
        application: &ApplicationSummary,
        session_id: &str,
    ) -> Result<ApplicationLaunchIntent, AppRelayError> {
        match self {
            Self::RecordOnly => Ok(ApplicationLaunchIntent {
                session_id: session_id.to_string(),
                application_id: application.id.clone(),
                launch: application.launch.clone(),
                status: if application.launch.is_some() {
                    LaunchIntentStatus::Recorded
                } else {
                    LaunchIntentStatus::Attached
                },
            }),
            Self::LinuxNative => match &application.launch {
                Some(ApplicationLaunch::DesktopCommand { command }) => {
                    spawn_linux_desktop_command(command)?;
                    Ok(ApplicationLaunchIntent {
                        session_id: session_id.to_string(),
                        application_id: application.id.clone(),
                        launch: application.launch.clone(),
                        status: LaunchIntentStatus::Recorded,
                    })
                }
                Some(ApplicationLaunch::MacosBundle { .. }) => Err(AppRelayError::unsupported(
                    Platform::Linux,
                    Feature::ApplicationLaunch,
                )),
                None => Ok(ApplicationLaunchIntent {
                    session_id: session_id.to_string(),
                    application_id: application.id.clone(),
                    launch: None,
                    status: LaunchIntentStatus::Attached,
                }),
            },
            Self::MacosNative { open_command } => match &application.launch {
                Some(ApplicationLaunch::MacosBundle { bundle_path }) => {
                    spawn_macos_bundle(open_command, bundle_path)?;
                    Ok(ApplicationLaunchIntent {
                        session_id: session_id.to_string(),
                        application_id: application.id.clone(),
                        launch: application.launch.clone(),
                        status: LaunchIntentStatus::Recorded,
                    })
                }
                Some(ApplicationLaunch::DesktopCommand { .. }) => Err(AppRelayError::unsupported(
                    Platform::Macos,
                    Feature::ApplicationLaunch,
                )),
                None => Ok(ApplicationLaunchIntent {
                    session_id: session_id.to_string(),
                    application_id: application.id.clone(),
                    launch: None,
                    status: LaunchIntentStatus::Attached,
                }),
            },
            Self::Unsupported { platform } => Err(AppRelayError::unsupported(
                *platform,
                Feature::ApplicationLaunch,
            )),
        }
    }
}

fn spawn_linux_desktop_command(command: &str) -> Result<(), AppRelayError> {
    let argv = parse_desktop_exec(command)?;
    let (program, args) = argv.split_first().ok_or_else(|| {
        AppRelayError::InvalidRequest("desktop Exec command is empty".to_string())
    })?;

    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            AppRelayError::ServiceUnavailable(format!(
                "failed to launch desktop command `{program}`: {error}"
            ))
        })
}

fn spawn_macos_bundle(open_command: &Path, bundle_path: &str) -> Result<(), AppRelayError> {
    if bundle_path.trim().is_empty() {
        return Err(AppRelayError::InvalidRequest(
            "macOS application bundle path is required".to_string(),
        ));
    }

    Command::new(open_command)
        .arg("-n")
        .arg(bundle_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            AppRelayError::ServiceUnavailable(format!(
                "failed to launch macOS application bundle `{bundle_path}`: {error}"
            ))
        })
}

fn parse_desktop_exec(command: &str) -> Result<Vec<String>, AppRelayError> {
    let mut argv = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_quotes = false;

    while let Some(character) = chars.next() {
        match character {
            '"' => in_quotes = !in_quotes,
            '\\' => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            '%' => match chars.next() {
                Some('%') => current.push('%'),
                Some('f' | 'F' | 'u' | 'U' | 'i' | 'c' | 'k') => {}
                Some(field_code) => {
                    current.push('%');
                    current.push(field_code);
                }
                None => current.push('%'),
            },
            character if character.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    argv.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }

    if in_quotes {
        return Err(AppRelayError::InvalidRequest(
            "desktop Exec command has an unterminated quote".to_string(),
        ));
    }

    if !current.is_empty() {
        argv.push(current);
    }

    if argv.is_empty() {
        return Err(AppRelayError::InvalidRequest(
            "desktop Exec command has no executable after field-code stripping".to_string(),
        ));
    }

    Ok(argv)
}

pub trait WindowResizeBackend {
    fn resize_window(
        &self,
        selected_window: &SelectedWindow,
        viewport: &ViewportSize,
    ) -> Result<ResizeIntentStatus, AppRelayError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowResizeBackendService {
    RecordOnly,
    Unsupported { platform: Platform },
}

impl WindowResizeBackend for WindowResizeBackendService {
    fn resize_window(
        &self,
        _selected_window: &SelectedWindow,
        _viewport: &ViewportSize,
    ) -> Result<ResizeIntentStatus, AppRelayError> {
        match self {
            Self::RecordOnly => Ok(ResizeIntentStatus::Recorded),
            Self::Unsupported { platform } => {
                Err(AppRelayError::unsupported(*platform, Feature::WindowResize))
            }
        }
    }
}

pub trait ConnectionProfileRepository {
    fn list(&self) -> Result<Vec<ConnectionProfile>, ProfileStoreError>;
    fn save(&self, profile: ConnectionProfile) -> Result<(), ProfileStoreError>;
    fn remove(&self, id: &str) -> Result<(), ProfileStoreError>;
}

pub trait ApplicationPermissionRepository {
    fn list(&self) -> Result<Vec<ApplicationPermission>, PermissionStoreError>;
    fn save(&self, permission: ApplicationPermission) -> Result<(), PermissionStoreError>;
    fn remove(&self, application_id: &str) -> Result<(), PermissionStoreError>;
}

pub trait ServerConfigRepository {
    fn load(&self) -> Result<ServerConfig, ConfigStoreError>;
    fn save(&self, config: &ServerConfig) -> Result<(), ConfigStoreError>;
}

pub trait EventSink {
    fn record(&mut self, event: ServerEvent);
}

pub trait ClientAuthorizationService {
    fn request_pairing(&mut self, request: PairingRequest)
        -> Result<PendingPairing, AppRelayError>;
    fn approve_pairing(
        &mut self,
        request: ApprovePairingRequest,
    ) -> Result<AuthorizedClient, AppRelayError>;
    fn authorize_client(&self, client_id: Option<&str>) -> Result<AuthorizedClient, AppRelayError>;
    fn authorized_clients(&self) -> Vec<AuthorizedClient>;
    fn pending_pairings(&self) -> Vec<PendingPairing>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedClient {
    pub id: String,
    pub label: String,
}

impl AuthorizedClient {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.id.trim().is_empty() {
            return Err(ConfigError::MissingAuthorizedClientId);
        }

        if self.label.trim().is_empty() {
            return Err(ConfigError::MissingAuthorizedClientLabel);
        }

        Ok(())
    }
}

impl From<ControlClientIdentity> for AuthorizedClient {
    fn from(identity: ControlClientIdentity) -> Self {
        Self {
            id: identity.id,
            label: identity.label,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPolicy {
    allowed_application_ids: Vec<String>,
    min_viewport_width: u32,
    min_viewport_height: u32,
    max_viewport_width: u32,
    max_viewport_height: u32,
}

impl SessionPolicy {
    pub fn allow_all() -> Self {
        Self {
            allowed_application_ids: Vec::new(),
            min_viewport_width: 320,
            min_viewport_height: 240,
            max_viewport_width: 7680,
            max_viewport_height: 4320,
        }
    }

    pub fn allow_applications(allowed_application_ids: Vec<String>) -> Self {
        Self {
            allowed_application_ids,
            ..Self::allow_all()
        }
    }

    pub fn from_permissions(permissions: &[ApplicationPermission]) -> Self {
        Self::allow_applications(
            permissions
                .iter()
                .map(|permission| permission.application_id.clone())
                .collect(),
        )
    }

    pub fn validate_application(&self, application_id: &str) -> Result<(), AppRelayError> {
        if application_id.trim().is_empty() {
            return Err(AppRelayError::InvalidRequest(
                "application id is required".to_string(),
            ));
        }

        if self.allowed_application_ids.is_empty()
            || self
                .allowed_application_ids
                .iter()
                .any(|allowed_id| allowed_id == application_id)
        {
            Ok(())
        } else {
            Err(AppRelayError::PermissionDenied(format!(
                "application {application_id} is not allowed"
            )))
        }
    }

    pub fn validate_viewport(&self, viewport: &ViewportSize) -> Result<(), AppRelayError> {
        if viewport.width < self.min_viewport_width || viewport.height < self.min_viewport_height {
            return Err(AppRelayError::InvalidRequest(format!(
                "viewport must be at least {}x{}",
                self.min_viewport_width, self.min_viewport_height
            )));
        }

        if viewport.width > self.max_viewport_width || viewport.height > self.max_viewport_height {
            return Err(AppRelayError::InvalidRequest(format!(
                "viewport must be at most {}x{}",
                self.max_viewport_width, self.max_viewport_height
            )));
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationPermission {
    pub application_id: String,
    pub label: String,
}

impl ApplicationPermission {
    pub fn validate(&self) -> Result<(), PermissionValidationError> {
        if self.application_id.trim().is_empty() {
            return Err(PermissionValidationError::MissingApplicationId);
        }

        if self.label.trim().is_empty() {
            return Err(PermissionValidationError::MissingLabel);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PermissionValidationError {
    MissingApplicationId,
    MissingLabel,
}

#[derive(Debug)]
pub enum PermissionStoreError {
    InvalidPermission(PermissionValidationError),
    Io(std::io::Error),
    CorruptedStore,
}

impl PartialEq for PermissionStoreError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::InvalidPermission(left), Self::InvalidPermission(right)) => left == right,
            (Self::CorruptedStore, Self::CorruptedStore) => true,
            (Self::Io(left), Self::Io(right)) => left.kind() == right.kind(),
            _ => false,
        }
    }
}

impl Eq for PermissionStoreError {}

impl From<std::io::Error> for PermissionStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<PermissionValidationError> for PermissionStoreError {
    fn from(error: PermissionValidationError) -> Self {
        Self::InvalidPermission(error)
    }
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
pub struct FileApplicationPermissionRepository {
    path: PathBuf,
}

impl FileApplicationPermissionRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn read_permissions(&self) -> Result<Vec<ApplicationPermission>, PermissionStoreError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };

        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(decode_application_permission)
            .collect()
    }

    fn write_permissions(
        &self,
        permissions: &[ApplicationPermission],
    ) -> Result<(), PermissionStoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut contents = String::new();
        for permission in permissions {
            contents.push_str(&encode_application_permission(permission));
            contents.push('\n');
        }

        fs::write(&self.path, contents)?;
        Ok(())
    }
}

impl ApplicationPermissionRepository for FileApplicationPermissionRepository {
    fn list(&self) -> Result<Vec<ApplicationPermission>, PermissionStoreError> {
        let mut permissions = self.read_permissions()?;
        permissions.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.application_id.cmp(&right.application_id))
        });

        Ok(permissions)
    }

    fn save(&self, permission: ApplicationPermission) -> Result<(), PermissionStoreError> {
        permission.validate()?;

        let mut permissions = self.read_permissions()?;
        permissions.retain(|existing| existing.application_id != permission.application_id);
        permissions.push(permission);
        permissions.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.application_id.cmp(&right.application_id))
        });
        self.write_permissions(&permissions)
    }

    fn remove(&self, application_id: &str) -> Result<(), PermissionStoreError> {
        let mut permissions = self.read_permissions()?;
        permissions.retain(|permission| permission.application_id != application_id);
        self.write_permissions(&permissions)
    }
}

fn encode_application_permission(permission: &ApplicationPermission) -> String {
    [
        encode_field(&permission.application_id),
        encode_field(&permission.label),
    ]
    .join("\t")
}

fn decode_application_permission(
    line: &str,
) -> Result<ApplicationPermission, PermissionStoreError> {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.len() != 2 {
        return Err(PermissionStoreError::CorruptedStore);
    }

    let permission = ApplicationPermission {
        application_id: decode_permission_field(fields[0])?,
        label: decode_permission_field(fields[1])?,
    };

    permission.validate()?;
    Ok(permission)
}

fn decode_permission_field(value: &str) -> Result<String, PermissionStoreError> {
    decode_escaped_field(value).map_err(|_| PermissionStoreError::CorruptedStore)
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
    pub authorized_clients: Vec<AuthorizedClient>,
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
    ControlPlaneStarted {
        bind_address: String,
        port: u16,
    },
    ControlPlaneStopped,
    ForegroundConnectionAccepted {
        peer_address: String,
    },
    ForegroundConnectionClosed {
        peer_address: String,
    },
    SshTunnelStarted {
        process_id: u32,
    },
    SshTunnelStopped,
    SshTunnelFailed {
        reason: String,
    },
    RequestAuthorized {
        operation: String,
    },
    RequestRejected {
        operation: String,
    },
    SessionCreated {
        session_id: String,
        application_id: String,
        client_id: String,
        viewport_width: u32,
        viewport_height: u32,
    },
    SessionResized {
        session_id: String,
        application_id: String,
        client_id: String,
        viewport_width: u32,
        viewport_height: u32,
    },
    SessionClosed {
        session_id: String,
        application_id: String,
        client_id: String,
    },
    ConfigLoaded {
        path: PathBuf,
    },
    ConfigSaved {
        path: PathBuf,
    },
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
            format!(
                "event=control_plane_started bind_address={} port={port}",
                event_field_value(bind_address)
            )
        }
        ServerEvent::ControlPlaneStopped => "event=control_plane_stopped".to_string(),
        ServerEvent::ForegroundConnectionAccepted { peer_address } => {
            format!(
                "event=foreground_connection_accepted peer_address={}",
                event_field_value(peer_address)
            )
        }
        ServerEvent::ForegroundConnectionClosed { peer_address } => {
            format!(
                "event=foreground_connection_closed peer_address={}",
                event_field_value(peer_address)
            )
        }
        ServerEvent::SshTunnelStarted { process_id } => {
            format!("event=ssh_tunnel_started process_id={process_id}")
        }
        ServerEvent::SshTunnelStopped => "event=ssh_tunnel_stopped".to_string(),
        ServerEvent::SshTunnelFailed { reason } => {
            format!(
                "event=ssh_tunnel_failed reason={}",
                event_field_value(reason)
            )
        }
        ServerEvent::RequestAuthorized { operation } => {
            format!(
                "event=request_authorized operation={}",
                event_field_value(operation)
            )
        }
        ServerEvent::RequestRejected { operation } => {
            format!(
                "event=request_rejected operation={}",
                event_field_value(operation)
            )
        }
        ServerEvent::SessionCreated {
            session_id,
            application_id,
            client_id,
            viewport_width,
            viewport_height,
        } => {
            format!(
                "event=session_created session_id={} application_id={} client_id={} viewport_width={viewport_width} viewport_height={viewport_height}",
                event_field_value(session_id),
                event_field_value(application_id),
                event_field_value(client_id),
            )
        }
        ServerEvent::SessionResized {
            session_id,
            application_id,
            client_id,
            viewport_width,
            viewport_height,
        } => {
            format!(
                "event=session_resized session_id={} application_id={} client_id={} viewport_width={viewport_width} viewport_height={viewport_height}",
                event_field_value(session_id),
                event_field_value(application_id),
                event_field_value(client_id),
            )
        }
        ServerEvent::SessionClosed {
            session_id,
            application_id,
            client_id,
        } => {
            format!(
                "event=session_closed session_id={} application_id={} client_id={}",
                event_field_value(session_id),
                event_field_value(application_id),
                event_field_value(client_id),
            )
        }
        ServerEvent::ConfigLoaded { path } => {
            format!(
                "event=config_loaded path={}",
                event_field_value(&path.display().to_string())
            )
        }
        ServerEvent::ConfigSaved { path } => {
            format!(
                "event=config_saved path={}",
                event_field_value(&path.display().to_string())
            )
        }
    }
}

fn event_field_value(value: &str) -> String {
    let mut encoded = String::new();

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':' | b'/' | b'[' | b']')
        {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryApplicationSessionService {
    policy: SessionPolicy,
    launch_backend: ApplicationLaunchBackendService,
    resize_backend: WindowResizeBackendService,
    sessions: Vec<ApplicationSession>,
    next_session_sequence: u64,
}

impl InMemoryApplicationSessionService {
    pub fn new(policy: SessionPolicy) -> Self {
        Self::with_backends(
            policy,
            ApplicationLaunchBackendService::RecordOnly,
            WindowResizeBackendService::RecordOnly,
        )
    }

    pub fn with_resize_backend(
        policy: SessionPolicy,
        resize_backend: WindowResizeBackendService,
    ) -> Self {
        Self::with_backends(
            policy,
            ApplicationLaunchBackendService::RecordOnly,
            resize_backend,
        )
    }

    pub fn with_launch_backend(
        policy: SessionPolicy,
        launch_backend: ApplicationLaunchBackendService,
    ) -> Self {
        Self::with_backends(
            policy,
            launch_backend,
            WindowResizeBackendService::RecordOnly,
        )
    }

    pub fn with_backends(
        policy: SessionPolicy,
        launch_backend: ApplicationLaunchBackendService,
        resize_backend: WindowResizeBackendService,
    ) -> Self {
        Self {
            policy,
            launch_backend,
            resize_backend,
            sessions: Vec::new(),
            next_session_sequence: 1,
        }
    }

    fn next_session_id(&mut self) -> String {
        let id = format!("session-{}", self.next_session_sequence);
        self.next_session_sequence += 1;
        id
    }

    pub fn create_session_for_application(
        &mut self,
        request: CreateSessionRequest,
        application: ApplicationSummary,
    ) -> Result<ApplicationSession, AppRelayError> {
        if request.application_id != application.id {
            return Err(AppRelayError::InvalidRequest(format!(
                "application {} does not match request {}",
                application.id, request.application_id
            )));
        }

        self.create_validated_session(request, application)
    }

    fn create_validated_session(
        &mut self,
        request: CreateSessionRequest,
        application: ApplicationSummary,
    ) -> Result<ApplicationSession, AppRelayError> {
        self.policy
            .validate_application(&request.application_id)
            .and_then(|_| self.policy.validate_viewport(&request.viewport))?;

        let session_id = self.next_session_id();
        let launch_intent = self
            .launch_backend
            .prepare_launch(&application, &session_id)?;
        let selection_method = match launch_intent.status {
            LaunchIntentStatus::Recorded => WindowSelectionMethod::LaunchIntent,
            LaunchIntentStatus::Attached => WindowSelectionMethod::ExistingWindow,
            LaunchIntentStatus::Unsupported => WindowSelectionMethod::Synthetic,
        };
        let session = ApplicationSession {
            id: session_id.clone(),
            application_id: application.id.clone(),
            selected_window: SelectedWindow {
                id: format!("window-{session_id}"),
                application_id: application.id,
                title: application.name,
                selection_method,
            },
            launch_intent: Some(launch_intent),
            viewport: request.viewport,
            resize_intent: None,
            state: SessionState::Ready,
        };
        self.sessions.push(session.clone());

        Ok(session)
    }
}

impl Default for InMemoryApplicationSessionService {
    fn default() -> Self {
        Self::new(SessionPolicy::allow_all())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryClientAuthorizationService {
    authorized_clients: Vec<AuthorizedClient>,
    pending_pairings: Vec<PendingPairing>,
    next_pairing_sequence: u64,
}

impl InMemoryClientAuthorizationService {
    pub fn new(authorized_clients: Vec<AuthorizedClient>) -> Self {
        Self {
            authorized_clients,
            pending_pairings: Vec::new(),
            next_pairing_sequence: 1,
        }
    }

    fn validate_identity(identity: &ControlClientIdentity) -> Result<(), AppRelayError> {
        if identity.id.trim().is_empty() {
            return Err(AppRelayError::InvalidRequest(
                "client id is required".to_string(),
            ));
        }

        if identity.label.trim().is_empty() {
            return Err(AppRelayError::InvalidRequest(
                "client label is required".to_string(),
            ));
        }

        Ok(())
    }

    fn next_request_id(&mut self) -> String {
        let id = format!("pairing-{}", self.next_pairing_sequence);
        self.next_pairing_sequence += 1;
        id
    }
}

impl Default for InMemoryClientAuthorizationService {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl ClientAuthorizationService for InMemoryClientAuthorizationService {
    fn request_pairing(
        &mut self,
        request: PairingRequest,
    ) -> Result<PendingPairing, AppRelayError> {
        Self::validate_identity(&request.client)?;

        if self
            .authorized_clients
            .iter()
            .any(|client| client.id == request.client.id)
        {
            return Ok(PendingPairing {
                request_id: "already-authorized".to_string(),
                client: request.client,
                status: PairingRequestStatus::Approved,
            });
        }

        if let Some(existing) = self
            .pending_pairings
            .iter()
            .find(|pending| pending.client.id == request.client.id)
        {
            return Ok(existing.clone());
        }

        let pending = PendingPairing {
            request_id: self.next_request_id(),
            client: request.client,
            status: PairingRequestStatus::PendingUserApproval,
        };
        self.pending_pairings.push(pending.clone());
        Ok(pending)
    }

    fn approve_pairing(
        &mut self,
        request: ApprovePairingRequest,
    ) -> Result<AuthorizedClient, AppRelayError> {
        let Some(index) = self
            .pending_pairings
            .iter()
            .position(|pending| pending.request_id == request.request_id)
        else {
            return Err(AppRelayError::NotFound(format!(
                "pairing request {} was not found",
                request.request_id
            )));
        };

        let pending = self.pending_pairings.remove(index);
        let authorized = AuthorizedClient::from(pending.client);
        self.authorized_clients
            .retain(|client| client.id != authorized.id);
        self.authorized_clients.push(authorized.clone());
        Ok(authorized)
    }

    fn authorize_client(&self, client_id: Option<&str>) -> Result<AuthorizedClient, AppRelayError> {
        let Some(client_id) = client_id else {
            return Err(AppRelayError::PermissionDenied(
                "client identity is required".to_string(),
            ));
        };

        self.authorized_clients
            .iter()
            .find(|client| client.id == client_id)
            .cloned()
            .ok_or_else(|| {
                AppRelayError::PermissionDenied(format!("client {client_id} is not paired"))
            })
    }

    fn authorized_clients(&self) -> Vec<AuthorizedClient> {
        self.authorized_clients.clone()
    }

    fn pending_pairings(&self) -> Vec<PendingPairing> {
        self.pending_pairings.clone()
    }
}

impl ApplicationSessionService for InMemoryApplicationSessionService {
    fn create_session(
        &mut self,
        request: CreateSessionRequest,
    ) -> Result<ApplicationSession, AppRelayError> {
        let application = ApplicationSummary {
            id: request.application_id.clone(),
            name: request.application_id.clone(),
            icon: None,
            launch: None,
        };

        self.create_validated_session(request, application)
    }

    fn resize_session(
        &mut self,
        request: ResizeSessionRequest,
    ) -> Result<ApplicationSession, AppRelayError> {
        self.policy.validate_viewport(&request.viewport)?;
        let session = self
            .sessions
            .iter_mut()
            .find(|session| {
                session.id == request.session_id && session.state != SessionState::Closed
            })
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("session {} was not found", request.session_id))
            })?;
        let intent = WindowResizeIntent {
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            viewport: request.viewport.clone(),
            status: self
                .resize_backend
                .resize_window(&session.selected_window, &request.viewport)?,
        };

        session.viewport = request.viewport;
        session.resize_intent = Some(intent);
        Ok(session.clone())
    }

    fn close_session(&mut self, session_id: &str) -> Result<ApplicationSession, AppRelayError> {
        let session = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id && session.state != SessionState::Closed)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("session {session_id} was not found"))
            })?;

        session.state = SessionState::Closed;
        Ok(session.clone())
    }

    fn active_sessions(&self) -> Vec<ApplicationSession> {
        self.sessions
            .iter()
            .filter(|session| session.state != SessionState::Closed)
            .cloned()
            .collect()
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
            authorized_clients: Vec::new(),
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

        for client in &self.authorized_clients {
            client.validate()?;
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
    MissingAuthorizedClientId,
    MissingAuthorizedClientLabel,
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
        format!(
            "authorized_clients={}",
            encode_authorized_clients(&config.authorized_clients)
        ),
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
    let mut authorized_clients = Vec::new();

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
            "authorized_clients" => authorized_clients = decode_authorized_clients(value)?,
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
        authorized_clients,
    })
}

fn encode_authorized_clients(clients: &[AuthorizedClient]) -> String {
    clients
        .iter()
        .map(|client| {
            format!(
                "{}:{}",
                encode_config_list_field(&client.id),
                encode_config_list_field(&client.label)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_authorized_clients(value: &str) -> Result<Vec<AuthorizedClient>, ConfigStoreError> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }

    value
        .split(',')
        .map(|encoded| {
            let Some((id, label)) = encoded.split_once(':') else {
                return Err(ConfigStoreError::CorruptedStore);
            };
            let client = AuthorizedClient {
                id: decode_config_list_field(id)?,
                label: decode_config_list_field(label)?,
            };
            client.validate()?;
            Ok(client)
        })
        .collect()
}

fn encode_config_list_field(value: &str) -> String {
    encode_field(value)
        .replace('%', "%25")
        .replace(',', "%2C")
        .replace(':', "%3A")
}

fn decode_config_list_field(value: &str) -> Result<String, ConfigStoreError> {
    let mut decoded = String::new();
    let mut chars = value.chars().peekable();

    while let Some(character) = chars.next() {
        if character != '%' {
            decoded.push(character);
            continue;
        }

        let Some(first) = chars.next() else {
            return Err(ConfigStoreError::CorruptedStore);
        };
        let Some(second) = chars.next() else {
            return Err(ConfigStoreError::CorruptedStore);
        };
        match (first, second) {
            ('2', '5') => decoded.push('%'),
            ('2', 'C') => decoded.push(','),
            ('3', 'A') => decoded.push(':'),
            _ => {
                decoded.push('%');
                decoded.push(first);
                decoded.push(second);
            }
        }
    }

    decode_config_field(&decoded)
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
        let audio_reason = match self.platform {
            Platform::Linux => {
                "Linux desktop audio control-plane is available; native PipeWire capture/playback backend is planned"
            }
            Platform::Macos => {
                "macOS desktop audio control-plane is available; native CoreAudio capture/playback backend is planned"
            }
            Platform::Windows => {
                "Windows desktop audio control-plane is available; native WASAPI capture/playback backend is planned"
            }
            Platform::Android | Platform::Ios => {
                "mobile platforms are client targets and do not expose desktop audio control-plane capture"
            }
            Platform::Unknown => "unknown platform cannot expose desktop audio control-plane capture",
        };
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

        let application_launch = match self.platform {
            Platform::Linux => PlatformCapability::supported_with_reason(
                self.platform,
                Feature::ApplicationLaunch,
                "Linux launches discovered .desktop Exec commands without a shell",
            ),
            Platform::Macos => PlatformCapability::supported_with_reason(
                self.platform,
                Feature::ApplicationLaunch,
                "macOS launches discovered .app bundles through the native open command",
            ),
            Platform::Windows => PlatformCapability::unsupported(
                self.platform,
                Feature::ApplicationLaunch,
                "native launch backend records launch or attach intent but does not spawn applications yet",
            ),
            Platform::Android | Platform::Ios => PlatformCapability::unsupported(
                self.platform,
                Feature::ApplicationLaunch,
                "mobile platforms are client targets and do not expose desktop application launch",
            ),
            Platform::Unknown => PlatformCapability::unsupported(
                self.platform,
                Feature::ApplicationLaunch,
                "unknown platform cannot expose desktop application launch",
            ),
        };

        vec![
            app_discovery,
            application_launch,
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
            if matches!(
                self.platform,
                Platform::Linux | Platform::Macos | Platform::Windows
            ) {
                PlatformCapability::supported_with_reason(
                    self.platform,
                    Feature::SystemAudioStream,
                    audio_reason,
                )
            } else {
                PlatformCapability::unsupported(
                    self.platform,
                    Feature::SystemAudioStream,
                    audio_reason,
                )
            },
            if matches!(
                self.platform,
                Platform::Linux | Platform::Macos | Platform::Windows
            ) {
                PlatformCapability::supported_with_reason(
                    self.platform,
                    Feature::ClientMicrophoneInput,
                    audio_reason,
                )
            } else {
                PlatformCapability::unsupported(
                    self.platform,
                    Feature::ClientMicrophoneInput,
                    audio_reason,
                )
            },
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
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, AppRelayError> {
        Err(AppRelayError::unsupported(
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
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, AppRelayError> {
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
    let mut icon = None;
    let mut exec = None;

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
            "Icon" => icon = Some(value.trim().to_string()),
            "Exec" => exec = Some(value.trim().to_string()),
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
        icon: icon
            .filter(|value| !value.is_empty())
            .map(|source| AppIcon {
                mime_type: "application/x-icon-theme-name".to_string(),
                bytes: Vec::new(),
                source: Some(source),
            }),
        launch: exec
            .filter(|value| !value.is_empty())
            .map(|command| ApplicationLaunch::DesktopCommand { command }),
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
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, AppRelayError> {
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
    let info = plist::Value::from_file(info_plist).ok()?;
    let info = info.as_dictionary()?;

    let id = plist_dictionary_string_value(info, "CFBundleIdentifier").or_else(|| {
        path.file_stem()
            .map(|value| value.to_string_lossy().into_owned())
    })?;
    let name = plist_dictionary_string_value(info, "CFBundleDisplayName")
        .or_else(|| plist_dictionary_string_value(info, "CFBundleName"))
        .or_else(|| {
            path.file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })?;
    let icon = plist_dictionary_string_value(info, "CFBundleIconFile");

    if id.trim().is_empty() || name.trim().is_empty() {
        return None;
    }

    Some(ApplicationSummary {
        id,
        name,
        icon: icon
            .filter(|value| !value.is_empty())
            .map(|source| AppIcon {
                mime_type: "application/x-macos-icon-name".to_string(),
                bytes: Vec::new(),
                source: Some(source),
            }),
        launch: Some(ApplicationLaunch::MacosBundle {
            bundle_path: path.display().to_string(),
        }),
    })
}

fn plist_dictionary_string_value(info: &plist::Dictionary, key: &str) -> Option<String> {
    info.get(key)
        .and_then(|value| value.as_string())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_health_service_returns_configured_status() {
        let service = StaticHealthService::new("apprelay-server", "0.1.0");

        assert_eq!(
            service.status(),
            HealthStatus::healthy("apprelay-server", "0.1.0")
        );
    }

    #[test]
    fn windows_capabilities_keep_non_audio_gaps_explicit() {
        let service = DefaultCapabilityService::new(Platform::Windows);
        let capabilities = service.platform_capabilities();

        assert_eq!(capabilities.len(), 8);
        assert!(capabilities
            .iter()
            .all(|capability| capability.platform == Platform::Windows));
        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::SystemAudioStream && capability.supported
        }));
        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::ClientMicrophoneInput && capability.supported
        }));
        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::AppDiscovery && !capability.supported
        }));
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
    fn linux_capabilities_support_application_launch() {
        let service = DefaultCapabilityService::new(Platform::Linux);
        let capabilities = service.platform_capabilities();

        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::ApplicationLaunch && capability.supported
        }));
    }

    #[test]
    fn platform_capability_matrix_covers_every_target_feature() {
        let platforms = [
            Platform::Linux,
            Platform::Macos,
            Platform::Windows,
            Platform::Android,
            Platform::Ios,
            Platform::Unknown,
        ];
        let features = [
            Feature::AppDiscovery,
            Feature::ApplicationLaunch,
            Feature::WindowResize,
            Feature::WindowVideoStream,
            Feature::SystemAudioStream,
            Feature::ClientMicrophoneInput,
            Feature::KeyboardInput,
            Feature::MouseInput,
        ];

        for platform in platforms {
            let service = DefaultCapabilityService::new(platform);
            let capabilities = service.platform_capabilities();

            assert_eq!(capabilities.len(), features.len());
            assert!(capabilities
                .iter()
                .all(|capability| capability.platform == platform));

            for feature in features.iter().cloned() {
                assert_eq!(
                    capabilities
                        .iter()
                        .filter(|capability| capability.feature == feature)
                        .count(),
                    1,
                    "expected exactly one {feature:?} capability for {platform:?}"
                );
            }
        }
    }

    #[test]
    fn unsupported_capabilities_include_user_facing_reasons() {
        let platforms = [
            Platform::Linux,
            Platform::Macos,
            Platform::Windows,
            Platform::Android,
            Platform::Ios,
            Platform::Unknown,
        ];

        for platform in platforms {
            let service = DefaultCapabilityService::new(platform);

            for capability in service.platform_capabilities() {
                if !capability.supported {
                    assert!(
                        capability
                            .reason
                            .as_deref()
                            .is_some_and(|reason| !reason.trim().is_empty()),
                        "unsupported {platform:?} {:?} capability needs a reason",
                        capability.feature
                    );
                }
            }
        }
    }

    #[test]
    fn desktop_audio_capabilities_report_planned_native_backend() {
        let cases = [
            (Platform::Linux, "PipeWire"),
            (Platform::Macos, "CoreAudio"),
            (Platform::Windows, "WASAPI"),
        ];

        for (platform, expected_backend) in cases {
            let service = DefaultCapabilityService::new(platform);
            let capabilities = service.platform_capabilities();
            let system_audio = capabilities
                .iter()
                .find(|capability| capability.feature == Feature::SystemAudioStream)
                .expect("system audio capability");
            let microphone = capabilities
                .iter()
                .find(|capability| capability.feature == Feature::ClientMicrophoneInput)
                .expect("microphone capability");

            assert!(system_audio.supported);
            assert!(microphone.supported);
            assert!(system_audio
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains(expected_backend)));
            assert_eq!(system_audio.reason, microphone.reason);
        }
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
            Err(AppRelayError::unsupported(
                Platform::Windows,
                Feature::AppDiscovery
            ))
        );
    }

    #[test]
    fn desktop_exec_parser_strips_field_codes_and_preserves_literal_percent() {
        assert_eq!(
            parse_desktop_exec("app --file %f --urls %U %%").expect("parse desktop exec"),
            vec!["app", "--file", "--urls", "%"]
        );
    }

    #[test]
    fn desktop_exec_parser_preserves_quoted_arguments() {
        assert_eq!(
            parse_desktop_exec(r#"app --title "Two Words" "quoted %c value" escaped\ space"#)
                .expect("parse desktop exec"),
            vec![
                "app",
                "--title",
                "Two Words",
                "quoted  value",
                "escaped space"
            ]
        );
    }

    #[test]
    fn desktop_exec_parser_rejects_empty_command_after_field_code_stripping() {
        assert_eq!(
            parse_desktop_exec("%f %U"),
            Err(AppRelayError::InvalidRequest(
                "desktop Exec command has no executable after field-code stripping".to_string()
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
        config.authorized_clients = vec![AuthorizedClient::new("client-1", "Test Client")];

        repository.save(&config).expect("save server config");

        assert_eq!(repository.load().expect("load server config"), config);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn server_config_rejects_invalid_authorized_client() {
        let mut config = ServerConfig::local("test-token");
        config.authorized_clients = vec![AuthorizedClient::new(" ", "Test Client")];

        assert_eq!(
            config.validate(),
            Err(ConfigError::MissingAuthorizedClientId)
        );
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
        sink.record(ServerEvent::SessionCreated {
            session_id: "session 1".to_string(),
            application_id: "terminal".to_string(),
            client_id: "client-1".to_string(),
            viewport_width: 1280,
            viewport_height: 720,
        });

        let contents = fs::read_to_string(&path).expect("read event log");
        assert_eq!(
            contents,
            "event=control_plane_started bind_address=127.0.0.1 port=7676\n\
event=request_authorized operation=health\n\
event=session_created session_id=session%201 application_id=terminal client_id=client-1 viewport_width=1280 viewport_height=720\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn client_authorization_denies_unknown_clients_by_default() {
        let service = InMemoryClientAuthorizationService::default();

        assert_eq!(
            service.authorize_client(Some("client-1")),
            Err(AppRelayError::PermissionDenied(
                "client client-1 is not paired".to_string()
            ))
        );
        assert_eq!(
            service.authorize_client(None),
            Err(AppRelayError::PermissionDenied(
                "client identity is required".to_string()
            ))
        );
    }

    #[test]
    fn client_pairing_requires_explicit_approval_before_authorization() {
        let mut service = InMemoryClientAuthorizationService::default();
        let pending = service
            .request_pairing(PairingRequest {
                client: ControlClientIdentity {
                    id: "client-1".to_string(),
                    label: "Laptop".to_string(),
                },
            })
            .expect("request pairing");

        assert_eq!(pending.request_id, "pairing-1");
        assert_eq!(pending.status, PairingRequestStatus::PendingUserApproval);
        assert_eq!(
            service.authorize_client(Some("client-1")),
            Err(AppRelayError::PermissionDenied(
                "client client-1 is not paired".to_string()
            ))
        );

        let approved = service
            .approve_pairing(ApprovePairingRequest {
                request_id: pending.request_id,
            })
            .expect("approve pairing");

        assert_eq!(approved, AuthorizedClient::new("client-1", "Laptop"));
        assert_eq!(service.authorize_client(Some("client-1")), Ok(approved));
        assert!(service.pending_pairings().is_empty());
    }

    #[test]
    fn session_service_creates_session_for_allowed_application() {
        let mut service =
            InMemoryApplicationSessionService::new(SessionPolicy::allow_applications(vec![
                "terminal".to_string(),
            ]));

        let session = service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");

        assert_eq!(session.id, "session-1");
        assert_eq!(session.application_id, "terminal");
        assert_eq!(session.state, SessionState::Ready);
        assert_eq!(
            service.active_sessions(),
            vec![ApplicationSession {
                id: "session-1".to_string(),
                application_id: "terminal".to_string(),
                selected_window: SelectedWindow {
                    id: "window-session-1".to_string(),
                    application_id: "terminal".to_string(),
                    title: "terminal".to_string(),
                    selection_method: WindowSelectionMethod::ExistingWindow,
                },
                launch_intent: Some(ApplicationLaunchIntent {
                    session_id: "session-1".to_string(),
                    application_id: "terminal".to_string(),
                    launch: None,
                    status: LaunchIntentStatus::Attached,
                }),
                viewport: ViewportSize::new(1280, 720),
                resize_intent: None,
                state: SessionState::Ready,
            }]
        );
    }

    #[test]
    fn session_service_rejects_denied_application() {
        let mut service =
            InMemoryApplicationSessionService::new(SessionPolicy::allow_applications(vec![
                "terminal".to_string(),
            ]));

        assert_eq!(
            service.create_session(CreateSessionRequest {
                application_id: "browser".to_string(),
                viewport: ViewportSize::new(1280, 720),
            }),
            Err(AppRelayError::PermissionDenied(
                "application browser is not allowed".to_string()
            ))
        );
    }

    #[test]
    fn session_service_attaches_when_launch_metadata_is_absent() {
        let mut service = InMemoryApplicationSessionService::default();
        let session = service
            .create_session_for_application(
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
                ApplicationSummary {
                    id: "terminal".to_string(),
                    name: "Terminal".to_string(),
                    icon: None,
                    launch: None,
                },
            )
            .expect("create session");

        assert_eq!(
            session.selected_window.selection_method,
            WindowSelectionMethod::ExistingWindow
        );
        assert_eq!(
            session.launch_intent,
            Some(ApplicationLaunchIntent {
                session_id: "session-1".to_string(),
                application_id: "terminal".to_string(),
                launch: None,
                status: LaunchIntentStatus::Attached,
            })
        );
    }

    #[test]
    fn session_service_records_launch_intent_for_discovered_application() {
        let mut service = InMemoryApplicationSessionService::default();
        let session = service
            .create_session_for_application(
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
                ApplicationSummary {
                    id: "terminal".to_string(),
                    name: "Terminal".to_string(),
                    icon: None,
                    launch: Some(ApplicationLaunch::DesktopCommand {
                        command: "gnome-terminal".to_string(),
                    }),
                },
            )
            .expect("create session");

        assert_eq!(session.selected_window.title, "Terminal");
        assert_eq!(session.selected_window.application_id, "terminal");
        assert_eq!(
            session.selected_window.selection_method,
            WindowSelectionMethod::LaunchIntent
        );
        assert_eq!(
            session.launch_intent,
            Some(ApplicationLaunchIntent {
                session_id: "session-1".to_string(),
                application_id: "terminal".to_string(),
                launch: Some(ApplicationLaunch::DesktopCommand {
                    command: "gnome-terminal".to_string(),
                }),
                status: LaunchIntentStatus::Recorded,
            })
        );
    }

    #[test]
    #[cfg(unix)]
    fn linux_launch_backend_spawns_desktop_command_and_records_intent() {
        let root = unique_test_dir("linux-launch-backend");
        fs::create_dir_all(&root).expect("create test launch directory");
        let marker = root.join("launch-marker");
        let executable = root.join("fake-app");
        write_executable_script(
            &executable,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" > {}\n",
                marker.display()
            ),
        );

        let mut service = InMemoryApplicationSessionService::with_launch_backend(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::LinuxNative,
        );
        let session = service
            .create_session_for_application(
                CreateSessionRequest {
                    application_id: "fake".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
                ApplicationSummary {
                    id: "fake".to_string(),
                    name: "Fake App".to_string(),
                    icon: None,
                    launch: Some(ApplicationLaunch::DesktopCommand {
                        command: format!("{} --name \"Fake App\" %U", executable.display()),
                    }),
                },
            )
            .expect("create launched session");

        wait_for_path(&marker);
        assert_eq!(
            fs::read_to_string(&marker).expect("read launch marker"),
            "--name\nFake App\n"
        );
        assert_eq!(
            session.launch_intent.expect("launch intent").status,
            LaunchIntentStatus::Recorded
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linux_launch_backend_rejects_spawn_failure_without_ready_session() {
        let root = unique_test_dir("linux-launch-failure");
        let missing = root.join("missing-app");
        let mut service = InMemoryApplicationSessionService::with_launch_backend(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::LinuxNative,
        );

        let error = service
            .create_session_for_application(
                CreateSessionRequest {
                    application_id: "missing".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
                ApplicationSummary {
                    id: "missing".to_string(),
                    name: "Missing App".to_string(),
                    icon: None,
                    launch: Some(ApplicationLaunch::DesktopCommand {
                        command: missing.display().to_string(),
                    }),
                },
            )
            .expect_err("missing executable should fail launch");

        assert!(matches!(error, AppRelayError::ServiceUnavailable(_)));
        assert_eq!(service.active_sessions(), Vec::new());
    }

    #[test]
    #[cfg(unix)]
    fn macos_launch_backend_opens_app_bundle() {
        let root = unique_test_dir("macos-launch");
        fs::create_dir_all(&root).expect("create test root");
        let marker = root.join("open-marker");
        let open_command = root.join("fake-open");
        write_executable_script(
            &open_command,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" > {}\n",
                marker.display()
            ),
        );
        let bundle_path = root.join("Fake.app");
        let mut service = InMemoryApplicationSessionService::with_launch_backend(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::MacosNative {
                open_command: open_command.clone(),
            },
        );

        let session = service
            .create_session_for_application(
                CreateSessionRequest {
                    application_id: "dev.apprelay.fake".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
                ApplicationSummary {
                    id: "dev.apprelay.fake".to_string(),
                    name: "Fake Mac App".to_string(),
                    icon: None,
                    launch: Some(ApplicationLaunch::MacosBundle {
                        bundle_path: bundle_path.display().to_string(),
                    }),
                },
            )
            .expect("create launched macOS session");

        wait_for_path(&marker);
        assert_eq!(
            fs::read_to_string(&marker).expect("read launch marker"),
            format!("-n\n{}\n", bundle_path.display())
        );
        assert_eq!(
            session.launch_intent.expect("launch intent").status,
            LaunchIntentStatus::Recorded
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn macos_launch_backend_rejects_missing_open_command_without_ready_session() {
        let root = unique_test_dir("macos-launch-failure");
        let missing_open = root.join("missing-open");
        let mut service = InMemoryApplicationSessionService::with_launch_backend(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::MacosNative {
                open_command: missing_open,
            },
        );

        let error = service
            .create_session_for_application(
                CreateSessionRequest {
                    application_id: "dev.apprelay.fake".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
                ApplicationSummary {
                    id: "dev.apprelay.fake".to_string(),
                    name: "Fake Mac App".to_string(),
                    icon: None,
                    launch: Some(ApplicationLaunch::MacosBundle {
                        bundle_path: "/Applications/Fake.app".to_string(),
                    }),
                },
            )
            .expect_err("missing open command should fail launch");

        assert!(matches!(error, AppRelayError::ServiceUnavailable(_)));
        assert_eq!(service.active_sessions(), Vec::new());
    }

    #[test]
    fn session_service_reports_unsupported_launch_backend() {
        let mut service = InMemoryApplicationSessionService::with_launch_backend(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::Unsupported {
                platform: Platform::Linux,
            },
        );

        assert_eq!(
            service.create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            }),
            Err(AppRelayError::unsupported(
                Platform::Linux,
                Feature::ApplicationLaunch
            ))
        );
    }

    #[test]
    fn session_service_validates_resize_and_records_viewport() {
        let mut service = InMemoryApplicationSessionService::default();
        let session = service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");

        let resized = service
            .resize_session(ResizeSessionRequest {
                session_id: session.id,
                viewport: ViewportSize::new(1440, 900),
            })
            .expect("resize session");

        assert_eq!(resized.viewport, ViewportSize::new(1440, 900));
        assert_eq!(
            resized.resize_intent,
            Some(WindowResizeIntent {
                session_id: "session-1".to_string(),
                selected_window_id: "window-session-1".to_string(),
                viewport: ViewportSize::new(1440, 900),
                status: ResizeIntentStatus::Recorded,
            })
        );
    }

    #[test]
    fn session_service_reports_unsupported_resize_backend() {
        let mut service = InMemoryApplicationSessionService::with_resize_backend(
            SessionPolicy::allow_all(),
            WindowResizeBackendService::Unsupported {
                platform: Platform::Linux,
            },
        );
        let session = service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");

        assert_eq!(
            service.resize_session(ResizeSessionRequest {
                session_id: session.id,
                viewport: ViewportSize::new(1440, 900),
            }),
            Err(AppRelayError::unsupported(
                Platform::Linux,
                Feature::WindowResize
            ))
        );
    }

    #[test]
    fn session_service_closes_session_cleanly() {
        let mut service = InMemoryApplicationSessionService::default();
        let session = service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");

        let closed = service.close_session(&session.id).expect("close session");

        assert_eq!(closed.state, SessionState::Closed);
        assert_eq!(service.active_sessions(), Vec::new());
    }

    #[test]
    fn session_service_rejects_invalid_viewport() {
        let mut service = InMemoryApplicationSessionService::default();

        assert_eq!(
            service.create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(100, 100),
            }),
            Err(AppRelayError::InvalidRequest(
                "viewport must be at least 320x240".to_string()
            ))
        );
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
            "[Desktop Entry]\nType=Application\nName=Visible App\nExec=visible --new-window\nIcon=visible-icon\n",
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
                icon: Some(AppIcon {
                    mime_type: "application/x-icon-theme-name".to_string(),
                    bytes: Vec::new(),
                    source: Some("visible-icon".to_string()),
                }),
                launch: Some(ApplicationLaunch::DesktopCommand {
                    command: "visible --new-window".to_string(),
                }),
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
  <string>dev.apprelay.visible</string>
  <key>CFBundleDisplayName</key>
  <string>Visible Mac App</string>
  <key>CFBundleIconFile</key>
  <string>VisibleIcon</string>
</dict>
</plist>
"#,
        )
        .expect("write info plist");
        let broken_contents = root.join("Broken.app/Contents");
        fs::create_dir_all(&broken_contents).expect("create broken app bundle");
        fs::write(broken_contents.join("Info.plist"), b"not a plist")
            .expect("write malformed info plist");
        fs::create_dir_all(root.join("Ignored.txt")).expect("create ignored non-app directory");

        let discovery = MacosApplicationDiscovery::new(vec![root.clone()]);
        let applications = discovery
            .available_applications()
            .expect("discover macOS applications");

        assert_eq!(
            applications,
            vec![ApplicationSummary {
                id: "dev.apprelay.visible".to_string(),
                name: "Visible Mac App".to_string(),
                icon: Some(AppIcon {
                    mime_type: "application/x-macos-icon-name".to_string(),
                    bytes: Vec::new(),
                    source: Some("VisibleIcon".to_string()),
                }),
                launch: Some(ApplicationLaunch::MacosBundle {
                    bundle_path: root.join("Visible.app").display().to_string(),
                }),
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
                launch: Some(ApplicationLaunch::MacosBundle {
                    bundle_path: root.join("Fallback.app").display().to_string(),
                }),
            }]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn macos_application_discovery_reads_binary_info_plist() {
        let root = unique_test_dir("macos-app-binary-plist");
        let app_contents = root.join("Binary.app/Contents");
        fs::create_dir_all(&app_contents).expect("create app bundle");

        let mut info = plist::Dictionary::new();
        info.insert(
            "CFBundleIdentifier".to_string(),
            plist::Value::String("dev.apprelay.binary".to_string()),
        );
        info.insert(
            "CFBundleName".to_string(),
            plist::Value::String("Binary Mac App".to_string()),
        );
        info.insert(
            "CFBundleIconFile".to_string(),
            plist::Value::String("BinaryIcon".to_string()),
        );
        plist::Value::Dictionary(info)
            .to_file_binary(app_contents.join("Info.plist"))
            .expect("write binary info plist");

        let discovery = MacosApplicationDiscovery::new(vec![root.clone()]);
        let applications = discovery
            .available_applications()
            .expect("discover macOS applications");

        assert_eq!(
            applications,
            vec![ApplicationSummary {
                id: "dev.apprelay.binary".to_string(),
                name: "Binary Mac App".to_string(),
                icon: Some(AppIcon {
                    mime_type: "application/x-macos-icon-name".to_string(),
                    bytes: Vec::new(),
                    source: Some("BinaryIcon".to_string()),
                }),
                launch: Some(ApplicationLaunch::MacosBundle {
                    bundle_path: root.join("Binary.app").display().to_string(),
                }),
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
    fn application_permission_validation_rejects_missing_application_id() {
        let permission = ApplicationPermission {
            application_id: " ".to_string(),
            label: "Terminal".to_string(),
        };

        assert_eq!(
            permission.validate(),
            Err(PermissionValidationError::MissingApplicationId)
        );
    }

    #[test]
    fn session_policy_can_be_built_from_permissions() {
        let policy = SessionPolicy::from_permissions(&[test_permission("terminal", "Terminal")]);

        assert_eq!(policy.validate_application("terminal"), Ok(()));
        assert_eq!(
            policy.validate_application("browser"),
            Err(AppRelayError::PermissionDenied(
                "application browser is not allowed".to_string()
            ))
        );
    }

    #[test]
    fn file_application_permission_repository_persists_permissions() {
        let root = unique_test_dir("application-permission-store");
        let repository = FileApplicationPermissionRepository::new(root.join("permissions.tsv"));

        repository
            .save(test_permission("zed", "Zed"))
            .expect("save zed permission");
        repository
            .save(test_permission("terminal", "Terminal"))
            .expect("save terminal permission");

        let permissions = repository.list().expect("list permissions");
        assert_eq!(
            permissions
                .iter()
                .map(|permission| permission.application_id.as_str())
                .collect::<Vec<_>>(),
            vec!["terminal", "zed"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_application_permission_repository_replaces_permissions_by_application_id() {
        let root = unique_test_dir("application-permission-replace");
        let repository = FileApplicationPermissionRepository::new(root.join("permissions.tsv"));

        repository
            .save(test_permission("terminal", "Terminal"))
            .expect("save original permission");
        repository
            .save(test_permission("terminal", "Terminal Updated"))
            .expect("replace permission");

        assert_eq!(
            repository.list().expect("list permissions"),
            vec![test_permission("terminal", "Terminal Updated")]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_application_permission_repository_removes_permissions_by_application_id() {
        let root = unique_test_dir("application-permission-remove");
        let repository = FileApplicationPermissionRepository::new(root.join("permissions.tsv"));

        repository
            .save(test_permission("terminal", "Terminal"))
            .expect("save permission");
        repository.remove("terminal").expect("remove permission");

        assert_eq!(repository.list().expect("list permissions"), Vec::new());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_application_permission_repository_reports_corruption() {
        let root = unique_test_dir("application-permission-corrupt");
        let path = root.join("permissions.tsv");
        fs::create_dir_all(&root).expect("create permission store dir");
        fs::write(&path, "bad data").expect("write corrupted permission store");

        let repository = FileApplicationPermissionRepository::new(path);

        assert_eq!(repository.list(), Err(PermissionStoreError::CorruptedStore));

        let _ = fs::remove_dir_all(root);
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

    fn test_permission(application_id: &str, label: &str) -> ApplicationPermission {
        ApplicationPermission {
            application_id: application_id.to_string(),
            label: label.to_string(),
        }
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
    }

    #[cfg(unix)]
    fn write_executable_script(path: &Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, contents).expect("write executable script");
        let mut permissions = fs::metadata(path)
            .expect("read executable script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("mark executable script");
    }

    #[cfg(unix)]
    fn wait_for_path(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                return;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        panic!("timed out waiting for {}", path.display());
    }
}
