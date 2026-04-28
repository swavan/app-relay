//! Server composition for Swavan AppRelay.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};

use swavan_core::{
    ApplicationDiscovery, CapabilityService, DefaultCapabilityService,
    DesktopEntryApplicationDiscovery, EventSink, HealthService, MacosApplicationDiscovery,
    ServerConfig, ServerEvent, StaticHealthService, UnsupportedApplicationDiscovery,
};
use swavan_protocol::{
    ApplicationSummary, ControlAuth, ControlError, ControlResult, HealthStatus, HeartbeatStatus,
    Platform, PlatformCapability, ServerVersion, SwavanError,
};

#[derive(Debug)]
pub struct ServerServices {
    health_service: StaticHealthService,
    capability_service: DefaultCapabilityService,
    application_discovery: ApplicationDiscoveryService,
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
}
