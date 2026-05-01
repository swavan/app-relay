//! Server composition for AppRelay.

mod audio_stream;
mod video_stream;

use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use apprelay_core::{
    ApplicationDiscovery, ApplicationLaunchBackendService, ApplicationSessionService,
    ApplicationWindowSelectionBackendService, CapabilityService, ClientAuthorizationService,
    DefaultCapabilityService, DesktopEntryApplicationDiscovery, EventSink, HealthService,
    InMemoryApplicationSessionService, InMemoryClientAuthorizationService,
    InMemoryInputForwardingService, InputForwardingService, MacosApplicationDiscovery,
    MacosWindowCaptureRuntime, ServerConfig, ServerEvent, SessionPolicy, StaticHealthService,
    UnsupportedApplicationDiscovery,
};
use apprelay_protocol::{
    AppRelayError, ApplicationLaunch, ApplicationSession, ApplicationSummary,
    ApprovePairingRequest, AudioStreamSession, ControlAuth, ControlClientIdentity, ControlError,
    ControlResult, CreateSessionRequest, DiagnosticsBundle, Feature, ForwardInputRequest,
    HealthStatus, HeartbeatStatus, InputDelivery, LaunchIntentStatus, NegotiateVideoStreamRequest,
    PairingRequest, PendingPairing, Platform, PlatformCapability, ReconnectVideoStreamRequest,
    ResizeSessionRequest, ServerVersion, StartAudioStreamRequest, StartVideoStreamRequest,
    StopAudioStreamRequest, StopVideoStreamRequest, UpdateAudioStreamRequest, VideoStreamSession,
    ViewportSize,
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
            session_service:
                InMemoryApplicationSessionService::with_launch_and_window_selection_backends(
                    SessionPolicy::allow_all(),
                    launch_backend_for_platform(platform),
                    window_selection_backend_for_platform(platform),
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
        services.session_service =
            InMemoryApplicationSessionService::with_launch_and_window_selection_backends(
                SessionPolicy::allow_all(),
                ApplicationLaunchBackendService::MacosNative { open_command },
                ApplicationWindowSelectionBackendService::RecordOnly,
            );
        services
    }

    #[doc(hidden)]
    pub fn with_macos_application_roots_open_and_osascript_commands(
        version: impl Into<String>,
        roots: Vec<PathBuf>,
        open_command: PathBuf,
        osascript_command: PathBuf,
    ) -> Self {
        let mut services = Self::with_macos_application_roots_and_open_command(
            version,
            roots,
            open_command.clone(),
        );
        services.session_service =
            InMemoryApplicationSessionService::with_launch_and_window_selection_backends(
                SessionPolicy::allow_all(),
                ApplicationLaunchBackendService::MacosNative { open_command },
                ApplicationWindowSelectionBackendService::MacosNative { osascript_command },
            );
        services
    }

    #[doc(hidden)]
    pub fn with_macos_application_roots_open_osascript_and_capture_runtime(
        version: impl Into<String>,
        roots: Vec<PathBuf>,
        open_command: PathBuf,
        osascript_command: PathBuf,
        capture_runtime: Arc<dyn MacosWindowCaptureRuntime>,
    ) -> Self {
        let mut services = Self::with_macos_application_roots_open_and_osascript_commands(
            version,
            roots,
            open_command,
            osascript_command,
        );
        services.video_stream = VideoStreamControl::for_macos_runtime(capture_runtime);
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

    pub fn active_video_streams(&self) -> Vec<VideoStreamSession> {
        let active_session_ids = self
            .session_service
            .active_sessions()
            .into_iter()
            .map(|session| session.id)
            .collect::<std::collections::HashSet<_>>();

        self.video_stream
            .active_streams()
            .into_iter()
            .filter(|stream| active_session_ids.contains(&stream.session_id))
            .collect()
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

impl ServerServices {
    fn diagnostics(&self, config: &ServerConfig) -> DiagnosticsBundle {
        let capabilities = self.capabilities();

        DiagnosticsBundle {
            format_version: 1,
            telemetry_enabled: false,
            secrets_redacted: true,
            service: "apprelay-server".to_string(),
            version: self.version.clone(),
            platform: self.platform,
            bind_address: config.bind_address.clone(),
            control_port: config.control_port,
            heartbeat_interval_millis: config.heartbeat_interval_millis,
            supported_capabilities: capabilities
                .iter()
                .filter(|capability| capability.supported)
                .count(),
            total_capabilities: capabilities.len(),
            active_sessions: self.active_sessions().len(),
        }
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

fn window_selection_backend_for_platform(
    platform: Platform,
) -> ApplicationWindowSelectionBackendService {
    match platform {
        Platform::Macos => ApplicationWindowSelectionBackendService::MacosNative {
            osascript_command: PathBuf::from("/usr/bin/osascript"),
        },
        Platform::Linux
        | Platform::Windows
        | Platform::Android
        | Platform::Ios
        | Platform::Unknown => ApplicationWindowSelectionBackendService::RecordOnly,
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
    client_authorization: InMemoryClientAuthorizationService,
    config: ServerConfig,
    heartbeat_sequence: AtomicU64,
}

impl ServerControlPlane {
    pub fn new(services: ServerServices, config: ServerConfig) -> Self {
        let client_authorization =
            InMemoryClientAuthorizationService::new(config.authorized_clients.clone());
        Self {
            services,
            client_authorization,
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

    pub fn diagnostics(&self, auth: &ControlAuth) -> ControlResult<DiagnosticsBundle> {
        self.authorize(auth)?;
        Ok(self.services.diagnostics(&self.config))
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
        self.authorize_paired_client(auth)?;
        self.services.create_session(request).map_err(Into::into)
    }

    pub fn request_pairing(
        &mut self,
        auth: &ControlAuth,
        client: ControlClientIdentity,
    ) -> ControlResult<PendingPairing> {
        self.authorize(auth)?;
        self.client_authorization
            .request_pairing(PairingRequest { client })
            .map_err(Into::into)
    }

    pub fn locally_approve_pairing(
        &mut self,
        request: ApprovePairingRequest,
    ) -> ControlResult<apprelay_core::AuthorizedClient> {
        let client = self
            .client_authorization
            .approve_pairing(request)
            .map_err(ControlError::Service)?;
        self.config.authorized_clients = self.client_authorization.authorized_clients();
        Ok(client)
    }

    pub fn resize_session(
        &mut self,
        auth: &ControlAuth,
        request: ResizeSessionRequest,
    ) -> ControlResult<ApplicationSession> {
        self.authorize_paired_client(auth)?;
        self.services.resize_session(request).map_err(Into::into)
    }

    pub fn close_session(
        &mut self,
        auth: &ControlAuth,
        session_id: &str,
    ) -> ControlResult<ApplicationSession> {
        self.authorize_paired_client(auth)?;
        self.services.close_session(session_id).map_err(Into::into)
    }

    pub fn active_sessions(&self, auth: &ControlAuth) -> ControlResult<Vec<ApplicationSession>> {
        self.authorize_paired_client(auth)?;
        Ok(self.services.active_sessions())
    }

    pub fn forward_input(
        &mut self,
        auth: &ControlAuth,
        request: ForwardInputRequest,
    ) -> ControlResult<InputDelivery> {
        self.authorize_paired_client(auth)?;
        self.services.forward_input(request).map_err(Into::into)
    }

    pub fn start_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: StartVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services
            .start_video_stream(request)
            .map_err(Into::into)
    }

    pub fn stop_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: StopVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services.stop_video_stream(request).map_err(Into::into)
    }

    pub fn reconnect_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: ReconnectVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services
            .reconnect_video_stream(request)
            .map_err(Into::into)
    }

    pub fn negotiate_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: NegotiateVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services
            .negotiate_video_stream(request)
            .map_err(Into::into)
    }

    pub fn video_stream_status(
        &self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services
            .video_stream_status(stream_id)
            .map_err(Into::into)
    }

    pub fn active_video_streams(
        &self,
        auth: &ControlAuth,
    ) -> ControlResult<Vec<VideoStreamSession>> {
        self.authorize_paired_client(auth)?;
        Ok(self.services.active_video_streams())
    }

    pub fn start_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: StartAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services
            .start_audio_stream(request)
            .map_err(Into::into)
    }

    pub fn stop_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: StopAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services.stop_audio_stream(request).map_err(Into::into)
    }

    pub fn update_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: UpdateAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize_paired_client(auth)?;
        self.services
            .update_audio_stream(request)
            .map_err(Into::into)
    }

    pub fn audio_stream_status(
        &self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize_paired_client(auth)?;
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

    fn authorize_paired_client(&self, auth: &ControlAuth) -> Result<(), ControlError> {
        self.authorize(auth)?;
        self.client_authorization
            .authorize_client(auth.client_id())
            .map(|_| ())
            .map_err(ControlError::Service)
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
        let peer_address = stream
            .peer_addr()
            .map(|address| address.to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        events.record(ServerEvent::ForegroundConnectionAccepted {
            peer_address: peer_address.clone(),
        });

        let result = (|| {
            let mut request = String::new();
            BufReader::new(stream.try_clone()?).read_line(&mut request)?;
            let response = self.handle_request(request.trim(), events);

            stream.write_all(response.as_bytes())?;
            stream.write_all(b"\n")?;
            Ok(())
        })();

        events.record(ServerEvent::ForegroundConnectionClosed { peer_address });
        result
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
                    response_only(format!(
                        "OK health service={} version={} healthy={}",
                        health.service, health.version, health.healthy
                    ))
                })
            }
            "version" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane.borrow().version(&auth).map(|version| {
                    response_only(format!(
                        "OK version service={} version={} platform={:?}",
                        version.service, version.version, version.platform
                    ))
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
                        response_only(format!(
                            "OK heartbeat healthy={} sequence={}",
                            heartbeat.healthy, heartbeat.sequence
                        ))
                    })
            }
            "capabilities" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .capabilities(&auth)
                    .map(|capabilities| response_only(format_capabilities_response(capabilities)))
            }
            "diagnostics" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .diagnostics(&auth)
                    .map(|diagnostics| response_only(format_diagnostics_response(diagnostics)))
            }
            "applications" => {
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .available_applications(&auth)
                    .map(|applications| response_only(format_applications_response(applications)))
            }
            "pairing-request" => {
                let Some(client_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                let Some(label) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow_mut()
                    .request_pairing(
                        &auth,
                        ControlClientIdentity {
                            id: client_id.to_string(),
                            label: label.replace("%20", " "),
                        },
                    )
                    .map(|pending| response_only(format_pairing_request_response(pending)))
            }
            "create-session" => {
                let Some(client_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
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

                let client_id = client_id.to_string();
                self.control_plane
                    .borrow_mut()
                    .create_session(
                        &ControlAuth::with_client_id(auth.token(), &client_id),
                        CreateSessionRequest {
                            application_id: application_id.to_string(),
                            viewport: ViewportSize::new(width, height),
                        },
                    )
                    .map(|session| {
                        let event = session_created_event(&client_id, &session);
                        (format_create_session_response(session), vec![event])
                    })
            }
            "resize-session" => {
                let Some(client_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                let Some(session_id) = args.next() else {
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

                let client_id = client_id.to_string();
                self.control_plane
                    .borrow_mut()
                    .resize_session(
                        &ControlAuth::with_client_id(auth.token(), &client_id),
                        ResizeSessionRequest {
                            session_id: session_id.to_string(),
                            viewport: ViewportSize::new(width, height),
                        },
                    )
                    .map(|session| {
                        let event = session_resized_event(&client_id, &session);
                        (format_resize_session_response(session), vec![event])
                    })
            }
            "close-session" => {
                let Some(client_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                let Some(session_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                let client_id = client_id.to_string();
                self.control_plane
                    .borrow_mut()
                    .close_session(
                        &ControlAuth::with_client_id(auth.token(), &client_id),
                        session_id,
                    )
                    .map(|session| {
                        let event = session_closed_event(&client_id, &session);
                        (format_close_session_response(session), vec![event])
                    })
            }
            "sessions" => {
                let Some(client_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                self.control_plane
                    .borrow()
                    .active_sessions(&ControlAuth::with_client_id(auth.token(), client_id))
                    .map(|sessions| response_only(format_sessions_response(sessions)))
            }
            _ => return "ERROR unknown-operation".to_string(),
        };

        match result {
            Ok((response, audit_events)) => {
                events.record(ServerEvent::RequestAuthorized {
                    operation: operation.to_string(),
                });
                for event in audit_events {
                    events.record(event);
                }
                response
            }
            Err(ControlError::Unauthorized) => {
                events.record(ServerEvent::RequestRejected {
                    operation: operation.to_string(),
                });
                "ERROR unauthorized".to_string()
            }
            Err(ControlError::Service(error)) => {
                if matches!(&error, AppRelayError::PermissionDenied(_)) {
                    events.record(ServerEvent::RequestRejected {
                        operation: operation.to_string(),
                    });
                }
                format!("ERROR service {}", error.user_message())
            }
        }
    }
}

fn response_only(response: String) -> (String, Vec<ServerEvent>) {
    (response, Vec::new())
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

fn format_diagnostics_response(bundle: DiagnosticsBundle) -> String {
    format!(
        "OK diagnostics format={} telemetry={} secrets={} service={} version={} platform={:?} bind={} port={} heartbeat_ms={} capabilities={}/{} sessions={}",
        bundle.format_version,
        bundle.telemetry_enabled,
        if bundle.secrets_redacted { "redacted" } else { "included" },
        line_token(&bundle.service),
        line_token(&bundle.version),
        bundle.platform,
        line_token(&bundle.bind_address),
        bundle.control_port,
        bundle.heartbeat_interval_millis,
        bundle.supported_capabilities,
        bundle.total_capabilities,
        bundle.active_sessions
    )
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

fn format_pairing_request_response(pending: PendingPairing) -> String {
    format!(
        "OK pairing-request id={} client_id={} label={} status={:?}",
        line_token(&pending.request_id),
        line_token(&pending.client.id),
        line_token(&pending.client.label),
        pending.status
    )
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

fn format_resize_session_response(session: ApplicationSession) -> String {
    format!(
        "OK resize-session id={} app={} viewport={}x{}",
        line_token(&session.id),
        line_token(&session.application_id),
        session.viewport.width,
        session.viewport.height
    )
}

fn format_close_session_response(session: ApplicationSession) -> String {
    format!(
        "OK close-session id={} app={} state={:?}",
        line_token(&session.id),
        line_token(&session.application_id),
        session.state
    )
}

fn session_created_event(client_id: &str, session: &ApplicationSession) -> ServerEvent {
    ServerEvent::SessionCreated {
        session_id: session.id.clone(),
        application_id: session.application_id.clone(),
        client_id: client_id.to_string(),
        viewport_width: session.viewport.width,
        viewport_height: session.viewport.height,
    }
}

fn session_resized_event(client_id: &str, session: &ApplicationSession) -> ServerEvent {
    ServerEvent::SessionResized {
        session_id: session.id.clone(),
        application_id: session.application_id.clone(),
        client_id: client_id.to_string(),
        viewport_width: session.viewport.width,
        viewport_height: session.viewport.height,
    }
}

fn session_closed_event(client_id: &str, session: &ApplicationSession) -> ServerEvent {
    ServerEvent::SessionClosed {
        session_id: session.id.clone(),
        application_id: session.application_id.clone(),
        client_id: client_id.to_string(),
    }
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
        apprelay_protocol::WindowSelectionMethod::NativeWindow => "native-window",
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
    pub crash_recovery: ServiceCrashRecoveryPolicy,
    pub manifest_contents: String,
    pub start_command: String,
    pub stop_command: String,
    pub status_command: String,
    pub uninstall_command: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCrashRecoveryPolicy {
    pub service_manager: &'static str,
    pub restart_condition: &'static str,
    pub restart_delay_seconds: u64,
    pub crash_loop_window_seconds: Option<u64>,
    pub crash_loop_max_restarts: Option<u64>,
    pub manifest_contract: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceUninstallPlan {
    pub platform: Platform,
    pub manifest_path: PathBuf,
    pub service_manifest_path: PathBuf,
    pub manifest_contents: String,
    pub run_command: String,
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

    pub fn uninstall_plan_for_current_platform(
        &self,
    ) -> Result<ServiceUninstallPlan, ServiceInstallError> {
        self.uninstall_plan_for_platform(Platform::current())
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

    pub fn uninstall_plan_for_platform(
        &self,
        platform: Platform,
    ) -> Result<ServiceUninstallPlan, ServiceInstallError> {
        match platform {
            Platform::Linux => self.linux_user_systemd_uninstall_plan(),
            Platform::Macos => self.macos_launch_agent_uninstall_plan(),
            Platform::Windows => self.windows_service_uninstall_script_plan(),
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

    pub fn write_uninstall_manifest(
        &self,
        plan: &ServiceUninstallPlan,
    ) -> Result<(), ServiceInstallError> {
        if let Some(parent) = plan.manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&plan.manifest_path, &plan.manifest_contents)?;
        Ok(())
    }

    fn linux_user_systemd_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        Ok(self.linux_user_systemd_plan_for_paths(
            &home,
            &xdg_config_home(&home),
            &xdg_state_home(&home),
        ))
    }

    #[cfg(test)]
    fn linux_user_systemd_plan_for_home(&self, home: &Path) -> ServiceInstallPlan {
        self.linux_user_systemd_plan_for_paths(
            home,
            &home.join(".config"),
            &home.join(".local/state"),
        )
    }

    fn linux_user_systemd_plan_for_paths(
        &self,
        home: &Path,
        config_home: &Path,
        state_home: &Path,
    ) -> ServiceInstallPlan {
        let config_path = config_home.join("apprelay/server.conf");
        let log_path = state_home.join("apprelay/server.log");
        let manifest_path = home.join(".config/systemd/user/apprelay.service");
        let executable_path = display_path(&self.executable_path);
        let config_arg = display_path(&config_path);
        let log_arg = display_path(&log_path);
        let manifest_contents = format!(
            "[Unit]\n\
Description=AppRelay server\n\
StartLimitIntervalSec=60\n\
StartLimitBurst=5\n\
\n\
[Service]\n\
ExecStart={executable_path} --config {config_arg} --log {log_arg}\n\
Restart=on-failure\n\
RestartSec=3\n\
\n\
[Install]\n\
WantedBy=default.target\n"
        );

        ServiceInstallPlan {
            platform: Platform::Linux,
            manifest_path,
            config_path,
            log_path,
            crash_recovery: LINUX_SYSTEMD_CRASH_RECOVERY,
            manifest_contents,
            start_command: "systemctl --user start apprelay.service".to_string(),
            stop_command: "systemctl --user stop apprelay.service".to_string(),
            status_command: "systemctl --user status apprelay.service".to_string(),
            uninstall_command:
                "systemctl --user disable --now apprelay.service && rm ~/.config/systemd/user/apprelay.service && systemctl --user daemon-reload"
                    .to_string(),
        }
    }

    fn linux_user_systemd_uninstall_plan(
        &self,
    ) -> Result<ServiceUninstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        Ok(self.linux_user_systemd_uninstall_plan_for_paths(&home, &xdg_config_home(&home)))
    }

    #[cfg(test)]
    fn linux_user_systemd_uninstall_plan_for_home(&self, home: &Path) -> ServiceUninstallPlan {
        self.linux_user_systemd_uninstall_plan_for_paths(home, &home.join(".config"))
    }

    fn linux_user_systemd_uninstall_plan_for_paths(
        &self,
        home: &Path,
        config_home: &Path,
    ) -> ServiceUninstallPlan {
        let manifest_path = config_home.join("apprelay/uninstall-service.sh");
        let service_manifest_path = home.join(".config/systemd/user/apprelay.service");
        let service_manifest_arg = shell_quote(&display_path(&service_manifest_path));
        let manifest_arg = shell_quote(&display_path(&manifest_path));
        let manifest_contents = format!(
            "#!/bin/sh\n\
set -eu\n\
systemctl --user disable --now apprelay.service || true\n\
rm -f {service_manifest_arg}\n\
systemctl --user daemon-reload\n"
        );

        ServiceUninstallPlan {
            platform: Platform::Linux,
            manifest_path,
            service_manifest_path,
            manifest_contents,
            run_command: format!("sh {manifest_arg}"),
        }
    }

    fn macos_launch_agent_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        Ok(self.macos_launch_agent_plan_for_home(&home))
    }

    fn macos_launch_agent_plan_for_home(&self, home: &Path) -> ServiceInstallPlan {
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
  <dict>\n\
    <key>SuccessfulExit</key>\n\
    <false/>\n\
  </dict>\n\
  <key>ThrottleInterval</key>\n\
  <integer>3</integer>\n\
  <key>RunAtLoad</key>\n\
  <true/>\n\
</dict>\n\
</plist>\n"
        );

        ServiceInstallPlan {
            platform: Platform::Macos,
            manifest_path,
            config_path,
            log_path,
            crash_recovery: MACOS_LAUNCHD_CRASH_RECOVERY,
            manifest_contents,
            start_command: format!("launchctl bootstrap gui/$UID {manifest_arg}"),
            stop_command: "launchctl bootout gui/$UID/dev.apprelay.server".to_string(),
            status_command: "launchctl print gui/$UID/dev.apprelay.server".to_string(),
            uninstall_command: format!(
                "launchctl bootout gui/$UID/dev.apprelay.server; rm {manifest_arg}"
            ),
        }
    }

    fn macos_launch_agent_uninstall_plan(
        &self,
    ) -> Result<ServiceUninstallPlan, ServiceInstallError> {
        let home = home_dir()?;
        Ok(self.macos_launch_agent_uninstall_plan_for_home(&home))
    }

    fn macos_launch_agent_uninstall_plan_for_home(&self, home: &Path) -> ServiceUninstallPlan {
        let service_manifest_path = home.join("Library/LaunchAgents/dev.apprelay.server.plist");
        let manifest_path = home.join("Library/Application Support/AppRelay/uninstall-service.sh");
        let service_manifest_arg = shell_quote(&display_path(&service_manifest_path));
        let manifest_arg = shell_quote(&display_path(&manifest_path));
        let manifest_contents = format!(
            "#!/bin/sh\n\
set -eu\n\
launchctl bootout gui/$UID/dev.apprelay.server || true\n\
rm -f {service_manifest_arg}\n"
        );

        ServiceUninstallPlan {
            platform: Platform::Macos,
            manifest_path,
            service_manifest_path,
            manifest_contents,
            run_command: format!("sh {manifest_arg}"),
        }
    }

    fn windows_service_script_plan(&self) -> Result<ServiceInstallPlan, ServiceInstallError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        Ok(self.windows_service_script_plan_for_program_data(&program_data))
    }

    fn windows_service_script_plan_for_program_data(
        &self,
        program_data: &Path,
    ) -> ServiceInstallPlan {
        let config_path = windows_path(program_data, &["AppRelay", "server.conf"]);
        let log_path = windows_path(program_data, &["AppRelay", "server.log"]);
        let manifest_path = windows_path(program_data, &["AppRelay", "install-service.ps1"]);
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
sc.exe failure $serviceName reset= 60 actions= restart/3000/restart/3000/restart/3000\n\
sc.exe failureflag $serviceName 1\n\
sc.exe start $serviceName\n"
        );

        ServiceInstallPlan {
            platform: Platform::Windows,
            manifest_path,
            config_path,
            log_path,
            crash_recovery: WINDOWS_SERVICE_CRASH_RECOVERY,
            manifest_contents,
            start_command: "sc.exe start AppRelay".to_string(),
            stop_command: "sc.exe stop AppRelay".to_string(),
            status_command: "sc.exe query AppRelay".to_string(),
            uninstall_command: "sc.exe stop AppRelay && sc.exe delete AppRelay".to_string(),
        }
    }

    fn windows_service_uninstall_script_plan(
        &self,
    ) -> Result<ServiceUninstallPlan, ServiceInstallError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        Ok(self.windows_service_uninstall_script_plan_for_program_data(&program_data))
    }

    fn windows_service_uninstall_script_plan_for_program_data(
        &self,
        program_data: &Path,
    ) -> ServiceUninstallPlan {
        let manifest_path = windows_path(program_data, &["AppRelay", "uninstall-service.ps1"]);
        let service_manifest_path =
            windows_path(program_data, &["AppRelay", "install-service.ps1"]);
        let manifest_arg = powershell_quote(&display_path(&manifest_path));
        let service_manifest_arg = powershell_quote(&display_path(&service_manifest_path));
        let manifest_contents = "$ErrorActionPreference = 'Stop'\n\
$serviceName = 'AppRelay'\n\
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue\n\
if ($service) {\n\
  if ($service.Status -ne 'Stopped') {\n\
    sc.exe stop $serviceName | Out-Null\n\
  }\n\
  sc.exe delete $serviceName | Out-Null\n\
}\n"
        .to_string()
            + &format!(
                "Remove-Item -LiteralPath {service_manifest_arg} -Force -ErrorAction SilentlyContinue\n"
            );

        ServiceUninstallPlan {
            platform: Platform::Windows,
            manifest_path,
            service_manifest_path,
            manifest_contents,
            run_command: format!("powershell.exe -ExecutionPolicy Bypass -File {manifest_arg}"),
        }
    }
}

const LINUX_SYSTEMD_CRASH_RECOVERY: ServiceCrashRecoveryPolicy = ServiceCrashRecoveryPolicy {
    service_manager: "systemd-user",
    restart_condition: "on-failure",
    restart_delay_seconds: 3,
    crash_loop_window_seconds: Some(60),
    crash_loop_max_restarts: Some(5),
    manifest_contract:
        "Restart=on-failure, RestartSec=3, StartLimitIntervalSec=60, StartLimitBurst=5",
};

const MACOS_LAUNCHD_CRASH_RECOVERY: ServiceCrashRecoveryPolicy = ServiceCrashRecoveryPolicy {
    service_manager: "launchd-agent",
    restart_condition: "non-zero exit",
    restart_delay_seconds: 3,
    crash_loop_window_seconds: None,
    crash_loop_max_restarts: None,
    manifest_contract: "KeepAlive SuccessfulExit=false, ThrottleInterval=3",
};

const WINDOWS_SERVICE_CRASH_RECOVERY: ServiceCrashRecoveryPolicy = ServiceCrashRecoveryPolicy {
    service_manager: "windows-service-control-manager",
    restart_condition: "service failure",
    restart_delay_seconds: 3,
    crash_loop_window_seconds: Some(60),
    crash_loop_max_restarts: Some(3),
    manifest_contract:
        "sc.exe failure reset=60 actions=restart/3000 repeated three times, failureflag=1",
};

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

fn windows_path(root: &Path, children: &[&str]) -> PathBuf {
    let mut path = display_path(root).replace('/', "\\");
    path = path.trim_end_matches('\\').to_string();

    for child in children {
        path.push('\\');
        path.push_str(child.trim_matches('\\'));
    }

    PathBuf::from(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
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

    fn paired_server_config() -> ServerConfig {
        let mut config = ServerConfig::local("correct-token");
        config.authorized_clients = vec![apprelay_core::AuthorizedClient::new(
            "test-client",
            "Test Client",
        )];
        config
    }

    fn paired_auth() -> ControlAuth {
        ControlAuth::with_client_id("correct-token", "test-client")
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
    fn server_services_build_telemetry_free_diagnostics() {
        let services = ServerServices::new(Platform::Linux, "test");
        let mut config = ServerConfig::local("secret-token");
        config.bind_address = "127.0.0.1".to_string();
        config.control_port = 9898;
        config.heartbeat_interval_millis = 2_500;

        let diagnostics = services.diagnostics(&config);

        assert_eq!(
            diagnostics,
            DiagnosticsBundle {
                format_version: 1,
                telemetry_enabled: false,
                secrets_redacted: true,
                service: "apprelay-server".to_string(),
                version: "test".to_string(),
                platform: Platform::Linux,
                bind_address: "127.0.0.1".to_string(),
                control_port: 9898,
                heartbeat_interval_millis: 2_500,
                supported_capabilities: 5,
                total_capabilities: 8,
                active_sessions: 0,
            }
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
            paired_server_config(),
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
            paired_server_config(),
        );

        assert_eq!(
            control_plane.health(&ControlAuth::new("correct-token")),
            Ok(HealthStatus::healthy("apprelay-server", "test"))
        );
    }

    #[test]
    fn control_plane_returns_authorized_diagnostics_without_token() {
        let control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        );

        let diagnostics = control_plane
            .diagnostics(&ControlAuth::new("correct-token"))
            .expect("authorized diagnostics response");

        assert!(!diagnostics.telemetry_enabled);
        assert!(diagnostics.secrets_redacted);
        assert_eq!(diagnostics.service, "apprelay-server");
        assert_eq!(diagnostics.control_port, 7676);
    }

    #[test]
    fn control_plane_heartbeat_increments_sequence() {
        let control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        );
        let auth = paired_auth();

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
            paired_server_config(),
        );
        let auth = paired_auth();

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
            paired_server_config(),
        );
        let auth = paired_auth();
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
            paired_server_config(),
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
    fn control_plane_rejects_create_session_for_unknown_client_identity() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );

        assert_eq!(
            control_plane.create_session(
                &ControlAuth::new("correct-token"),
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client identity is required".to_string()
            )))
        );
        assert_eq!(
            control_plane.create_session(
                &ControlAuth::with_client_id("correct-token", "unknown-client"),
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client unknown-client is not paired".to_string()
            )))
        );
    }

    #[test]
    fn control_plane_requires_explicit_pairing_approval_before_session_creation() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );
        let auth = ControlAuth::with_client_id("correct-token", "client-1");
        let pending = control_plane
            .request_pairing(
                &auth,
                ControlClientIdentity {
                    id: "client-1".to_string(),
                    label: "Laptop".to_string(),
                },
            )
            .expect("request pairing");

        assert_eq!(
            control_plane.create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client client-1 is not paired".to_string()
            )))
        );

        let approved = control_plane
            .locally_approve_pairing(ApprovePairingRequest {
                request_id: pending.request_id,
            })
            .expect("approve pairing");
        assert_eq!(approved.id, "client-1");

        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create session after approval");
        assert_eq!(session.application_id, "terminal");
    }

    #[test]
    fn control_plane_requires_paired_client_for_sensitive_session_controls() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        );
        let auth = paired_auth();
        let unpaired_auth = ControlAuth::new("correct-token");
        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");
        let expected = Err(ControlError::Service(AppRelayError::PermissionDenied(
            "client identity is required".to_string(),
        )));

        assert_eq!(control_plane.active_sessions(&unpaired_auth), expected);
        assert_eq!(
            control_plane.resize_session(
                &unpaired_auth,
                ResizeSessionRequest {
                    session_id: session.id.clone(),
                    viewport: apprelay_protocol::ViewportSize::new(1440, 900),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client identity is required".to_string()
            )))
        );
        assert_eq!(
            control_plane.forward_input(
                &unpaired_auth,
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                    event: apprelay_protocol::InputEvent::Focus,
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client identity is required".to_string()
            )))
        );
        assert_eq!(
            control_plane.start_video_stream(
                &unpaired_auth,
                StartVideoStreamRequest {
                    session_id: session.id,
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client identity is required".to_string()
            )))
        );
    }

    #[test]
    fn foreground_server_handles_health_request() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
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
            paired_server_config(),
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
    fn foreground_server_records_connection_events_without_tokens() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind foreground listener");
        let address = listener.local_addr().expect("listener address");
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(address).expect("connect foreground listener");
            stream
                .write_all(b"health correct-token\n")
                .expect("write request");
            let mut response = String::new();
            BufReader::new(stream)
                .read_line(&mut response)
                .expect("read response");
            response
        });
        let (mut stream, _) = listener.accept().expect("accept foreground connection");
        let mut events = InMemoryEventSink::default();

        server
            .handle_stream(&mut stream, &mut events)
            .expect("handle foreground stream");

        assert_eq!(
            client.join().expect("join foreground client"),
            "OK health service=apprelay-server version=test healthy=true\n"
        );
        assert_eq!(events.events().len(), 3);
        assert!(matches!(
            &events.events()[0],
            ServerEvent::ForegroundConnectionAccepted { peer_address }
                if peer_address.starts_with("127.0.0.1:")
        ));
        assert_eq!(
            events.events()[1],
            ServerEvent::RequestAuthorized {
                operation: "health".to_string(),
            }
        );
        assert!(matches!(
            &events.events()[2],
            ServerEvent::ForegroundConnectionClosed { peer_address }
                if peer_address.starts_with("127.0.0.1:")
        ));
        assert!(!format!("{:?}", events.events()).contains("correct-token"));
    }

    #[test]
    fn foreground_server_rejects_unknown_operation() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
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
            paired_server_config(),
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
    fn foreground_server_returns_diagnostics_bundle() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        let response = server.handle_request("diagnostics correct-token", &mut events);

        assert_eq!(
            response,
            "OK diagnostics format=1 telemetry=false secrets=redacted service=apprelay-server version=test platform=Linux bind=127.0.0.1 port=7676 heartbeat_ms=5000 capabilities=5/8 sessions=0"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "diagnostics".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_reports_macos_application_launch_capability() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Macos, "test"),
            paired_server_config(),
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
            paired_server_config(),
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
    fn foreground_server_records_pairing_request() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-request correct-token client-1 Laptop", &mut events),
            "OK pairing-request id=pairing-1 client_id=client-1 label=Laptop status=PendingUserApproval"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestAuthorized {
                operation: "pairing-request".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_rejects_bad_pairing_request_args() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-request correct-token client-1", &mut events),
            "ERROR bad-request"
        );
        assert_eq!(events.events(), &[]);
    }

    #[test]
    fn foreground_server_rejects_unauthorized_pairing_request() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-request wrong-token client-1 Laptop", &mut events),
            "ERROR unauthorized"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestRejected {
                operation: "pairing-request".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_does_not_allow_pairing_self_approval() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-request correct-token client-1 Laptop", &mut events),
            "OK pairing-request id=pairing-1 client_id=client-1 label=Laptop status=PendingUserApproval"
        );
        assert_eq!(
            server.handle_request("pairing-approve correct-token pairing-1", &mut events),
            "ERROR unknown-operation"
        );
        assert_eq!(
            server.handle_request(
                "create-session correct-token client-1 terminal 1280 720",
                &mut events,
            ),
            "ERROR service client client-1 is not paired"
        );

        let approved = server
            .control_plane
            .borrow_mut()
            .locally_approve_pairing(ApprovePairingRequest {
                request_id: "pairing-1".to_string(),
            })
            .expect("local approve pairing");
        assert_eq!(approved.id, "client-1");

        assert_eq!(
            server.handle_request(
                "create-session correct-token client-1 terminal 1280 720",
                &mut events,
            ),
            "OK create-session id=session-1 app=terminal window_id=window-session-1 window_title=terminal selection=existing-window launch=attached viewport=1280x720"
        );
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
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        let response = server.handle_request(
            "create-session correct-token test-client fake 1280 720",
            &mut events,
        );

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
            &[
                ServerEvent::RequestAuthorized {
                    operation: "create-session".to_string(),
                },
                ServerEvent::SessionCreated {
                    session_id: "session-1".to_string(),
                    application_id: "fake".to_string(),
                    client_id: "test-client".to_string(),
                    viewport_width: 1280,
                    viewport_height: 720,
                },
            ]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foreground_server_records_session_lifecycle_events() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request(
                "create-session correct-token test-client terminal 1280 720",
                &mut events,
            ),
            "OK create-session id=session-1 app=terminal window_id=window-session-1 window_title=terminal selection=existing-window launch=attached viewport=1280x720"
        );
        assert_eq!(
            server.handle_request(
                "resize-session correct-token test-client session-1 1440 900",
                &mut events,
            ),
            "OK resize-session id=session-1 app=terminal viewport=1440x900"
        );
        assert_eq!(
            server.handle_request(
                "close-session correct-token test-client session-1",
                &mut events,
            ),
            "OK close-session id=session-1 app=terminal state=Closed"
        );

        assert_eq!(
            events.events(),
            &[
                ServerEvent::RequestAuthorized {
                    operation: "create-session".to_string(),
                },
                ServerEvent::SessionCreated {
                    session_id: "session-1".to_string(),
                    application_id: "terminal".to_string(),
                    client_id: "test-client".to_string(),
                    viewport_width: 1280,
                    viewport_height: 720,
                },
                ServerEvent::RequestAuthorized {
                    operation: "resize-session".to_string(),
                },
                ServerEvent::SessionResized {
                    session_id: "session-1".to_string(),
                    application_id: "terminal".to_string(),
                    client_id: "test-client".to_string(),
                    viewport_width: 1440,
                    viewport_height: 900,
                },
                ServerEvent::RequestAuthorized {
                    operation: "close-session".to_string(),
                },
                ServerEvent::SessionClosed {
                    session_id: "session-1".to_string(),
                    application_id: "terminal".to_string(),
                    client_id: "test-client".to_string(),
                },
            ]
        );
    }

    #[test]
    fn foreground_server_rejects_bad_create_session_args() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request(
                "create-session correct-token test-client terminal wide 720",
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
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request(
                "create-session wrong-token test-client terminal 1280 720",
                &mut events,
            ),
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
    fn foreground_server_records_paired_client_denial_as_rejected_request() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request(
                "create-session correct-token unknown-client terminal 1280 720",
                &mut events,
            ),
            "ERROR service client unknown-client is not paired"
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
        let home = PathBuf::from("/home/apprelay");
        let plan = installer.linux_user_systemd_plan_for_home(&home);

        assert_eq!(plan.platform, Platform::Linux);
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("/home/apprelay/.config/systemd/user/apprelay.service")
        );
        assert_eq!(
            plan.config_path,
            PathBuf::from("/home/apprelay/.config/apprelay/server.conf")
        );
        assert_eq!(
            plan.log_path,
            PathBuf::from("/home/apprelay/.local/state/apprelay/server.log")
        );
        assert!(plan
            .manifest_contents
            .contains("ExecStart=/usr/bin/apprelay-server --config /home/apprelay/.config/apprelay/server.conf --log /home/apprelay/.local/state/apprelay/server.log"));
        assert!(plan
            .manifest_contents
            .contains("Restart=on-failure\nRestartSec=3"));
        assert!(plan
            .manifest_contents
            .contains("StartLimitIntervalSec=60\nStartLimitBurst=5"));
        assert_eq!(plan.crash_recovery, LINUX_SYSTEMD_CRASH_RECOVERY);
        assert_eq!(
            plan.start_command,
            "systemctl --user start apprelay.service"
        );
    }

    #[test]
    fn daemon_service_installer_builds_macos_launch_agent_plan() {
        let installer = DaemonServiceInstaller::new("/Applications/AppRelay.app/server");
        let home = PathBuf::from("/Users/apprelay");
        let plan = installer.macos_launch_agent_plan_for_home(&home);

        assert_eq!(plan.platform, Platform::Macos);
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("/Users/apprelay/Library/LaunchAgents/dev.apprelay.server.plist")
        );
        assert_eq!(
            plan.config_path,
            PathBuf::from("/Users/apprelay/Library/Application Support/AppRelay/server.conf")
        );
        assert_eq!(
            plan.log_path,
            PathBuf::from("/Users/apprelay/Library/Logs/AppRelay/server.log")
        );
        assert!(plan
            .manifest_contents
            .contains("<string>dev.apprelay.server</string>"));
        assert!(plan.manifest_contents.contains(
            r#"<key>KeepAlive</key>
<dict>
<key>SuccessfulExit</key>
<false/>
</dict>
<key>ThrottleInterval</key>
<integer>3</integer>"#
        ));
        assert_eq!(plan.crash_recovery, MACOS_LAUNCHD_CRASH_RECOVERY);
        assert!(plan.start_command.starts_with("launchctl bootstrap"));
    }

    #[test]
    fn daemon_service_installer_builds_windows_service_script_plan() {
        let installer = DaemonServiceInstaller::new("C:\\Program Files\\AppRelay\\server.exe");
        let program_data = PathBuf::from("C:\\ProgramData");
        let plan = installer.windows_service_script_plan_for_program_data(&program_data);

        assert_eq!(plan.platform, Platform::Windows);
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("C:\\ProgramData\\AppRelay\\install-service.ps1")
        );
        assert_eq!(
            plan.config_path,
            PathBuf::from("C:\\ProgramData\\AppRelay\\server.conf")
        );
        assert_eq!(
            plan.log_path,
            PathBuf::from("C:\\ProgramData\\AppRelay\\server.log")
        );
        assert!(plan.manifest_contents.contains("$serviceName = 'AppRelay'"));
        assert!(plan
            .manifest_contents
            .contains("sc.exe create $serviceName"));
        assert!(plan.manifest_contents.contains(
            "sc.exe failure $serviceName reset= 60 actions= restart/3000/restart/3000/restart/3000"
        ));
        assert!(plan
            .manifest_contents
            .contains("sc.exe failureflag $serviceName 1"));
        assert_eq!(plan.crash_recovery, WINDOWS_SERVICE_CRASH_RECOVERY);
        assert_eq!(plan.status_command, "sc.exe query AppRelay");
    }

    #[test]
    fn daemon_service_installer_builds_linux_uninstall_plan() {
        let installer = DaemonServiceInstaller::new("/usr/bin/apprelay-server");
        let home = PathBuf::from("/home/app relay");
        let plan = installer.linux_user_systemd_uninstall_plan_for_home(&home);

        assert_eq!(plan.platform, Platform::Linux);
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("/home/app relay/.config/apprelay/uninstall-service.sh")
        );
        assert_eq!(
            plan.service_manifest_path,
            PathBuf::from("/home/app relay/.config/systemd/user/apprelay.service")
        );
        assert_eq!(
            plan.manifest_contents,
            "#!/bin/sh\nset -eu\nsystemctl --user disable --now apprelay.service || true\nrm -f '/home/app relay/.config/systemd/user/apprelay.service'\nsystemctl --user daemon-reload\n"
        );
        assert_eq!(
            plan.run_command,
            "sh '/home/app relay/.config/apprelay/uninstall-service.sh'"
        );
    }

    #[test]
    fn daemon_service_installer_builds_macos_uninstall_plan() {
        let installer = DaemonServiceInstaller::new("/Applications/AppRelay.app/server");
        let home = PathBuf::from("/Users/app relay");
        let plan = installer.macos_launch_agent_uninstall_plan_for_home(&home);

        assert_eq!(plan.platform, Platform::Macos);
        assert_eq!(
            plan.manifest_path,
            PathBuf::from(
                "/Users/app relay/Library/Application Support/AppRelay/uninstall-service.sh"
            )
        );
        assert_eq!(
            plan.service_manifest_path,
            PathBuf::from("/Users/app relay/Library/LaunchAgents/dev.apprelay.server.plist")
        );
        assert_eq!(
            plan.manifest_contents,
            "#!/bin/sh\nset -eu\nlaunchctl bootout gui/$UID/dev.apprelay.server || true\nrm -f '/Users/app relay/Library/LaunchAgents/dev.apprelay.server.plist'\n"
        );
        assert_eq!(
            plan.run_command,
            "sh '/Users/app relay/Library/Application Support/AppRelay/uninstall-service.sh'"
        );
    }

    #[test]
    fn daemon_service_installer_builds_windows_uninstall_plan() {
        let installer = DaemonServiceInstaller::new("C:\\Program Files\\AppRelay\\server.exe");
        let program_data = PathBuf::from("C:\\ProgramData");
        let plan = installer.windows_service_uninstall_script_plan_for_program_data(&program_data);

        assert_eq!(plan.platform, Platform::Windows);
        assert_eq!(
            plan.manifest_path,
            PathBuf::from("C:\\ProgramData\\AppRelay\\uninstall-service.ps1")
        );
        assert_eq!(
            plan.service_manifest_path,
            PathBuf::from("C:\\ProgramData\\AppRelay\\install-service.ps1")
        );
        assert!(plan
            .manifest_contents
            .contains("$service = Get-Service -Name $serviceName"));
        assert!(plan
            .manifest_contents
            .contains("sc.exe delete $serviceName | Out-Null"));
        assert!(plan
            .manifest_contents
            .contains("Remove-Item -LiteralPath 'C:\\ProgramData\\AppRelay\\install-service.ps1'"));
        assert_eq!(
            plan.run_command,
            "powershell.exe -ExecutionPolicy Bypass -File 'C:\\ProgramData\\AppRelay\\uninstall-service.ps1'"
        );
    }

    #[test]
    fn daemon_service_installer_writes_uninstall_manifest() {
        let root = unique_test_dir("apprelay-uninstall-manifest");
        let manifest_path = root.join("service/uninstall-service.sh");
        let service_manifest_path = root.join("service/apprelay.service");
        let plan = ServiceUninstallPlan {
            platform: Platform::Linux,
            manifest_path: manifest_path.clone(),
            service_manifest_path,
            manifest_contents: "#!/bin/sh\nexit 0\n".to_string(),
            run_command: format!("sh {}", manifest_path.display()),
        };
        let installer = DaemonServiceInstaller::new("/usr/bin/apprelay-server");

        installer
            .write_uninstall_manifest(&plan)
            .expect("write uninstall manifest");

        assert_eq!(
            std::fs::read_to_string(&manifest_path).expect("read uninstall manifest"),
            "#!/bin/sh\nexit 0\n"
        );
        std::fs::remove_dir_all(root).expect("remove uninstall manifest test dir");
    }

    #[test]
    fn daemon_service_installer_rejects_client_platforms() {
        let installer = DaemonServiceInstaller::new("/usr/bin/apprelay-server");

        assert_eq!(
            installer.plan_for_platform(Platform::Ios),
            Err(ServiceInstallError::UnsupportedPlatform(Platform::Ios))
        );
        assert_eq!(
            installer.uninstall_plan_for_platform(Platform::Android),
            Err(ServiceInstallError::UnsupportedPlatform(Platform::Android))
        );
    }
}
