//! Server composition for Swavan AppRelay.

mod video_stream;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use swavan_core::{
    ApplicationDiscovery, ApplicationSessionService, CapabilityService, DefaultCapabilityService,
    DesktopEntryApplicationDiscovery, EventSink, HealthService, InMemoryApplicationSessionService,
    MacosApplicationDiscovery, ServerConfig, ServerEvent, SessionPolicy, StaticHealthService,
    UnsupportedApplicationDiscovery,
};
use swavan_protocol::{
    ApplicationSession, ApplicationSummary, ControlAuth, ControlError, ControlResult,
    CreateSessionRequest, HealthStatus, HeartbeatStatus, Platform, PlatformCapability,
    ResizeSessionRequest, ServerVersion, StartVideoStreamRequest, StopVideoStreamRequest,
    SwavanError, VideoStreamSession,
};

use crate::video_stream::VideoStreamControl;

#[derive(Debug)]
pub struct ServerServices {
    health_service: StaticHealthService,
    capability_service: DefaultCapabilityService,
    application_discovery: ApplicationDiscoveryService,
    session_service: InMemoryApplicationSessionService,
    video_stream: VideoStreamControl,
    platform: Platform,
    version: String,
}

impl ServerServices {
    pub fn new(platform: Platform, version: impl Into<String>) -> Self {
        let version = version.into();

        Self {
            health_service: StaticHealthService::new("swavan-server", version.clone()),
            capability_service: DefaultCapabilityService::new(platform),
            application_discovery: ApplicationDiscoveryService::for_platform(platform),
            session_service: InMemoryApplicationSessionService::new(SessionPolicy::allow_all()),
            video_stream: VideoStreamControl::new(),
            platform,
            version,
        }
    }

    pub fn for_current_platform() -> Self {
        Self::new(Platform::current(), env!("CARGO_PKG_VERSION"))
    }

    pub fn health(&self) -> HealthStatus {
        self.health_service.status()
    }

    pub fn capabilities(&self) -> Vec<PlatformCapability> {
        self.capability_service.platform_capabilities()
    }

    pub fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError> {
        self.application_discovery.available_applications()
    }

    pub fn create_session(
        &mut self,
        request: CreateSessionRequest,
    ) -> Result<ApplicationSession, SwavanError> {
        let application = self
            .application_discovery
            .available_applications()
            .ok()
            .and_then(|applications| {
                applications
                    .into_iter()
                    .find(|application| application.id == request.application_id)
            });

        match application {
            Some(application) => self
                .session_service
                .create_session_for_application(request, application),
            None => self.session_service.create_session(request),
        }
    }

    pub fn resize_session(
        &mut self,
        request: ResizeSessionRequest,
    ) -> Result<ApplicationSession, SwavanError> {
        self.session_service.resize_session(request)
    }

    pub fn close_session(&mut self, session_id: &str) -> Result<ApplicationSession, SwavanError> {
        self.session_service.close_session(session_id)
    }

    pub fn active_sessions(&self) -> Vec<ApplicationSession> {
        self.session_service.active_sessions()
    }

    pub fn start_video_stream(
        &mut self,
        request: StartVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError> {
        self.video_stream
            .start(request, &self.session_service.active_sessions())
    }

    pub fn stop_video_stream(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, SwavanError> {
        self.video_stream.stop(request)
    }

    pub fn video_stream_status(&self, stream_id: &str) -> Result<VideoStreamSession, SwavanError> {
        self.video_stream.status(stream_id)
    }

    pub fn version(&self) -> ServerVersion {
        ServerVersion::new("swavan-server", self.version.clone(), self.platform)
    }
}

#[derive(Clone, Debug)]
enum ApplicationDiscoveryService {
    DesktopEntries(DesktopEntryApplicationDiscovery),
    MacosApplications(MacosApplicationDiscovery),
    Unsupported(UnsupportedApplicationDiscovery),
}

impl ApplicationDiscoveryService {
    fn for_platform(platform: Platform) -> Self {
        match platform {
            Platform::Linux => {
                Self::DesktopEntries(DesktopEntryApplicationDiscovery::linux_defaults())
            }
            Platform::Macos => Self::MacosApplications(MacosApplicationDiscovery::macos_defaults()),
            Platform::Windows | Platform::Android | Platform::Ios | Platform::Unknown => {
                Self::Unsupported(UnsupportedApplicationDiscovery::new(platform))
            }
        }
    }
}

impl ApplicationDiscovery for ApplicationDiscoveryService {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError> {
        match self {
            Self::DesktopEntries(discovery) => discovery.available_applications(),
            Self::MacosApplications(discovery) => discovery.available_applications(),
            Self::Unsupported(discovery) => discovery.available_applications(),
        }
    }
}

#[derive(Debug)]
pub struct ServerControlPlane {
    services: ServerServices,
    config: ServerConfig,
    heartbeat_sequence: AtomicU64,
}

impl ServerControlPlane {
    pub fn new(services: ServerServices, config: ServerConfig) -> Self {
        Self {
            services,
            config,
            heartbeat_sequence: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    pub fn health(&self, auth: &ControlAuth) -> ControlResult<HealthStatus> {
        self.authorize(auth)?;
        Ok(self.services.health())
    }

    pub fn capabilities(&self, auth: &ControlAuth) -> ControlResult<Vec<PlatformCapability>> {
        self.authorize(auth)?;
        Ok(self.services.capabilities())
    }

    pub fn version(&self, auth: &ControlAuth) -> ControlResult<ServerVersion> {
        self.authorize(auth)?;
        Ok(self.services.version())
    }

    pub fn available_applications(
        &self,
        auth: &ControlAuth,
    ) -> ControlResult<Vec<ApplicationSummary>> {
        self.authorize(auth)?;
        self.services.available_applications().map_err(Into::into)
    }

    pub fn create_session(
        &mut self,
        auth: &ControlAuth,
        request: CreateSessionRequest,
    ) -> ControlResult<ApplicationSession> {
        self.authorize(auth)?;
        self.services.create_session(request).map_err(Into::into)
    }

    pub fn resize_session(
        &mut self,
        auth: &ControlAuth,
        request: ResizeSessionRequest,
    ) -> ControlResult<ApplicationSession> {
        self.authorize(auth)?;
        self.services.resize_session(request).map_err(Into::into)
    }

    pub fn close_session(
        &mut self,
        auth: &ControlAuth,
        session_id: &str,
    ) -> ControlResult<ApplicationSession> {
        self.authorize(auth)?;
        self.services.close_session(session_id).map_err(Into::into)
    }

    pub fn active_sessions(&self, auth: &ControlAuth) -> ControlResult<Vec<ApplicationSession>> {
        self.authorize(auth)?;
        Ok(self.services.active_sessions())
    }

    pub fn start_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: StartVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize(auth)?;
        self.services
            .start_video_stream(request)
            .map_err(Into::into)
    }

    pub fn stop_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: StopVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize(auth)?;
        self.services.stop_video_stream(request).map_err(Into::into)
    }

    pub fn video_stream_status(
        &self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize(auth)?;
        self.services
            .video_stream_status(stream_id)
            .map_err(Into::into)
    }

    pub fn heartbeat(&self, auth: &ControlAuth) -> ControlResult<HeartbeatStatus> {
        self.authorize(auth)?;
        let sequence = self.heartbeat_sequence.fetch_add(1, Ordering::Relaxed) + 1;

        Ok(HeartbeatStatus {
            healthy: true,
            sequence,
        })
    }

    fn authorize(&self, auth: &ControlAuth) -> Result<(), ControlError> {
        if auth.token() == self.config.auth_token {
            Ok(())
        } else {
            Err(ControlError::Unauthorized)
        }
    }
}

pub struct ForegroundControlServer {
    control_plane: ServerControlPlane,
}

impl ForegroundControlServer {
    pub fn new(control_plane: ServerControlPlane) -> Self {
        Self { control_plane }
    }

    pub fn bind_address(&self) -> String {
        format!(
            "{}:{}",
            self.control_plane.config().bind_address,
            self.control_plane.config().control_port
        )
    }

    pub fn run_once(
        &self,
        listener: TcpListener,
        events: &mut impl EventSink,
    ) -> std::io::Result<()> {
        events.record(ServerEvent::ControlPlaneStarted {
            bind_address: self.control_plane.config().bind_address.clone(),
            port: self.control_plane.config().control_port,
        });

        let (mut stream, _) = listener.accept()?;
        self.handle_stream(&mut stream, events)?;
        events.record(ServerEvent::ControlPlaneStopped);
        Ok(())
    }

    pub fn handle_stream(
        &self,
        stream: &mut TcpStream,
        events: &mut impl EventSink,
    ) -> std::io::Result<()> {
        let mut request = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut request)?;
        let response = self.handle_request(request.trim(), events);

        stream.write_all(response.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(())
    }

    pub fn handle_request(&self, request: &str, events: &mut impl EventSink) -> String {
        let Some((operation, token)) = request.split_once(' ') else {
            return "ERROR bad-request".to_string();
        };

        let auth = ControlAuth::new(token);
        let result = match operation {
            "health" => self.control_plane.health(&auth).map(|health| {
                format!(
                    "OK health service={} version={} healthy={}",
                    health.service, health.version, health.healthy
                )
            }),
            "version" => self.control_plane.version(&auth).map(|version| {
                format!(
                    "OK version service={} version={} platform={:?}",
                    version.service, version.version, version.platform
                )
            }),
            "heartbeat" => self.control_plane.heartbeat(&auth).map(|heartbeat| {
                format!(
                    "OK heartbeat healthy={} sequence={}",
                    heartbeat.healthy, heartbeat.sequence
                )
            }),
            _ => return "ERROR unknown-operation".to_string(),
        };

        match result {
            Ok(response) => {
                events.record(ServerEvent::RequestAuthorized {
                    operation: operation.to_string(),
                });
                response
            }
            Err(ControlError::Unauthorized) => {
                events.record(ServerEvent::RequestRejected {
                    operation: operation.to_string(),
                });
                "ERROR unauthorized".to_string()
            }
            Err(ControlError::Service(_)) => "ERROR service".to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInstallPlan {
    pub platform: Platform,
    pub manifest_path: PathBuf,
    pub config_path: PathBuf,
    pub log_path: PathBuf,
    pub manifest_contents: String,
    pub start_command: String,
    pub stop_command: String,
    pub status_command: String,
    pub uninstall_command: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceInstallError {
    UnsupportedPlatform(Platform),
    MissingHomeDirectory,
    Io(std::io::ErrorKind),
}

impl From<std::io::Error> for ServiceInstallError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonServiceInstaller {
    executable_path: PathBuf,
}

impl DaemonServiceInstaller {
    pub fn new(executable_path: impl Into<PathBuf>) -> Self {
        Self {
            executable_path: executable_path.into(),
        }
    }

    pub fn plan_for_current_platform(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        self.plan_for_platform(Platform::current())
    }

    pub fn plan_for_platform(
        &self,
        platform: Platform,
    ) -> Result<ServiceInstallPlan, ServiceInstallError> {
        match platform {
            Platform::Linux => self.linux_user_systemd_plan(),
            Platform::Macos => self.macos_launch_agent_plan(),
            Platform::Windows => self.windows_service_script_plan(),
            Platform::Android | Platform::Ios | Platform::Unknown => {
                Err(ServiceInstallError::UnsupportedPlatform(platform))
            }
        }
    }

    pub fn install_manifest(&self, plan: &ServiceInstallPlan) -> Result<(), ServiceInstallError> {
        if let Some(parent) = plan.manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&plan.manifest_path, &plan.manifest_contents)?;
        Ok(())
    }

    fn linux_user_systemd_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        let config_path = xdg_config_home(&home).join("swavan/app-relay/server.conf");
        let log_path = xdg_state_home(&home).join("swavan/app-relay/server.log");
        let manifest_path = home.join(".config/systemd/user/swavan-app-relay.service");
        let executable_path = display_path(&self.executable_path);
        let config_arg = display_path(&config_path);
        let log_arg = display_path(&log_path);
        let manifest_contents = format!(
            "[Unit]\n\
Description=Swavan AppRelay server\n\
\n\
[Service]\n\
ExecStart={executable_path} --config {config_arg} --log {log_arg}\n\
Restart=on-failure\n\
RestartSec=3\n\
\n\
[Install]\n\
WantedBy=default.target\n"
        );

        Ok(ServiceInstallPlan {
            platform: Platform::Linux,
            manifest_path,
            config_path,
            log_path,
            manifest_contents,
            start_command: "systemctl --user start swavan-app-relay.service".to_string(),
            stop_command: "systemctl --user stop swavan-app-relay.service".to_string(),
            status_command: "systemctl --user status swavan-app-relay.service".to_string(),
            uninstall_command:
                "systemctl --user disable --now swavan-app-relay.service && rm ~/.config/systemd/user/swavan-app-relay.service && systemctl --user daemon-reload"
                    .to_string(),
        })
    }

    fn macos_launch_agent_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        let config_path = home.join("Library/Application Support/Swavan/AppRelay/server.conf");
        let log_path = home.join("Library/Logs/Swavan/AppRelay/server.log");
        let manifest_path = home.join("Library/LaunchAgents/com.swavan.apprelay.server.plist");
        let executable_path = xml_escape(&display_path(&self.executable_path));
        let config_arg = xml_escape(&display_path(&config_path));
        let log_arg = xml_escape(&display_path(&log_path));
        let manifest_arg = display_path(&manifest_path);
        let manifest_contents = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>Label</key>\n\
  <string>com.swavan.apprelay.server</string>\n\
  <key>ProgramArguments</key>\n\
  <array>\n\
    <string>{executable_path}</string>\n\
    <string>--config</string>\n\
    <string>{config_arg}</string>\n\
    <string>--log</string>\n\
    <string>{log_arg}</string>\n\
  </array>\n\
  <key>KeepAlive</key>\n\
  <true/>\n\
  <key>RunAtLoad</key>\n\
  <true/>\n\
</dict>\n\
</plist>\n"
        );

        Ok(ServiceInstallPlan {
            platform: Platform::Macos,
            manifest_path,
            config_path,
            log_path,
            manifest_contents,
            start_command: format!("launchctl bootstrap gui/$UID {manifest_arg}"),
            stop_command: "launchctl bootout gui/$UID/com.swavan.apprelay.server".to_string(),
            status_command: "launchctl print gui/$UID/com.swavan.apprelay.server".to_string(),
            uninstall_command: format!(
                "launchctl bootout gui/$UID/com.swavan.apprelay.server; rm {manifest_arg}"
            ),
        })
    }

    fn windows_service_script_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        let service_root = program_data.join("Swavan\\AppRelay");
        let config_path = service_root.join("server.conf");
        let log_path = service_root.join("server.log");
        let manifest_path = service_root.join("install-service.ps1");
        let executable_path = display_path(&self.executable_path);
        let config_arg = display_path(&config_path);
        let log_arg = display_path(&log_path);
        let manifest_contents = format!(
            "$ErrorActionPreference = 'Stop'\n\
$serviceName = 'SwavanAppRelay'\n\
$binaryPath = '\"{executable_path}\" --config \"{config_arg}\" --log \"{log_arg}\"'\n\
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {{\n\
  sc.exe stop $serviceName | Out-Null\n\
  sc.exe delete $serviceName | Out-Null\n\
}}\n\
sc.exe create $serviceName binPath= $binaryPath start= auto DisplayName= 'Swavan AppRelay Server'\n\
sc.exe start $serviceName\n"
        );

        Ok(ServiceInstallPlan {
            platform: Platform::Windows,
            manifest_path,
            config_path,
            log_path,
            manifest_contents,
            start_command: "sc.exe start SwavanAppRelay".to_string(),
            stop_command: "sc.exe stop SwavanAppRelay".to_string(),
            status_command: "sc.exe query SwavanAppRelay".to_string(),
            uninstall_command: "sc.exe stop SwavanAppRelay && sc.exe delete SwavanAppRelay"
                .to_string(),
        })
    }
}

fn home_dir() -> Result<PathBuf, ServiceInstallError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(ServiceInstallError::MissingHomeDirectory)
}

fn xdg_config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
}

fn xdg_state_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use swavan_core::InMemoryEventSink;

    #[test]
    fn server_services_report_health() {
        let services = ServerServices::new(Platform::Linux, "test");

        assert_eq!(
            services.health(),
            HealthStatus::healthy("swavan-server", "test")
        );
    }

    #[test]
    fn server_services_report_version() {
        let services = ServerServices::new(Platform::Linux, "test");

        assert_eq!(
            services.version(),
            ServerVersion::new("swavan-server", "test", Platform::Linux)
        );
    }

    #[test]
    fn server_services_report_capabilities_for_platform() {
        let services = ServerServices::new(Platform::Ios, "test");

        assert!(services
            .capabilities()
            .iter()
            .all(|capability| capability.platform == Platform::Ios));
    }

    #[test]
    fn server_services_expose_application_discovery_result() {
        let services = ServerServices::new(Platform::Android, "test");

        assert!(matches!(
            services.available_applications(),
            Err(SwavanError::UnsupportedPlatform { .. })
        ));
    }

    #[test]
    fn control_plane_rejects_bad_token() {
        let control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );

        assert_eq!(
            control_plane.health(&ControlAuth::new("wrong-token")),
            Err(ControlError::Unauthorized)
        );
    }

    #[test]
    fn control_plane_returns_health_for_valid_token() {
        let control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );

        assert_eq!(
            control_plane.health(&ControlAuth::new("correct-token")),
            Ok(HealthStatus::healthy("swavan-server", "test"))
        );
    }

    #[test]
    fn control_plane_heartbeat_increments_sequence() {
        let control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );
        let auth = ControlAuth::new("correct-token");

        assert_eq!(
            control_plane.heartbeat(&auth),
            Ok(HeartbeatStatus {
                healthy: true,
                sequence: 1
            })
        );
        assert_eq!(
            control_plane.heartbeat(&auth),
            Ok(HeartbeatStatus {
                healthy: true,
                sequence: 2
            })
        );
    }

    #[test]
    fn control_plane_creates_and_tracks_sessions() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );
        let auth = ControlAuth::new("correct-token");

        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: swavan_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");

        assert_eq!(session.application_id, "terminal");
        assert_eq!(control_plane.active_sessions(&auth), Ok(vec![session]));
    }

    #[test]
    fn control_plane_resizes_and_closes_sessions() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );
        let auth = ControlAuth::new("correct-token");
        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: swavan_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");

        let resized = control_plane
            .resize_session(
                &auth,
                ResizeSessionRequest {
                    session_id: session.id.clone(),
                    viewport: swavan_protocol::ViewportSize::new(1440, 900),
                },
            )
            .expect("resize session");
        let closed = control_plane
            .close_session(&auth, &session.id)
            .expect("close session");

        assert_eq!(
            resized.viewport,
            swavan_protocol::ViewportSize::new(1440, 900)
        );
        assert_eq!(closed.state, swavan_protocol::SessionState::Closed);
        assert_eq!(control_plane.active_sessions(&auth), Ok(Vec::new()));
    }

    #[test]
    fn control_plane_rejects_unauthorized_session_requests() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );

        assert_eq!(
            control_plane.create_session(
                &ControlAuth::new("wrong-token"),
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: swavan_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Unauthorized)
        );
    }

    #[test]
    fn foreground_server_handles_health_request() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("health correct-token", &mut events),
            "OK health service=swavan-server version=test healthy=true"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "health".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_rejects_bad_token() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("health wrong-token", &mut events),
            "ERROR unauthorized"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestRejected {
                operation: "health".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_rejects_unknown_operation() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("unknown correct-token", &mut events),
            "ERROR unknown-operation"
        );
        assert_eq!(events.events(), &[]);
    }

    #[test]
    fn daemon_service_installer_builds_linux_user_systemd_plan() {
        let installer = DaemonServiceInstaller::new("/usr/bin/swavan-server");
        let plan = installer
            .plan_for_platform(Platform::Linux)
            .expect("linux service plan");

        assert_eq!(plan.platform, Platform::Linux);
        assert!(plan
            .manifest_path
            .ends_with(".config/systemd/user/swavan-app-relay.service"));
        assert!(plan
            .manifest_contents
            .contains("ExecStart=/usr/bin/swavan-server --config"));
        assert!(plan
            .manifest_contents
            .contains("Restart=on-failure\nRestartSec=3"));
        assert_eq!(
            plan.start_command,
            "systemctl --user start swavan-app-relay.service"
        );
    }

    #[test]
    fn daemon_service_installer_builds_macos_launch_agent_plan() {
        let installer = DaemonServiceInstaller::new("/Applications/Swavan AppRelay.app/server");
        let plan = installer
            .plan_for_platform(Platform::Macos)
            .expect("macos service plan");

        assert_eq!(plan.platform, Platform::Macos);
        assert!(plan
            .manifest_path
            .ends_with("Library/LaunchAgents/com.swavan.apprelay.server.plist"));
        assert!(plan
            .manifest_contents
            .contains("<string>com.swavan.apprelay.server</string>"));
        assert!(plan.manifest_contents.contains("<key>KeepAlive</key>"));
        assert!(plan.start_command.starts_with("launchctl bootstrap"));
    }

    #[test]
    fn daemon_service_installer_builds_windows_service_script_plan() {
        let installer = DaemonServiceInstaller::new("C:\\Program Files\\Swavan\\server.exe");
        let plan = installer
            .plan_for_platform(Platform::Windows)
            .expect("windows service plan");

        assert_eq!(plan.platform, Platform::Windows);
        assert!(plan.manifest_path.ends_with("install-service.ps1"));
        assert!(plan
            .manifest_contents
            .contains("$serviceName = 'SwavanAppRelay'"));
        assert!(plan
            .manifest_contents
            .contains("sc.exe create $serviceName"));
        assert_eq!(plan.status_command, "sc.exe query SwavanAppRelay");
    }

    #[test]
    fn daemon_service_installer_rejects_client_platforms() {
        let installer = DaemonServiceInstaller::new("/usr/bin/swavan-server");

        assert_eq!(
            installer.plan_for_platform(Platform::Ios),
            Err(ServiceInstallError::UnsupportedPlatform(Platform::Ios))
        );
    }
}
