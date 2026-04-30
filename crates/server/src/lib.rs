//! Server composition for AppRelay.

mod audio_stream;
mod video_stream;

use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use apprelay_core::{
    ApplicationDiscovery, ApplicationLaunchBackendService, ApplicationSessionService,
    CapabilityService, DefaultCapabilityService, DesktopEntryApplicationDiscovery, EventSink,
    HealthService, InMemoryApplicationSessionService, InMemoryInputForwardingService,
    InputForwardingService, MacosApplicationDiscovery, ServerConfig, ServerEvent, SessionPolicy,
    StaticHealthService, UnsupportedApplicationDiscovery,
};
use apprelay_protocol::{
    AppRelayError, ApplicationLaunch, ApplicationSession, ApplicationSummary, AudioStreamSession,
    ControlAuth, ControlError, ControlResult, CreateSessionRequest, Feature, ForwardInputRequest,
    HealthStatus, HeartbeatStatus, InputDelivery, LaunchIntentStatus, NegotiateVideoStreamRequest,
    Platform, PlatformCapability, ReconnectVideoStreamRequest, ResizeSessionRequest, ServerVersion,
    StartAudioStreamRequest, StartVideoStreamRequest, StopAudioStreamRequest,
    StopVideoStreamRequest, UpdateAudioStreamRequest, VideoStreamSession, ViewportSize,
};

use crate::audio_stream::AudioStreamControl;
use crate::video_stream::VideoStreamControl;

#[derive(Debug)]
pub struct ServerServices {
    health_service: StaticHealthService,
    capability_service: DefaultCapabilityService,
    application_discovery: ApplicationDiscoveryService,
    session_service: InMemoryApplicationSessionService,
    input_forwarding: InMemoryInputForwardingService,
    video_stream: VideoStreamControl,
    audio_stream: AudioStreamControl,
    platform: Platform,
    version: String,
}

impl ServerServices {
    pub fn new(platform: Platform, version: impl Into<String>) -> Self {
        let version = version.into();

        Self {
            health_service: StaticHealthService::new("apprelay-server", version.clone()),
            capability_service: DefaultCapabilityService::new(platform),
            application_discovery: ApplicationDiscoveryService::for_platform(platform),
            session_service: InMemoryApplicationSessionService::with_launch_backend(
                SessionPolicy::allow_all(),
                launch_backend_for_platform(platform),
            ),
            input_forwarding: InMemoryInputForwardingService::default(),
            video_stream: VideoStreamControl::for_platform(platform),
            audio_stream: AudioStreamControl::for_platform(platform),
            platform,
            version,
        }
    }

    pub fn for_current_platform() -> Self {
        Self::new(Platform::current(), env!("CARGO_PKG_VERSION"))
    }

    #[doc(hidden)]
    pub fn with_linux_desktop_entry_roots(version: impl Into<String>, roots: Vec<PathBuf>) -> Self {
        let mut services = Self::new(Platform::Linux, version);
        services.application_discovery = ApplicationDiscoveryService::DesktopEntries(
            DesktopEntryApplicationDiscovery::new(roots),
        );
        services
    }

    #[doc(hidden)]
    pub fn with_macos_application_roots_and_open_command(
        version: impl Into<String>,
        roots: Vec<PathBuf>,
        open_command: PathBuf,
    ) -> Self {
        let mut services = Self::new(Platform::Macos, version);
        services.application_discovery =
            ApplicationDiscoveryService::MacosApplications(MacosApplicationDiscovery::new(roots));
        services.session_service = InMemoryApplicationSessionService::with_launch_backend(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::MacosNative { open_command },
        );
        services
    }

    pub fn health(&self) -> HealthStatus {
        self.health_service.status()
    }

    pub fn capabilities(&self) -> Vec<PlatformCapability> {
        self.capability_service.platform_capabilities()
    }

    pub fn available_applications(&self) -> Result<Vec<ApplicationSummary>, AppRelayError> {
        self.application_discovery.available_applications()
    }

    pub fn create_session(
        &mut self,
        request: CreateSessionRequest,
    ) -> Result<ApplicationSession, AppRelayError> {
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
    ) -> Result<ApplicationSession, AppRelayError> {
        let session = self.session_service.resize_session(request.clone())?;
        self.video_stream.record_resize(&request);
        Ok(session)
    }

    pub fn close_session(&mut self, session_id: &str) -> Result<ApplicationSession, AppRelayError> {
        let session = self.session_service.close_session(session_id)?;
        self.input_forwarding.close_session(session_id);
        self.video_stream.record_session_closed(session_id);
        self.audio_stream.record_session_closed(session_id);
        Ok(session)
    }

    pub fn active_sessions(&self) -> Vec<ApplicationSession> {
        self.session_service.active_sessions()
    }

    pub fn forward_input(
        &mut self,
        request: ForwardInputRequest,
    ) -> Result<InputDelivery, AppRelayError> {
        self.input_forwarding
            .forward_input(request, &self.session_service.active_sessions())
    }

    pub fn start_video_stream(
        &mut self,
        request: StartVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.video_stream
            .start(request, &self.session_service.active_sessions())
    }

    pub fn stop_video_stream(
        &mut self,
        request: StopVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.video_stream.stop(request)
    }

    pub fn reconnect_video_stream(
        &mut self,
        request: ReconnectVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.video_stream.reconnect(request)
    }

    pub fn negotiate_video_stream(
        &mut self,
        request: NegotiateVideoStreamRequest,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.video_stream.negotiate(request)
    }

    pub fn video_stream_status(
        &self,
        stream_id: &str,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.video_stream.status(stream_id)
    }

    pub fn start_audio_stream(
        &mut self,
        request: StartAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.audio_stream
            .start(request, &self.session_service.active_sessions())
    }

    pub fn stop_audio_stream(
        &mut self,
        request: StopAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.audio_stream.stop(request)
    }

    pub fn update_audio_stream(
        &mut self,
        request: UpdateAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.audio_stream.update(request)
    }

    pub fn audio_stream_status(
        &self,
        stream_id: &str,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.audio_stream.status(stream_id)
    }

    pub fn version(&self) -> ServerVersion {
        ServerVersion::new("apprelay-server", self.version.clone(), self.platform)
    }
}

fn launch_backend_for_platform(platform: Platform) -> ApplicationLaunchBackendService {
    match platform {
        Platform::Linux => ApplicationLaunchBackendService::LinuxNative,
        Platform::Macos => ApplicationLaunchBackendService::MacosNative {
            open_command: PathBuf::from("/usr/bin/open"),
        },
        Platform::Windows | Platform::Android | Platform::Ios | Platform::Unknown => {
            ApplicationLaunchBackendService::RecordOnly
        }
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
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, AppRelayError> {
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

    pub fn forward_input(
        &mut self,
        auth: &ControlAuth,
        request: ForwardInputRequest,
    ) -> ControlResult<InputDelivery> {
        self.authorize(auth)?;
        self.services.forward_input(request).map_err(Into::into)
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

    pub fn reconnect_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: ReconnectVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize(auth)?;
        self.services
            .reconnect_video_stream(request)
            .map_err(Into::into)
    }

    pub fn negotiate_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: NegotiateVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize(auth)?;
        self.services
            .negotiate_video_stream(request)
            .map_err(Into::into)
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

    pub fn start_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: StartAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize(auth)?;
        self.services
            .start_audio_stream(request)
            .map_err(Into::into)
    }

    pub fn stop_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: StopAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize(auth)?;
        self.services.stop_audio_stream(request).map_err(Into::into)
    }

    pub fn update_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: UpdateAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize(auth)?;
        self.services
            .update_audio_stream(request)
            .map_err(Into::into)
    }

    pub fn audio_stream_status(
        &self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize(auth)?;
        self.services
            .audio_stream_status(stream_id)
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
    control_plane: RefCell<ServerControlPlane>,
}

impl ForegroundControlServer {
    pub fn new(control_plane: ServerControlPlane) -> Self {
        Self {
            control_plane: RefCell::new(control_plane),
        }
    }

    pub fn bind_address(&self) -> String {
        let control_plane = self.control_plane.borrow();
        format!(
            "{}:{}",
            control_plane.config().bind_address,
            control_plane.config().control_port
        )
    }

    pub fn run_once(
        &self,
        listener: TcpListener,
        events: &mut impl EventSink,
    ) -> std::io::Result<()> {
        events.record(ServerEvent::ControlPlaneStarted {
            bind_address: self.control_plane.borrow().config().bind_address.clone(),
            port: self.control_plane.borrow().config().control_port,
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

        let mut args = token.split_whitespace();
        let Some(token) = args.next() else {
            return "ERROR bad-request".to_string();
        };
        let auth = ControlAuth::new(token);
        let result = match operation {
            "health" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane.borrow().health(&auth).map(|health| {
                    format!(
                        "OK health service={} version={} healthy={}",
                        health.service, health.version, health.healthy
                    )
                })
            }
            "version" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane.borrow().version(&auth).map(|version| {
                    format!(
                        "OK version service={} version={} platform={:?}",
                        version.service, version.version, version.platform
                    )
                })
            }
            "heartbeat" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .heartbeat(&auth)
                    .map(|heartbeat| {
                        format!(
                            "OK heartbeat healthy={} sequence={}",
                            heartbeat.healthy, heartbeat.sequence
                        )
                    })
            }
            "capabilities" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .capabilities(&auth)
                    .map(format_capabilities_response)
            }
            "applications" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .available_applications(&auth)
                    .map(format_applications_response)
            }
            "create-session" => {
                let Some(application_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                let Some(width) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
                    return "ERROR bad-request".to_string();
                };
                let Some(height) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
                    return "ERROR bad-request".to_string();
                };
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow_mut()
                    .create_session(
                        &auth,
                        CreateSessionRequest {
                            application_id: application_id.to_string(),
                            viewport: ViewportSize::new(width, height),
                        },
                    )
                    .map(format_create_session_response)
            }
            "sessions" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .active_sessions(&auth)
                    .map(format_sessions_response)
            }
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
            Err(ControlError::Service(error)) => format!("ERROR service {}", error.user_message()),
        }
    }
}

fn format_capabilities_response(mut capabilities: Vec<PlatformCapability>) -> String {
    capabilities.sort_by_key(|capability| feature_key(&capability.feature));
    let supported = capabilities
        .iter()
        .filter(|capability| capability.supported)
        .count();
    let mut response = format!(
        "OK capabilities supported={supported} total={}",
        capabilities.len()
    );

    for capability in capabilities {
        response.push(' ');
        response.push_str(feature_key(&capability.feature));
        response.push(':');
        response.push_str(if capability.supported {
            "supported"
        } else {
            "unsupported"
        });
    }

    response
}

fn format_applications_response(mut applications: Vec<ApplicationSummary>) -> String {
    applications.sort_by(|left, right| left.id.cmp(&right.id));
    let mut response = format!("OK applications count={}", applications.len());

    for (index, application) in applications.into_iter().enumerate() {
        response.push_str(&format!(
            " app{index}.id={} app{index}.name={} app{index}.launch={}",
            line_token(&application.id),
            line_token(&application.name),
            launch_kind(application.launch.as_ref())
        ));
    }

    response
}

fn format_create_session_response(session: ApplicationSession) -> String {
    format!(
        "OK create-session id={} app={} window_id={} window_title={} selection={} launch={} viewport={}x{}",
        line_token(&session.id),
        line_token(&session.application_id),
        line_token(&session.selected_window.id),
        line_token(&session.selected_window.title),
        selection_method(&session.selected_window.selection_method),
        launch_status(session.launch_intent.as_ref().map(|intent| &intent.status)),
        session.viewport.width,
        session.viewport.height
    )
}

fn format_sessions_response(mut sessions: Vec<ApplicationSession>) -> String {
    sessions.sort_by(|left, right| left.id.cmp(&right.id));
    let mut response = format!("OK sessions count={}", sessions.len());

    for (index, session) in sessions.into_iter().enumerate() {
        response.push_str(&format!(
            " session{index}.id={} session{index}.app={} session{index}.state={:?} session{index}.viewport={}x{}",
            line_token(&session.id),
            line_token(&session.application_id),
            session.state,
            session.viewport.width,
            session.viewport.height
        ));
    }

    response
}

fn feature_key(feature: &Feature) -> &'static str {
    match feature {
        Feature::AppDiscovery => "app-discovery",
        Feature::ApplicationLaunch => "application-launch",
        Feature::WindowResize => "window-resize",
        Feature::WindowVideoStream => "window-video-stream",
        Feature::SystemAudioStream => "system-audio-stream",
        Feature::ClientMicrophoneInput => "client-microphone-input",
        Feature::KeyboardInput => "keyboard-input",
        Feature::MouseInput => "mouse-input",
    }
}

fn launch_kind(launch: Option<&ApplicationLaunch>) -> &'static str {
    match launch {
        Some(ApplicationLaunch::DesktopCommand { .. }) => "desktop-command",
        Some(ApplicationLaunch::MacosBundle { .. }) => "macos-bundle",
        None => "none",
    }
}

fn launch_status(status: Option<&LaunchIntentStatus>) -> &'static str {
    match status {
        Some(LaunchIntentStatus::Recorded) => "recorded",
        Some(LaunchIntentStatus::Attached) => "attached",
        Some(LaunchIntentStatus::Unsupported) => "unsupported",
        None => "none",
    }
}

fn selection_method(method: &apprelay_protocol::WindowSelectionMethod) -> &'static str {
    match method {
        apprelay_protocol::WindowSelectionMethod::LaunchIntent => "launch-intent",
        apprelay_protocol::WindowSelectionMethod::ExistingWindow => "existing-window",
        apprelay_protocol::WindowSelectionMethod::Synthetic => "synthetic",
    }
}

fn line_token(value: &str) -> String {
    let mut encoded = String::new();

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
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
        let config_path = xdg_config_home(&home).join("apprelay/server.conf");
        let log_path = xdg_state_home(&home).join("apprelay/server.log");
        let manifest_path = home.join(".config/systemd/user/apprelay.service");
        let executable_path = display_path(&self.executable_path);
        let config_arg = display_path(&config_path);
        let log_arg = display_path(&log_path);
        let manifest_contents = format!(
            "[Unit]\n\
Description=AppRelay server\n\
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
            start_command: "systemctl --user start apprelay.service".to_string(),
            stop_command: "systemctl --user stop apprelay.service".to_string(),
            status_command: "systemctl --user status apprelay.service".to_string(),
            uninstall_command:
                "systemctl --user disable --now apprelay.service && rm ~/.config/systemd/user/apprelay.service && systemctl --user daemon-reload"
                    .to_string(),
        })
    }

    fn macos_launch_agent_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        let config_path = home.join("Library/Application Support/AppRelay/server.conf");
        let log_path = home.join("Library/Logs/AppRelay/server.log");
        let manifest_path = home.join("Library/LaunchAgents/dev.apprelay.server.plist");
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
  <string>dev.apprelay.server</string>\n\
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
            stop_command: "launchctl bootout gui/$UID/dev.apprelay.server".to_string(),
            status_command: "launchctl print gui/$UID/dev.apprelay.server".to_string(),
            uninstall_command: format!(
                "launchctl bootout gui/$UID/dev.apprelay.server; rm {manifest_arg}"
            ),
        })
    }

    fn windows_service_script_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        let service_root = program_data.join("AppRelay");
        let config_path = service_root.join("server.conf");
        let log_path = service_root.join("server.log");
        let manifest_path = service_root.join("install-service.ps1");
        let executable_path = display_path(&self.executable_path);
        let config_arg = display_path(&config_path);
        let log_arg = display_path(&log_path);
        let manifest_contents = format!(
            "$ErrorActionPreference = 'Stop'\n\
$serviceName = 'AppRelay'\n\
$binaryPath = '\"{executable_path}\" --config \"{config_arg}\" --log \"{log_arg}\"'\n\
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {{\n\
  sc.exe stop $serviceName | Out-Null\n\
  sc.exe delete $serviceName | Out-Null\n\
}}\n\
sc.exe create $serviceName binPath= $binaryPath start= auto DisplayName= 'AppRelay Server'\n\
sc.exe start $serviceName\n"
        );

        Ok(ServiceInstallPlan {
            platform: Platform::Windows,
            manifest_path,
            config_path,
            log_path,
            manifest_contents,
            start_command: "sc.exe start AppRelay".to_string(),
            stop_command: "sc.exe stop AppRelay".to_string(),
            status_command: "sc.exe query AppRelay".to_string(),
            uninstall_command: "sc.exe stop AppRelay && sc.exe delete AppRelay".to_string(),
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
    use apprelay_core::InMemoryEventSink;

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

        std::fs::write(path, contents).expect("write executable script");
        let mut permissions = std::fs::metadata(path)
            .expect("read executable script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark executable script");
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

    #[test]
    fn server_services_report_health() {
        let services = ServerServices::new(Platform::Linux, "test");

        assert_eq!(
            services.health(),
            HealthStatus::healthy("apprelay-server", "test")
        );
    }

    #[test]
    fn server_services_report_version() {
        let services = ServerServices::new(Platform::Linux, "test");

        assert_eq!(
            services.version(),
            ServerVersion::new("apprelay-server", "test", Platform::Linux)
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
            Err(AppRelayError::UnsupportedPlatform { .. })
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
            Ok(HealthStatus::healthy("apprelay-server", "test"))
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
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
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
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");

        let resized = control_plane
            .resize_session(
                &auth,
                ResizeSessionRequest {
                    session_id: session.id.clone(),
                    viewport: apprelay_protocol::ViewportSize::new(1440, 900),
                },
            )
            .expect("resize session");
        let closed = control_plane
            .close_session(&auth, &session.id)
            .expect("close session");

        assert_eq!(
            resized.viewport,
            apprelay_protocol::ViewportSize::new(1440, 900)
        );
        assert_eq!(closed.state, apprelay_protocol::SessionState::Closed);
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
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
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
            "OK health service=apprelay-server version=test healthy=true"
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
    fn foreground_server_reports_linux_application_launch_capability() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        let response = server.handle_request("capabilities correct-token", &mut events);

        assert!(response.starts_with("OK capabilities supported="));
        assert!(response.contains(" total=8 "));
        assert!(response.contains("application-launch:supported"));
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "capabilities".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_reports_macos_application_launch_capability() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Macos, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        let response = server.handle_request("capabilities correct-token", &mut events);

        assert!(response.starts_with("OK capabilities supported="));
        assert!(response.contains(" total=8 "));
        assert!(response.contains("application-launch:supported"));
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "capabilities".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_lists_applications_as_parseable_tokens() {
        let root = unique_test_dir("foreground-applications");
        let applications = root.join("applications");
        std::fs::create_dir_all(&applications).expect("create desktop entry root");
        std::fs::write(
            applications.join("fake.desktop"),
            "[Desktop Entry]\nType=Application\nName=Fake App\nExec=/bin/true %U\n",
        )
        .expect("write desktop entry");
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::with_linux_desktop_entry_roots("test", vec![applications]),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        let response = server.handle_request("applications correct-token", &mut events);

        assert_eq!(
            response,
            "OK applications count=1 app0.id=fake app0.name=Fake%20App app0.launch=desktop-command"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "applications".to_string(),
            }]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn foreground_server_create_session_launches_linux_desktop_entry() {
        let root = unique_test_dir("foreground-create-session");
        let applications = root.join("applications");
        std::fs::create_dir_all(&applications).expect("create desktop entry root");
        let marker = root.join("launch-marker");
        let executable = root.join("fake-app");
        write_executable_script(
            &executable,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" > {}\n",
                marker.display()
            ),
        );
        std::fs::write(
            applications.join("fake.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Fake App\nExec={} --label \"Fake App\" %U\n",
                executable.display()
            ),
        )
        .expect("write desktop entry");
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::with_linux_desktop_entry_roots("test", vec![applications]),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        let response =
            server.handle_request("create-session correct-token fake 1280 720", &mut events);

        wait_for_path(&marker);
        assert_eq!(
            std::fs::read_to_string(&marker).expect("read launch marker"),
            "--label\nFake App\n"
        );
        assert_eq!(
            response,
            "OK create-session id=session-1 app=fake window_id=window-session-1 window_title=Fake%20App selection=launch-intent launch=recorded viewport=1280x720"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "create-session".to_string(),
            }]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foreground_server_rejects_bad_create_session_args() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request(
                "create-session correct-token terminal wide 720",
                &mut events
            ),
            "ERROR bad-request"
        );
        assert_eq!(events.events(), &[]);
    }

    #[test]
    fn foreground_server_rejects_unauthorized_create_session() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("create-session wrong-token terminal 1280 720", &mut events),
            "ERROR unauthorized"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestRejected {
                operation: "create-session".to_string(),
            }]
        );
    }

    #[test]
    fn daemon_service_installer_builds_linux_user_systemd_plan() {
        let installer = DaemonServiceInstaller::new("/usr/bin/apprelay-server");
        let plan = installer
            .plan_for_platform(Platform::Linux)
            .expect("linux service plan");

        assert_eq!(plan.platform, Platform::Linux);
        assert!(plan
            .manifest_path
            .ends_with(".config/systemd/user/apprelay.service"));
        assert!(plan
            .manifest_contents
            .contains("ExecStart=/usr/bin/apprelay-server --config"));
        assert!(plan
            .manifest_contents
            .contains("Restart=on-failure\nRestartSec=3"));
        assert_eq!(
            plan.start_command,
            "systemctl --user start apprelay.service"
        );
    }

    #[test]
    fn daemon_service_installer_builds_macos_launch_agent_plan() {
        let installer = DaemonServiceInstaller::new("/Applications/AppRelay.app/server");
        let plan = installer
            .plan_for_platform(Platform::Macos)
            .expect("macos service plan");

        assert_eq!(plan.platform, Platform::Macos);
        assert!(plan
            .manifest_path
            .ends_with("Library/LaunchAgents/dev.apprelay.server.plist"));
        assert!(plan
            .manifest_contents
            .contains("<string>dev.apprelay.server</string>"));
        assert!(plan.manifest_contents.contains("<key>KeepAlive</key>"));
        assert!(plan.start_command.starts_with("launchctl bootstrap"));
    }

    #[test]
    fn daemon_service_installer_builds_windows_service_script_plan() {
        let installer = DaemonServiceInstaller::new("C:\\Program Files\\AppRelay\\server.exe");
        let plan = installer
            .plan_for_platform(Platform::Windows)
            .expect("windows service plan");

        assert_eq!(plan.platform, Platform::Windows);
        assert!(plan.manifest_path.ends_with("install-service.ps1"));
        assert!(plan.manifest_contents.contains("$serviceName = 'AppRelay'"));
        assert!(plan
            .manifest_contents
            .contains("sc.exe create $serviceName"));
        assert_eq!(plan.status_command, "sc.exe query AppRelay");
    }

    #[test]
    fn daemon_service_installer_rejects_client_platforms() {
        let installer = DaemonServiceInstaller::new("/usr/bin/apprelay-server");

        assert_eq!(
            installer.plan_for_platform(Platform::Ios),
            Err(ServiceInstallError::UnsupportedPlatform(Platform::Ios))
        );
    }
}
