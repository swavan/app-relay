//! Server composition for AppRelay.

mod audio_stream;
mod signaling;
mod video_stream;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;

use apprelay_core::{
    ApplicationDiscovery, ApplicationLaunchBackendService, ApplicationSessionService,
    ApplicationWindowSelectionBackendService, CapabilityService, ClientAuthorizationService,
    DefaultCapabilityService, DesktopEntryApplicationDiscovery, EventSink, HealthService,
    InMemoryApplicationSessionService, InMemoryClientAuthorizationService,
    InMemoryInputForwardingService, InputBackendService, InputForwardingService,
    MacosApplicationDiscovery, MacosWindowCaptureRuntime, ServerConfig, ServerConfigRepository,
    ServerEvent, SessionPolicy, StaticHealthService, UnsupportedApplicationDiscovery,
    WindowResizeBackendService, SIGNALING_BACKLOG_FULL_MESSAGE_PREFIX,
};
use apprelay_protocol::{
    ActiveInputFocus, AppRelayError, ApplicationLaunch, ApplicationSession, ApplicationSummary,
    ApprovePairingRequest, AudioStreamSession, ControlAuth, ControlClientIdentity, ControlError,
    ControlResult, CreateSessionRequest, DiagnosticsBundle, Feature, ForwardInputRequest,
    HealthStatus, HeartbeatStatus, IceCandidatePayload, InputDelivery, InputDeliveryStatus,
    InputEvent, LaunchIntentStatus, NegotiateVideoStreamRequest, PairingRequest, PendingPairing,
    Platform, PlatformCapability, PollSignalingRequest, ReconnectVideoStreamRequest,
    ResizeSessionRequest, RevokeClientRequest, SdpRole, ServerVersion, SignalingDirection,
    SignalingEnvelope, SignalingPoll, SignalingSubmitAck, StartAudioStreamRequest,
    StartVideoStreamRequest, StopAudioStreamRequest, StopVideoStreamRequest,
    SubmitSignalingRequest, UpdateAudioStreamRequest, VideoStreamSession, ViewportSize,
    MAX_SIGNALING_PAYLOAD_BASE64_BYTES, MAX_SIGNALING_PAYLOAD_DECODED_BYTES,
};

use crate::audio_stream::AudioStreamControl;
use crate::signaling::SignalingControl;
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
    signaling: SignalingControl,
    /// Server-side WebRTC peer. Default: `InMemoryWebRtcPeer` no-op.
    /// With the `webrtc-peer` feature on, swapped for the
    /// `Str0mWebRtcPeer` scaffold (Phase D.0). Phase D.1.1 wires it
    /// into the start/stop/submit-signaling control-plane flows;
    /// Phase D.1.2 shares this `Arc<Mutex<...>>` with a background
    /// `WebRtcIoWorker` thread that drives the UDP transport pump.
    webrtc_peer: Arc<Mutex<Box<dyn apprelay_core::WebRtcPeer>>>,
    /// Phase D.1.2 background I/O worker. Holds the UDP socket and a
    /// `JoinHandle` for the pump thread that reads inbound datagrams
    /// into the peer and writes the peer's outbound RTP queue back
    /// out to the network. `None` in default builds (no `webrtc-peer`
    /// feature) and in test fixtures that construct `ServerServices`
    /// directly via `new`.
    #[cfg(feature = "webrtc-peer")]
    webrtc_io_worker: Option<WebRtcIoWorker>,
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
            session_service: InMemoryApplicationSessionService::with_backends(
                SessionPolicy::allow_all(),
                launch_backend_for_platform(platform),
                window_selection_backend_for_platform(platform),
                resize_backend_for_platform(platform),
            ),
            input_forwarding: InMemoryInputForwardingService::new(input_backend_for_platform(
                platform,
            )),
            video_stream: VideoStreamControl::for_platform(platform),
            audio_stream: AudioStreamControl::for_platform(platform),
            signaling: SignalingControl::new(),
            webrtc_peer: Arc::new(Mutex::new(Box::new(
                apprelay_core::InMemoryWebRtcPeer::new(),
            ))),
            #[cfg(feature = "webrtc-peer")]
            webrtc_io_worker: None,
            platform,
            version,
        }
    }

    pub fn for_current_platform() -> Self {
        #[cfg_attr(
            not(all(feature = "macos-screencapturekit", target_os = "macos")),
            allow(unused_mut)
        )]
        let mut services = Self::new(Platform::current(), env!("CARGO_PKG_VERSION"));
        // When both opt-in macOS features are enabled, prefer the
        // ScreenCaptureKit + VideoToolbox bridge so captured sample
        // buffers flow through the hardware H.264 encoder. When only
        // ScreenCaptureKit is enabled we install the capture-only
        // runtime that emits payload-free `EncodedVideoFrame`s, matching
        // the previous Phase A.1 behaviour.
        #[cfg(all(
            feature = "macos-screencapturekit",
            feature = "macos-videotoolbox",
            target_os = "macos"
        ))]
        {
            services.video_stream = VideoStreamControl::for_macos_runtime(Arc::new(
                apprelay_core::VideoToolboxScreenCaptureKitBridge::new(),
            ));
        }
        #[cfg(all(
            feature = "macos-screencapturekit",
            not(feature = "macos-videotoolbox"),
            target_os = "macos"
        ))]
        {
            services.video_stream = VideoStreamControl::for_macos_runtime(Arc::new(
                apprelay_core::ScreenCaptureKitWindowRuntime::new(),
            ));
        }
        // Phase D.1.0–D.1.1: when the `webrtc-peer` feature is enabled,
        // swap the in-memory no-op peer for the real `Str0mWebRtcPeer`.
        // No UDP transport is bound here — that path requires a
        // [`ServerConfig`] and is exposed via
        // [`Self::for_current_platform_with_config`].
        #[cfg(feature = "webrtc-peer")]
        {
            services.webrtc_peer =
                Arc::new(Mutex::new(Box::new(apprelay_core::Str0mWebRtcPeer::new())));
        }
        services
    }

    /// Like [`Self::for_current_platform`] but also binds the
    /// configured UDP transport and spawns the background I/O worker
    /// when the `webrtc-peer` feature is on. Default builds simply
    /// delegate to [`Self::for_current_platform`].
    ///
    /// Returns `Err(AppRelayError::ServiceUnavailable(...))` if the
    /// UDP bind fails — the user explicitly opted into real-media
    /// support, so a silent fallback to the no-op peer would violate
    /// the project-wide invariant that unsupported configurations
    /// surface a typed error.
    pub fn for_current_platform_with_config(config: &ServerConfig) -> Result<Self, AppRelayError> {
        #[cfg_attr(not(feature = "webrtc-peer"), allow(unused_mut))]
        let mut services = Self::for_current_platform();
        #[cfg(feature = "webrtc-peer")]
        {
            let bind_addr = config.webrtc_udp_bind_socket_addr().map_err(|err| {
                AppRelayError::ServiceUnavailable(format!(
                    "webrtc UDP bind address invalid: {err:?}"
                ))
            })?;
            let transport: Arc<dyn apprelay_core::WebRtcUdpTransport> = Arc::new(
                apprelay_core::StdUdpTransport::bind(bind_addr).map_err(|err| {
                    AppRelayError::ServiceUnavailable(format!(
                        "webrtc UDP bind {bind_addr} failed: {err}"
                    ))
                })?,
            );
            // Re-build the str0m peer so its host candidate matches the
            // freshly-bound socket address. Without this the peer would
            // advertise the loopback `127.0.0.1:0` placeholder set by
            // `Str0mWebRtcPeer::new`, and remote peers could not route
            // datagrams back to us.
            services.webrtc_peer = Arc::new(Mutex::new(Box::new(
                apprelay_core::Str0mWebRtcPeer::with_local_socket(transport.local_addr()),
            )));
            services.webrtc_io_worker = Some(WebRtcIoWorker::spawn(
                Arc::clone(&services.webrtc_peer),
                transport,
            ));
        }
        // Suppress unused-variable lint when the feature is off.
        #[cfg(not(feature = "webrtc-peer"))]
        {
            let _ = config;
        }
        Ok(services)
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
        services.session_service = InMemoryApplicationSessionService::with_backends(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::MacosNative { open_command },
            ApplicationWindowSelectionBackendService::RecordOnly,
            WindowResizeBackendService::RecordOnly,
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
        services.session_service = InMemoryApplicationSessionService::with_backends(
            SessionPolicy::allow_all(),
            ApplicationLaunchBackendService::MacosNative { open_command },
            ApplicationWindowSelectionBackendService::MacosNative {
                osascript_command: osascript_command.clone(),
            },
            WindowResizeBackendService::MacosNative {
                osascript_command: osascript_command.clone(),
            },
        );
        services.input_forwarding =
            InMemoryInputForwardingService::new(InputBackendService::MacosNativeInput {
                osascript_command,
            });
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

    #[doc(hidden)]
    pub fn with_macos_input_osascript_command(
        version: impl Into<String>,
        osascript_command: PathBuf,
    ) -> Self {
        let mut services = Self::new(Platform::Macos, version);
        services.input_forwarding =
            InMemoryInputForwardingService::new(InputBackendService::MacosNativeInput {
                osascript_command,
            });
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
        self.signaling.record_session_closed(session_id);
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

    pub fn active_input_focus(&self) -> Option<ActiveInputFocus> {
        self.input_forwarding
            .active_input_focus(&self.session_service.active_sessions())
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
        &mut self,
        stream_id: &str,
    ) -> Result<VideoStreamSession, AppRelayError> {
        self.video_stream.status(stream_id)
    }

    pub fn active_video_streams(&mut self) -> Vec<VideoStreamSession> {
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

    /// Test-only: advance the in-memory video encoder for `stream_id`
    /// so a fresh `EncodedVideoFrame` lands in `last_frame`. Used by
    /// the encoded-frame pump tests; not part of the production
    /// surface area.
    #[doc(hidden)]
    pub fn advance_encoded_frame_for_test(&mut self, stream_id: &str) -> Option<u64> {
        self.video_stream.advance_encoded_frame_for_test(stream_id)
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

    pub fn active_audio_streams(&self) -> Vec<AudioStreamSession> {
        let active_session_ids = self
            .session_service
            .active_sessions()
            .into_iter()
            .map(|session| session.id)
            .collect::<std::collections::HashSet<_>>();

        self.audio_stream
            .active_streams()
            .into_iter()
            .filter(|stream| active_session_ids.contains(&stream.session_id))
            .collect()
    }

    pub fn submit_signaling(
        &mut self,
        request: SubmitSignalingRequest,
    ) -> Result<SignalingSubmitAck, AppRelayError> {
        self.signaling.submit(request)
    }

    pub fn poll_signaling(
        &mut self,
        request: PollSignalingRequest,
    ) -> Result<SignalingPoll, AppRelayError> {
        self.signaling.poll(request)
    }

    /// Combined backlog depth (both directions) for `session_id`. Used by
    /// the foreground wire codec when emitting a
    /// `ServerEvent::SignalingBacklogFull` audit event.
    pub fn signaling_backlog_depth(&self, session_id: &str) -> usize {
        self.signaling.current_depth(session_id)
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

fn resize_backend_for_platform(platform: Platform) -> WindowResizeBackendService {
    match platform {
        Platform::Macos => WindowResizeBackendService::MacosNative {
            osascript_command: PathBuf::from("/usr/bin/osascript"),
        },
        Platform::Linux
        | Platform::Windows
        | Platform::Android
        | Platform::Ios
        | Platform::Unknown => WindowResizeBackendService::RecordOnly,
    }
}

fn input_backend_for_platform(platform: Platform) -> InputBackendService {
    match platform {
        Platform::Macos => InputBackendService::MacosNativeInput {
            osascript_command: PathBuf::from("/usr/bin/osascript"),
        },
        Platform::Linux
        | Platform::Windows
        | Platform::Android
        | Platform::Ios
        | Platform::Unknown => InputBackendService::RecordOnly,
    }
}

/// Phase D.1.2 background I/O worker. Drives the sans-IO
/// `Str0mWebRtcPeer` against a real UDP socket so DTLS/SRTP/ICE can
/// progress while the rest of the server keeps its synchronous,
/// `&mut self`-based control flow.
///
/// The worker owns a clone of the peer's `Arc<Mutex<...>>` and the
/// shared `Arc<dyn WebRtcUdpTransport>`. Each loop iteration:
///
///  1. Calls `transport.recv_from(...)` with the transport's read
///     timeout — usually 100 ms. On a real datagram, locks the peer
///     and feeds the bytes through `handle_inbound_datagram`. On
///     `WouldBlock`/`TimedOut`, falls through silently.
///  2. Locks the peer and drains `take_outbound_rtp()`, writing each
///     batch to the transport. `take_outbound_rtp` advances the str0m
///     timeout internally, so the worker does not need to call
///     `Input::Timeout` itself.
///
/// Shutdown: `Drop` flips the shared `AtomicBool` and calls
/// `join` on the thread; if the join blocks for more than a short
/// grace period we detach so we don't deadlock the server's main
/// thread on a wedged worker.
#[cfg(feature = "webrtc-peer")]
pub struct WebRtcIoWorker {
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
    transport: Arc<dyn apprelay_core::WebRtcUdpTransport>,
}

#[cfg(feature = "webrtc-peer")]
impl std::fmt::Debug for WebRtcIoWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebRtcIoWorker")
            .field("local_addr", &self.transport.local_addr())
            .field(
                "shutdown",
                &self.shutdown.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field("running", &self.join_handle.is_some())
            .finish()
    }
}

#[cfg(feature = "webrtc-peer")]
impl WebRtcIoWorker {
    /// Spawn the worker thread. `peer` must be the same
    /// `Arc<Mutex<...>>` stored on `ServerServices`. `transport` is the
    /// shared UDP socket whose `local_addr()` was already plumbed into
    /// the peer's host candidate.
    pub fn spawn(
        peer: Arc<Mutex<Box<dyn apprelay_core::WebRtcPeer>>>,
        transport: Arc<dyn apprelay_core::WebRtcUdpTransport>,
    ) -> Self {
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let worker_transport = Arc::clone(&transport);
        let join_handle = std::thread::Builder::new()
            .name("apprelay-webrtc-io".to_string())
            .spawn(move || {
                let local_addr = worker_transport.local_addr();
                let mut buf = [0u8; 2048];
                while !worker_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    match worker_transport.recv_from(&mut buf) {
                        Ok((n, source)) => {
                            let mut peer =
                                peer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                            // Ignore both Ok(false) (no Rtc claimed
                            // the datagram) and Err — the worker is a
                            // best-effort pump; failed parses are
                            // expected during startup.
                            let _ = peer.handle_inbound_datagram(source, local_addr, &buf[..n]);
                        }
                        Err(err)
                            if matches!(
                                err.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            // Idle tick — fall through to the outbound
                            // drain.
                        }
                        Err(_) => {
                            // Transient read error. Skip outbound for
                            // this iteration to avoid spinning on a
                            // broken socket; the next loop will retry.
                            continue;
                        }
                    }

                    let outbound = {
                        let mut peer = peer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        peer.take_outbound_rtp()
                    };
                    for batch in outbound {
                        let _ = worker_transport.send_to(&batch.payload, batch.destination);
                    }
                }
            })
            .ok();

        Self {
            shutdown,
            join_handle,
            transport,
        }
    }

    /// Return the post-bind local address that the peer's host
    /// candidate advertises. Useful for tests that need to send
    /// datagrams at the worker without going through the peer.
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.transport.local_addr()
    }
}

#[cfg(feature = "webrtc-peer")]
impl Drop for WebRtcIoWorker {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            // Best-effort join: if the worker is wedged we don't want
            // to block the server's shutdown forever. The transport
            // socket itself drops with the `Arc<UdpSocket>` ref count
            // hitting zero, so any in-flight `recv_from` will return.
            let _ = handle.join();
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

pub struct ServerControlPlane {
    services: ServerServices,
    client_authorization: InMemoryClientAuthorizationService,
    config: ServerConfig,
    config_repository: Option<Arc<dyn ServerConfigRepository>>,
    heartbeat_sequence: AtomicU64,
    session_owners: HashMap<String, String>,
    /// Highest `EncodedVideoFrame.sequence` already pushed into the
    /// peer for each active video stream. Used by the encoded-frame
    /// pump to dedupe so the same `last_frame` is not delivered twice
    /// when the encoder hasn't advanced between polls. Cleared when a
    /// stream stops or its owning session closes.
    peer_pushed_sequences: HashMap<String, u64>,
}

/// Zero-cost [`EventSink`] used by the non-audit public control-plane
/// entry points. Lets the inner methods always emit through a sink
/// without forcing every caller to thread one through.
pub struct NullEventSink;

impl EventSink for NullEventSink {
    fn record(&mut self, _event: ServerEvent) {}
}

impl std::fmt::Debug for ServerControlPlane {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerControlPlane")
            .field("services", &self.services)
            .field("client_authorization", &self.client_authorization)
            .field("config", &self.config)
            .field("config_repository", &self.config_repository.is_some())
            .field("heartbeat_sequence", &self.heartbeat_sequence)
            .field("session_owners", &self.session_owners)
            .field("peer_pushed_sequences", &self.peer_pushed_sequences)
            .finish()
    }
}

impl ServerControlPlane {
    pub fn new(services: ServerServices, config: ServerConfig) -> Self {
        Self::with_optional_config_repository(services, config, None)
    }

    pub fn with_config_repository(
        services: ServerServices,
        config: ServerConfig,
        config_repository: impl ServerConfigRepository + 'static,
    ) -> Self {
        Self::with_optional_config_repository(services, config, Some(Arc::new(config_repository)))
    }

    fn with_optional_config_repository(
        services: ServerServices,
        config: ServerConfig,
        config_repository: Option<Arc<dyn ServerConfigRepository>>,
    ) -> Self {
        let client_authorization =
            InMemoryClientAuthorizationService::new(config.authorized_clients.clone());
        Self {
            services,
            client_authorization,
            config,
            config_repository,
            heartbeat_sequence: AtomicU64::new(0),
            session_owners: HashMap::new(),
            peer_pushed_sequences: HashMap::new(),
        }
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Test-only: advance the in-memory video encoder for `stream_id`
    /// by one frame. Mirrors
    /// [`ServerServices::advance_encoded_frame_for_test`] and is used
    /// by the encoded-frame pump tests; not part of the production
    /// surface area.
    #[doc(hidden)]
    pub fn advance_encoded_frame_for_test(&mut self, stream_id: &str) -> Option<u64> {
        self.services.advance_encoded_frame_for_test(stream_id)
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
        let client = self.authorize_paired_client(auth)?;
        if !client.allows_application(&request.application_id) {
            return Err(ControlError::Service(AppRelayError::PermissionDenied(
                format!(
                    "client {} is not authorized for application {}",
                    client.id, request.application_id
                ),
            )));
        }
        let session = self
            .services
            .create_session(request)
            .map_err(ControlError::from)?;
        self.session_owners
            .insert(session.id.clone(), client.id.clone());
        Ok(session)
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

    pub fn locally_approve_pairing_with_audit(
        &mut self,
        request: ApprovePairingRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<apprelay_core::AuthorizedClient> {
        let request_id = request.request_id.clone();
        let client = match self.locally_approve_pairing_inner(request) {
            Ok(client) => client,
            Err(ControlError::Service(error)) => {
                events.record(ServerEvent::PairingApprovalFailed {
                    request_id,
                    reason: error.user_message(),
                });
                return Err(ControlError::Service(error));
            }
            Err(error) => return Err(error),
        };
        events.record(ServerEvent::PairingApproved {
            request_id,
            client_id: client.id.clone(),
        });
        Ok(client)
    }

    fn locally_approve_pairing_inner(
        &mut self,
        request: ApprovePairingRequest,
    ) -> ControlResult<apprelay_core::AuthorizedClient> {
        let mut next_authorization = self.client_authorization.clone();
        let client = next_authorization
            .approve_pairing(request)
            .map_err(ControlError::Service)?;
        self.commit_authorization_change(next_authorization)?;
        Ok(client)
    }

    pub fn locally_revoke_client(
        &mut self,
        request: RevokeClientRequest,
    ) -> ControlResult<apprelay_core::AuthorizedClient> {
        self.locally_revoke_client_with_teardown(request, &mut NullEventSink)
            .map(|(client, _)| client)
    }

    /// Audit-aware sibling of [`Self::locally_revoke_client`]. Drives
    /// the same teardown path but threads an [`EventSink`] through the
    /// session-close cascade so callers see `WebRtcPeerStopped` (or
    /// `WebRtcPeerRejected`) audit events for every owned video
    /// stream. Returns the closed sessions for callers that want to
    /// emit additional audit on top.
    pub fn locally_revoke_client_with_audit(
        &mut self,
        request: RevokeClientRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<(apprelay_core::AuthorizedClient, Vec<ApplicationSession>)> {
        self.locally_revoke_client_with_teardown(request, events)
    }

    pub fn revoke_client(
        &mut self,
        auth: &ControlAuth,
        request: RevokeClientRequest,
    ) -> ControlResult<apprelay_core::AuthorizedClient> {
        self.authorize(auth)?;
        self.locally_revoke_client(request)
    }

    fn revoke_client_with_teardown(
        &mut self,
        auth: &ControlAuth,
        request: RevokeClientRequest,
        events: &mut dyn EventSink,
    ) -> ControlResult<(apprelay_core::AuthorizedClient, Vec<ApplicationSession>)> {
        self.authorize(auth)?;
        self.locally_revoke_client_with_teardown(request, events)
    }

    fn locally_revoke_client_with_teardown(
        &mut self,
        request: RevokeClientRequest,
        events: &mut dyn EventSink,
    ) -> ControlResult<(apprelay_core::AuthorizedClient, Vec<ApplicationSession>)> {
        let mut next_authorization = self.client_authorization.clone();
        let client = next_authorization
            .revoke_client(request)
            .map_err(ControlError::Service)?;
        self.commit_authorization_change(next_authorization)?;
        let closed_sessions = self.close_sessions_owned_by(&client.id, events);
        Ok((client, closed_sessions))
    }

    fn commit_authorization_change(
        &mut self,
        next_authorization: InMemoryClientAuthorizationService,
    ) -> ControlResult<()> {
        let mut next_config = self.config.clone();
        next_config.authorized_clients = next_authorization.authorized_clients();

        if let Some(repository) = &self.config_repository {
            repository.save(&next_config).map_err(|error| {
                ControlError::Service(AppRelayError::ServiceUnavailable(format!(
                    "failed to persist server config: {error:?}"
                )))
            })?;
        }

        self.client_authorization = next_authorization;
        self.config = next_config;
        Ok(())
    }

    fn close_sessions_owned_by(
        &mut self,
        client_id: &str,
        events: &mut dyn EventSink,
    ) -> Vec<ApplicationSession> {
        let session_ids = self
            .session_owners
            .iter()
            .filter(|(_, owner_client_id)| *owner_client_id == client_id)
            .map(|(session_id, _)| session_id.clone())
            .collect::<Vec<_>>();
        let mut closed_sessions = Vec::new();

        for session_id in session_ids {
            // Capture the per-session video streams before tearing
            // down so the peer cascade can fire `stop` for each one
            // even after the service drops them from its active list.
            let cascade_streams: Vec<String> = self
                .services
                .active_video_streams()
                .into_iter()
                .filter(|stream| {
                    stream.session_id == session_id
                        && stream.state != apprelay_protocol::VideoStreamState::Stopped
                })
                .map(|stream| stream.id)
                .collect();
            if let Ok(session) = self.services.close_session(&session_id) {
                closed_sessions.push(session);
            }
            self.session_owners.remove(&session_id);
            self.cascade_peer_stop_for_streams(&session_id, client_id, &cascade_streams, events);
        }

        closed_sessions
    }

    /// Drive `peer.stop` for every captured stream and emit one
    /// `WebRtcPeerStopped` (or `WebRtcPeerRejected` on error) event per
    /// stream. The peer mutex is taken once per stream because the
    /// trait method is non-batched; each call still completes before
    /// the next event is emitted, so audit ordering matches the lock
    /// order. Tracking state for the dedup pump is also cleared here.
    fn cascade_peer_stop_for_streams(
        &mut self,
        session_id: &str,
        paired_client: &str,
        stream_ids: &[String],
        events: &mut dyn EventSink,
    ) {
        for stream_id in stream_ids {
            self.peer_pushed_sequences.remove(stream_id);
            let peer_result = {
                let mut peer = self
                    .services
                    .webrtc_peer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (**peer).stop(session_id, stream_id)
            };
            match peer_result {
                Ok(()) => {
                    events.record(ServerEvent::WebRtcPeerStopped {
                        session_id: session_id.to_string(),
                        stream_id: stream_id.clone(),
                        paired_client: paired_client.to_string(),
                    });
                }
                Err(error) => {
                    events.record(ServerEvent::WebRtcPeerRejected {
                        session_id: session_id.to_string(),
                        paired_client: paired_client.to_string(),
                        reason: error.user_message(),
                    });
                }
            }
        }
    }

    pub fn resize_session(
        &mut self,
        auth: &ControlAuth,
        request: ResizeSessionRequest,
    ) -> ControlResult<ApplicationSession> {
        self.authorize_session_owner(auth, &request.session_id)?;
        self.services.resize_session(request).map_err(Into::into)
    }

    pub fn close_session(
        &mut self,
        auth: &ControlAuth,
        session_id: &str,
    ) -> ControlResult<ApplicationSession> {
        self.close_session_inner(auth, session_id, &mut NullEventSink)
    }

    pub fn close_session_with_audit(
        &mut self,
        auth: &ControlAuth,
        session_id: &str,
        events: &mut dyn EventSink,
    ) -> ControlResult<ApplicationSession> {
        self.close_session_inner(auth, session_id, events)
    }

    fn close_session_inner(
        &mut self,
        auth: &ControlAuth,
        session_id: &str,
        events: &mut dyn EventSink,
    ) -> ControlResult<ApplicationSession> {
        let client = self.authorize_session_owner(auth, session_id)?;
        // Snapshot the active streams owned by this session BEFORE
        // closing so the cascade can target the peer with deterministic
        // (session_id, stream_id) pairs even after the underlying
        // service tears them down.
        let cascade_streams: Vec<String> = self
            .services
            .active_video_streams()
            .into_iter()
            .filter(|stream| {
                stream.session_id == session_id
                    && stream.state != apprelay_protocol::VideoStreamState::Stopped
            })
            .map(|stream| stream.id)
            .collect();
        let session = self
            .services
            .close_session(session_id)
            .map_err(ControlError::from)?;
        self.cascade_peer_stop_for_streams(session_id, &client.id, &cascade_streams, events);
        Ok(session)
    }

    pub fn active_sessions(&self, auth: &ControlAuth) -> ControlResult<Vec<ApplicationSession>> {
        let client = self.authorize_paired_client(auth)?;
        Ok(self
            .services
            .active_sessions()
            .into_iter()
            .filter(|session| self.client_owns_session(&client.id, &session.id))
            .collect())
    }

    pub fn forward_input(
        &mut self,
        auth: &ControlAuth,
        request: ForwardInputRequest,
    ) -> ControlResult<InputDelivery> {
        self.forward_input_inner(auth, request)
            .map(|(delivery, _)| delivery)
    }

    pub fn forward_input_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: ForwardInputRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<InputDelivery> {
        let (delivery, event) = self.forward_input_inner(auth, request)?;
        if let Some(event) = event {
            events.record(event);
        }
        Ok(delivery)
    }

    fn forward_input_inner(
        &mut self,
        auth: &ControlAuth,
        request: ForwardInputRequest,
    ) -> ControlResult<(InputDelivery, Option<ServerEvent>)> {
        let client = self.authorize_session_owner(auth, &request.session_id)?;
        let requested_event = request.event.clone();
        let delivery = self
            .services
            .forward_input(request)
            .map_err(ControlError::from)?;
        let audit_event = input_focus_event(&client.id, &delivery, &requested_event);
        Ok((delivery, audit_event))
    }

    pub fn active_input_focus(
        &self,
        auth: &ControlAuth,
    ) -> ControlResult<Option<ActiveInputFocus>> {
        let client = self.authorize_paired_client(auth)?;
        Ok(self
            .services
            .active_input_focus()
            .filter(|focus| self.client_owns_session(&client.id, &focus.session_id)))
    }

    pub fn start_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: StartVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.start_video_stream_inner(auth, request, &mut NullEventSink)
    }

    pub fn start_video_stream_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: StartVideoStreamRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<VideoStreamSession> {
        self.start_video_stream_inner(auth, request, events)
    }

    fn start_video_stream_inner(
        &mut self,
        auth: &ControlAuth,
        request: StartVideoStreamRequest,
        events: &mut dyn EventSink,
    ) -> ControlResult<VideoStreamSession> {
        let client = self.authorize_session_owner(auth, &request.session_id)?;
        let stream = self
            .services
            .start_video_stream(request)
            .map_err(ControlError::from)?;
        events.record(video_stream_started_event(&client.id, &stream));

        let peer_result = {
            let mut peer = self
                .services
                .webrtc_peer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (**peer).start(
                &stream.session_id,
                &stream.id,
                apprelay_protocol::WebRtcPeerRole::Answerer,
            )
        };
        match peer_result {
            Ok(()) => {
                events.record(ServerEvent::WebRtcPeerStarted {
                    session_id: stream.session_id.clone(),
                    stream_id: stream.id.clone(),
                    role: apprelay_protocol::WebRtcPeerRole::Answerer
                        .label()
                        .to_string(),
                    paired_client: client.id.clone(),
                });
                Ok(stream)
            }
            Err(error) => {
                // Roll back so the server doesn't expose a stream the
                // peer can't service. The rollback is best-effort; if
                // it fails, we still surface the original peer error.
                let _ = self.services.stop_video_stream(StopVideoStreamRequest {
                    stream_id: stream.id.clone(),
                });
                events.record(ServerEvent::WebRtcPeerRejected {
                    session_id: stream.session_id,
                    paired_client: client.id,
                    reason: error.user_message(),
                });
                Err(ControlError::Service(error))
            }
        }
    }

    pub fn stop_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: StopVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.stop_video_stream_inner(auth, request, &mut NullEventSink)
    }

    pub fn stop_video_stream_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: StopVideoStreamRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<VideoStreamSession> {
        self.stop_video_stream_inner(auth, request, events)
    }

    fn stop_video_stream_inner(
        &mut self,
        auth: &ControlAuth,
        request: StopVideoStreamRequest,
        events: &mut dyn EventSink,
    ) -> ControlResult<VideoStreamSession> {
        let client = self.authorize_video_stream_owner(auth, &request.stream_id)?;
        let stream = self
            .services
            .stop_video_stream(request)
            .map_err(ControlError::from)?;
        events.record(video_stream_stopped_event(&client.id, &stream));
        // Drop any dedup tracker so a future re-start of the same id
        // does not skip its very first encoded frame.
        self.peer_pushed_sequences.remove(&stream.id);

        let peer_result = {
            let mut peer = self
                .services
                .webrtc_peer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (**peer).stop(&stream.session_id, &stream.id)
        };
        match peer_result {
            Ok(()) => {
                events.record(ServerEvent::WebRtcPeerStopped {
                    session_id: stream.session_id.clone(),
                    stream_id: stream.id.clone(),
                    paired_client: client.id.clone(),
                });
                Ok(stream)
            }
            Err(error) => {
                events.record(ServerEvent::WebRtcPeerRejected {
                    session_id: stream.session_id,
                    paired_client: client.id,
                    reason: error.user_message(),
                });
                Err(ControlError::Service(error))
            }
        }
    }

    pub fn reconnect_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: ReconnectVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.reconnect_video_stream_inner(auth, request)
            .map(|(stream, _)| stream)
    }

    pub fn reconnect_video_stream_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: ReconnectVideoStreamRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<VideoStreamSession> {
        let (stream, event) = self.reconnect_video_stream_inner(auth, request)?;
        events.record(event);
        Ok(stream)
    }

    fn reconnect_video_stream_inner(
        &mut self,
        auth: &ControlAuth,
        request: ReconnectVideoStreamRequest,
    ) -> ControlResult<(VideoStreamSession, ServerEvent)> {
        let client = self.authorize_video_stream_owner(auth, &request.stream_id)?;
        let stream = self
            .services
            .reconnect_video_stream(request)
            .map_err(ControlError::from)?;
        let event = video_stream_reconnected_event(&client.id, &stream);
        Ok((stream, event))
    }

    pub fn negotiate_video_stream(
        &mut self,
        auth: &ControlAuth,
        request: NegotiateVideoStreamRequest,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_video_stream_owner(auth, &request.stream_id)?;
        self.services
            .negotiate_video_stream(request)
            .map_err(Into::into)
    }

    pub fn video_stream_status(
        &mut self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> ControlResult<VideoStreamSession> {
        self.authorize_video_stream_owner(auth, stream_id)?;
        self.services
            .video_stream_status(stream_id)
            .map_err(Into::into)
    }

    pub fn active_video_streams(
        &mut self,
        auth: &ControlAuth,
    ) -> ControlResult<Vec<VideoStreamSession>> {
        let client = self.authorize_paired_client(auth)?;
        Ok(self
            .services
            .active_video_streams()
            .into_iter()
            .filter(|stream| self.client_owns_session(&client.id, &stream.session_id))
            .collect())
    }

    pub fn start_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: StartAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.start_audio_stream_inner(auth, request)
            .map(|(stream, _)| stream)
    }

    pub fn start_audio_stream_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: StartAudioStreamRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<AudioStreamSession> {
        let (stream, event) = self.start_audio_stream_inner(auth, request)?;
        events.record(event);
        Ok(stream)
    }

    fn start_audio_stream_inner(
        &mut self,
        auth: &ControlAuth,
        request: StartAudioStreamRequest,
    ) -> ControlResult<(AudioStreamSession, ServerEvent)> {
        let client = self.authorize_session_owner(auth, &request.session_id)?;
        let stream = self
            .services
            .start_audio_stream(request)
            .map_err(ControlError::from)?;
        let event = audio_stream_started_event(&client.id, &stream);
        Ok((stream, event))
    }

    pub fn stop_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: StopAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.stop_audio_stream_inner(auth, request)
            .map(|(stream, _)| stream)
    }

    pub fn stop_audio_stream_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: StopAudioStreamRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<AudioStreamSession> {
        let (stream, event) = self.stop_audio_stream_inner(auth, request)?;
        events.record(event);
        Ok(stream)
    }

    fn stop_audio_stream_inner(
        &mut self,
        auth: &ControlAuth,
        request: StopAudioStreamRequest,
    ) -> ControlResult<(AudioStreamSession, ServerEvent)> {
        let client = self.authorize_audio_stream_owner(auth, &request.stream_id)?;
        let stream = self
            .services
            .stop_audio_stream(request)
            .map_err(ControlError::from)?;
        let event = audio_stream_stopped_event(&client.id, &stream);
        Ok((stream, event))
    }

    pub fn update_audio_stream(
        &mut self,
        auth: &ControlAuth,
        request: UpdateAudioStreamRequest,
    ) -> ControlResult<AudioStreamSession> {
        self.update_audio_stream_inner(auth, request)
            .map(|(stream, _)| stream)
    }

    pub fn update_audio_stream_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: UpdateAudioStreamRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<AudioStreamSession> {
        let (stream, event) = self.update_audio_stream_inner(auth, request)?;
        events.record(event);
        Ok(stream)
    }

    fn update_audio_stream_inner(
        &mut self,
        auth: &ControlAuth,
        request: UpdateAudioStreamRequest,
    ) -> ControlResult<(AudioStreamSession, ServerEvent)> {
        let client = self.authorize_audio_stream_owner(auth, &request.stream_id)?;
        let stream = self
            .services
            .update_audio_stream(request)
            .map_err(ControlError::from)?;
        let event = audio_stream_updated_event(&client.id, &stream);
        Ok((stream, event))
    }

    pub fn audio_stream_status(
        &self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> ControlResult<AudioStreamSession> {
        self.authorize_audio_stream_owner(auth, stream_id)?;
        self.services
            .audio_stream_status(stream_id)
            .map_err(Into::into)
    }

    pub fn active_audio_streams(
        &self,
        auth: &ControlAuth,
    ) -> ControlResult<Vec<AudioStreamSession>> {
        let client = self.authorize_paired_client(auth)?;
        Ok(self
            .services
            .active_audio_streams()
            .into_iter()
            .filter(|stream| self.client_owns_session(&client.id, &stream.session_id))
            .collect())
    }

    pub fn submit_signaling(
        &mut self,
        auth: &ControlAuth,
        request: SubmitSignalingRequest,
    ) -> ControlResult<SignalingSubmitAck> {
        self.submit_signaling_inner(auth, request, &mut NullEventSink)
    }

    pub fn submit_signaling_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: SubmitSignalingRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<SignalingSubmitAck> {
        self.submit_signaling_inner(auth, request, events)
    }

    fn submit_signaling_inner(
        &mut self,
        auth: &ControlAuth,
        request: SubmitSignalingRequest,
        events: &mut dyn EventSink,
    ) -> ControlResult<SignalingSubmitAck> {
        let client = self.authorize_existing_session_owner(auth, &request.session_id)?;
        let sdp_mid = request.envelope.sdp_mid_for_audit().map(str::to_string);
        let session_id = request.session_id.clone();
        let direction = request.direction;
        let direction_label = direction.label().to_string();
        // Clone the envelope before forwarding into the signaling
        // service: the peer also needs a copy when this is a
        // client-submitted (`OfferToAnswerer`) envelope.
        let envelope_for_peer = if direction == SignalingDirection::OfferToAnswerer {
            Some(request.envelope.clone())
        } else {
            None
        };
        let ack = self
            .services
            .submit_signaling(request)
            .map_err(ControlError::from)?;
        events.record(ServerEvent::SignalingEnvelopeSubmitted {
            session_id: session_id.clone(),
            client_id: client.id.clone(),
            direction: direction_label,
            envelope_kind: ack.envelope_kind.clone(),
            sequence: ack.sequence,
            payload_byte_length: ack.payload_byte_length,
            sdp_mid,
        });

        // Polling-direction envelopes (`AnswererToOfferer`) are
        // server-produced traffic the client polls — never fed back
        // into the peer.
        let Some(envelope) = envelope_for_peer else {
            return Ok(ack);
        };

        let consume_result = {
            let mut peer = self
                .services
                .webrtc_peer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (**peer).consume_signaling(&session_id, envelope)
        };
        match consume_result {
            Ok(()) => {
                events.record(ServerEvent::WebRtcPeerSignalingConsumed {
                    session_id: session_id.clone(),
                    paired_client: client.id.clone(),
                    envelope_kind: ack.envelope_kind.clone(),
                });
            }
            Err(error) => {
                // The envelope is already in the signaling queue but
                // the peer rejected it. Surface the failure to the
                // caller so it can retry/abort. The leftover queue
                // entry will be drained by future polls — accepted
                // for D.1.1.0; revisit when adding richer rollback.
                events.record(ServerEvent::WebRtcPeerRejected {
                    session_id,
                    paired_client: client.id,
                    reason: error.user_message(),
                });
                return Err(ControlError::Service(error));
            }
        }

        // Drain any answerer-side envelopes the peer produced (SDP
        // answer, local ICE candidates) and re-inject them as
        // `AnswererToOfferer` traffic so the client's existing
        // poll-signaling flow delivers them. Each injection is
        // audited as a separate `SignalingEnvelopeSubmitted` event
        // attributed to a synthetic `"server"` identity.
        let outbound = {
            let mut peer = self
                .services
                .webrtc_peer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (**peer).take_outbound_signaling(&session_id)
        };
        for envelope in outbound {
            let injected_sdp_mid = envelope.sdp_mid_for_audit().map(str::to_string);
            let injected_kind = envelope.kind_label().to_string();
            let inject_request = SubmitSignalingRequest {
                session_id: session_id.clone(),
                direction: SignalingDirection::AnswererToOfferer,
                envelope,
            };
            match self.services.submit_signaling(inject_request) {
                Ok(injected_ack) => {
                    events.record(ServerEvent::SignalingEnvelopeSubmitted {
                        session_id: session_id.clone(),
                        client_id: "server".to_string(),
                        direction: SignalingDirection::AnswererToOfferer.label().to_string(),
                        envelope_kind: injected_kind,
                        sequence: injected_ack.sequence,
                        payload_byte_length: injected_ack.payload_byte_length,
                        sdp_mid: injected_sdp_mid,
                    });
                }
                Err(error) => {
                    // Backlog full or other typed rejection on the
                    // server-side queue. Surface as a peer rejection
                    // so callers and audit sinks see a single typed
                    // error path; do not fail the client's submit.
                    events.record(ServerEvent::WebRtcPeerRejected {
                        session_id: session_id.clone(),
                        paired_client: client.id.clone(),
                        reason: error.user_message(),
                    });
                }
            }
        }

        Ok(ack)
    }

    pub fn poll_signaling(
        &mut self,
        auth: &ControlAuth,
        request: PollSignalingRequest,
    ) -> ControlResult<SignalingPoll> {
        self.poll_signaling_inner(auth, request, &mut NullEventSink)
    }

    pub fn poll_signaling_with_audit(
        &mut self,
        auth: &ControlAuth,
        request: PollSignalingRequest,
        events: &mut impl EventSink,
    ) -> ControlResult<SignalingPoll> {
        self.poll_signaling_inner(auth, request, events)
    }

    fn poll_signaling_inner(
        &mut self,
        auth: &ControlAuth,
        request: PollSignalingRequest,
        events: &mut dyn EventSink,
    ) -> ControlResult<SignalingPoll> {
        let client = self.authorize_existing_session_owner(auth, &request.session_id)?;
        let session_id = request.session_id.clone();
        let direction_label = request.direction.label().to_string();
        let since_sequence = request.since_sequence;
        let poll = self
            .services
            .poll_signaling(request)
            .map_err(ControlError::from)?;
        // Drain any newly produced encoded frames into the peer before
        // surfacing the SignalingPolled audit event so the audit log
        // shows the pump activity in the same logical poll.
        let client_id = client.id.clone();
        self.pump_active_streams_for_client(&client_id, events);
        let message_count = u32::try_from(poll.messages.len()).unwrap_or(u32::MAX);
        events.record(ServerEvent::SignalingPolled {
            session_id,
            client_id,
            direction: direction_label,
            since_sequence,
            last_sequence: poll.last_sequence,
            message_count,
        });
        Ok(poll)
    }

    /// Push the latest encoded video frame for every active stream
    /// owned by `paired_client` into the peer. Called from
    /// [`Self::poll_signaling_inner`] so the pump piggy-backs on the
    /// client's existing poll cadence — no extra clock or thread.
    fn pump_active_streams_for_client(&mut self, paired_client: &str, events: &mut dyn EventSink) {
        let active_streams: Vec<VideoStreamSession> = self
            .services
            .active_video_streams()
            .into_iter()
            .filter(|stream| self.client_owns_session(paired_client, &stream.session_id))
            .collect();
        for stream in active_streams {
            self.pump_video_frame_into_peer(&stream, paired_client, events);
        }
    }

    /// Try to deliver the latest encoded frame for `stream` to the
    /// peer. Skips when the encoder has not advanced since the last
    /// successful push (dedup), and silently swallows the
    /// `negotiation`-pending error from the str0m peer so the same
    /// frame is retried on the next poll once the SDP handshake
    /// completes.
    fn pump_video_frame_into_peer(
        &mut self,
        stream: &VideoStreamSession,
        paired_client: &str,
        events: &mut dyn EventSink,
    ) {
        let Some(frame) = stream.encoding.output.last_frame.as_ref() else {
            return;
        };
        let last_pushed = self
            .peer_pushed_sequences
            .get(&stream.id)
            .copied()
            .unwrap_or(0);
        if frame.sequence <= last_pushed {
            return;
        }

        let push_result = {
            let mut peer = self
                .services
                .webrtc_peer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (**peer).push_encoded_frame(&stream.id, frame)
        };
        match push_result {
            Ok(()) => {
                self.peer_pushed_sequences
                    .insert(stream.id.clone(), frame.sequence);
                events.record(ServerEvent::WebRtcPeerOutboundFrame {
                    session_id: stream.session_id.clone(),
                    stream_id: stream.id.clone(),
                    paired_client: paired_client.to_string(),
                    sequence: frame.sequence,
                    byte_length: frame.byte_length,
                    keyframe: frame.keyframe,
                });
            }
            Err(error) => {
                let message = error.user_message();
                // The str0m peer returns `ServiceUnavailable` with a
                // "negotiation"-prefixed message until the SDP
                // handshake completes. That's the expected idle-state
                // for an Answerer that has not yet seen a client offer
                // — silently skip without advancing the dedup pointer
                // so the same frame is retried next poll.
                if message.to_ascii_lowercase().contains("negotiation") {
                    return;
                }
                events.record(ServerEvent::WebRtcPeerRejected {
                    session_id: stream.session_id.clone(),
                    paired_client: paired_client.to_string(),
                    reason: message,
                });
            }
        }
    }

    pub fn heartbeat(&self, auth: &ControlAuth) -> ControlResult<HeartbeatStatus> {
        self.authorize(auth)?;
        let sequence = self.heartbeat_sequence.fetch_add(1, Ordering::Relaxed) + 1;

        Ok(HeartbeatStatus {
            healthy: true,
            sequence,
        })
    }

    /// Combined backlog depth (both directions) for `session_id`. Exposed
    /// for the foreground wire codec so it can include `current_depth` in
    /// `ServerEvent::SignalingBacklogFull` after a backlog-full rejection.
    pub fn signaling_backlog_depth(&self, session_id: &str) -> usize {
        self.services.signaling_backlog_depth(session_id)
    }

    fn authorize(&self, auth: &ControlAuth) -> Result<(), ControlError> {
        if auth.token() == self.config.auth_token {
            Ok(())
        } else {
            Err(ControlError::Unauthorized)
        }
    }

    fn authorize_paired_client(
        &self,
        auth: &ControlAuth,
    ) -> Result<apprelay_core::AuthorizedClient, ControlError> {
        self.authorize(auth)?;
        self.client_authorization
            .authorize_client(auth.client_id())
            .map_err(ControlError::Service)
    }

    fn authorize_session_owner(
        &self,
        auth: &ControlAuth,
        session_id: &str,
    ) -> Result<apprelay_core::AuthorizedClient, ControlError> {
        let client = self.authorize_paired_client(auth)?;
        if self.client_owns_session(&client.id, session_id) {
            return Ok(client);
        }

        if self.session_owners.contains_key(session_id) {
            return Err(ControlError::Service(AppRelayError::PermissionDenied(
                format!(
                    "client {} is not authorized for session {}",
                    client.id, session_id
                ),
            )));
        }

        Ok(client)
    }

    /// Strict variant of [`Self::authorize_session_owner`] that requires the
    /// session to already be tracked in `session_owners` and owned by the
    /// caller. Unlike `authorize_session_owner`, an unknown `session_id` is
    /// rejected with `PermissionDenied` rather than implicitly accepted.
    ///
    /// This closes a queue-poisoning vector against the in-memory signaling
    /// service: predictable monotonic session ids (`session-N`) combined with
    /// implicit-create on `submit` would otherwise let a paired client stage
    /// envelopes for `session-N+1` before the legitimate owner creates it.
    /// Use this for every signaling op (`submit-sdp-offer`,
    /// `submit-sdp-answer`, `submit-ice-candidate`, `signal-end-of-candidates`,
    /// `poll-signaling`).
    fn authorize_existing_session_owner(
        &self,
        auth: &ControlAuth,
        session_id: &str,
    ) -> Result<apprelay_core::AuthorizedClient, ControlError> {
        let client = self.authorize_paired_client(auth)?;
        if self.client_owns_session(&client.id, session_id) {
            return Ok(client);
        }

        Err(ControlError::Service(AppRelayError::PermissionDenied(
            format!(
                "client {} is not authorized for session {}",
                client.id, session_id
            ),
        )))
    }

    fn authorize_video_stream_owner(
        &mut self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> Result<apprelay_core::AuthorizedClient, ControlError> {
        let client = self.authorize_paired_client(auth)?;
        let stream = self
            .services
            .video_stream_status(stream_id)
            .map_err(ControlError::Service)?;
        if self.client_owns_session(&client.id, &stream.session_id) {
            return Ok(client);
        }

        Err(ControlError::Service(AppRelayError::PermissionDenied(
            format!(
                "client {} is not authorized for session {}",
                client.id, stream.session_id
            ),
        )))
    }

    fn authorize_audio_stream_owner(
        &self,
        auth: &ControlAuth,
        stream_id: &str,
    ) -> Result<apprelay_core::AuthorizedClient, ControlError> {
        let client = self.authorize_paired_client(auth)?;
        let stream = self
            .services
            .audio_stream_status(stream_id)
            .map_err(ControlError::Service)?;
        if self.client_owns_session(&client.id, &stream.session_id) {
            return Ok(client);
        }

        Err(ControlError::Service(AppRelayError::PermissionDenied(
            format!(
                "client {} is not authorized for session {}",
                client.id, stream.session_id
            ),
        )))
    }

    fn client_owns_session(&self, client_id: &str, session_id: &str) -> bool {
        matches!(
            self.session_owners.get(session_id),
            Some(owner_client_id) if owner_client_id == client_id
        )
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

                let client_id = client_id.to_string();
                let label = label.replace("%20", " ");
                match self.control_plane.borrow_mut().request_pairing(
                    &auth,
                    ControlClientIdentity {
                        id: client_id.clone(),
                        label,
                    },
                ) {
                    Ok(pending) => {
                        let event = pairing_requested_event(&pending);
                        Ok((format_pairing_request_response(pending), vec![event]))
                    }
                    Err(ControlError::Service(error)) => {
                        events.record(ServerEvent::PairingRequestFailed {
                            client_id,
                            reason: error.user_message(),
                        });
                        Err(ControlError::Service(error))
                    }
                    Err(error) => Err(error),
                }
            }
            "pairing-revoke" => {
                let Some(client_id) = args.next() else {
                    return "ERROR bad-request".to_string();
                };
                if args.next().is_some() {
                    return "ERROR bad-request".to_string();
                }

                let client_id = client_id.to_string();
                // Collect the cascade events (peer-stop / peer-rejected
                // for each owned video stream) into a temporary sink so
                // they can be returned alongside the existing
                // ClientRevoked + SessionClosed audit events.
                let mut cascade_sink = apprelay_core::InMemoryEventSink::default();
                let revoke_result = self.control_plane.borrow_mut().revoke_client_with_teardown(
                    &auth,
                    RevokeClientRequest {
                        client_id: client_id.clone(),
                    },
                    &mut cascade_sink,
                );
                match revoke_result {
                    Ok((client, closed_sessions)) => {
                        let mut audit_events = vec![ServerEvent::ClientRevoked {
                            client_id: client.id.clone(),
                        }];
                        audit_events.extend(
                            closed_sessions
                                .iter()
                                .map(|session| session_closed_event(&client.id, session)),
                        );
                        audit_events.extend(cascade_sink.events().iter().cloned());
                        Ok((format_pairing_revoke_response(client), audit_events))
                    }
                    Err(ControlError::Service(error)) => {
                        events.record(ServerEvent::ClientRevocationFailed {
                            client_id,
                            reason: error.user_message(),
                        });
                        Err(ControlError::Service(error))
                    }
                    Err(error) => Err(error),
                }
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
                // Capture peer-cascade events (stop/rejected) into a
                // temporary sink so they're surfaced in the audit log
                // alongside the SessionClosed event.
                let mut cascade_sink = apprelay_core::InMemoryEventSink::default();
                let close_result = self.control_plane.borrow_mut().close_session_with_audit(
                    &ControlAuth::with_client_id(auth.token(), &client_id),
                    session_id,
                    &mut cascade_sink,
                );
                close_result.map(|session| {
                    let mut audit_events = vec![session_closed_event(&client_id, &session)];
                    audit_events.extend(cascade_sink.events().iter().cloned());
                    (format_close_session_response(session), audit_events)
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
            "submit-sdp-offer" => match parse_signaling_offer_args(&mut args) {
                Ok(parsed) => match self.handle_submit_signaling(auth.token(), parsed, events) {
                    SubmitSignalingDispatch::Forward(result) => result,
                    SubmitSignalingDispatch::ShortCircuit(response) => return response,
                },
                Err(error) => return error.to_response().to_string(),
            },
            "submit-sdp-answer" => match parse_signaling_answer_args(&mut args) {
                Ok(parsed) => match self.handle_submit_signaling(auth.token(), parsed, events) {
                    SubmitSignalingDispatch::Forward(result) => result,
                    SubmitSignalingDispatch::ShortCircuit(response) => return response,
                },
                Err(error) => return error.to_response().to_string(),
            },
            "submit-ice-candidate" => match parse_signaling_candidate_args(&mut args) {
                Ok(parsed) => match self.handle_submit_signaling(auth.token(), parsed, events) {
                    SubmitSignalingDispatch::Forward(result) => result,
                    SubmitSignalingDispatch::ShortCircuit(response) => return response,
                },
                Err(error) => return error.to_response().to_string(),
            },
            "signal-end-of-candidates" => match parse_signaling_end_args(&mut args) {
                Ok(parsed) => match self.handle_submit_signaling(auth.token(), parsed, events) {
                    SubmitSignalingDispatch::Forward(result) => result,
                    SubmitSignalingDispatch::ShortCircuit(response) => return response,
                },
                Err(error) => return error.to_response().to_string(),
            },
            "poll-signaling" => match parse_poll_signaling_args(&mut args) {
                Ok(parsed) => self.handle_poll_signaling(auth.token(), parsed),
                Err(error) => return error.to_response().to_string(),
            },
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

    fn handle_submit_signaling(
        &self,
        token: &str,
        parsed: ParsedSignalingSubmit,
        events: &mut impl EventSink,
    ) -> SubmitSignalingDispatch {
        let ParsedSignalingSubmit { client_id, request } = parsed;
        let session_id = request.session_id.clone();
        let direction = request.direction;
        let sdp_mid = request.envelope.sdp_mid_for_audit().map(str::to_string);
        let auth = ControlAuth::with_client_id(token, &client_id);
        let submit_result = self
            .control_plane
            .borrow_mut()
            .submit_signaling(&auth, request);
        match submit_result {
            Ok(ack) => {
                let event = ServerEvent::SignalingEnvelopeSubmitted {
                    session_id,
                    client_id: client_id.clone(),
                    direction: direction.label().to_string(),
                    envelope_kind: ack.envelope_kind.clone(),
                    sequence: ack.sequence,
                    payload_byte_length: ack.payload_byte_length,
                    sdp_mid,
                };
                SubmitSignalingDispatch::Forward(Ok((
                    format_signaling_submit_response(&ack),
                    vec![event],
                )))
            }
            Err(ControlError::Service(error)) => {
                // Backlog-full is a typed flow-control rejection. Short-
                // circuit the outer dispatch so we emit the matching
                // audit event without a misleading `RequestAuthorized`
                // entry, and return a stable wire response.
                if matches!(
                    &error,
                    AppRelayError::ServiceUnavailable(message)
                        if message.starts_with(SIGNALING_BACKLOG_FULL_MESSAGE_PREFIX)
                ) {
                    let current_depth = u32::try_from(
                        self.control_plane
                            .borrow()
                            .signaling_backlog_depth(&session_id),
                    )
                    .unwrap_or(u32::MAX);
                    events.record(ServerEvent::SignalingBacklogFull {
                        session_id,
                        paired_client: client_id,
                        current_depth,
                    });
                    return SubmitSignalingDispatch::ShortCircuit(
                        "ERROR signaling-backlog-full".to_string(),
                    );
                }
                if matches!(&error, AppRelayError::InvalidRequest(_)) {
                    events.record(ServerEvent::SignalingEnvelopeRejected {
                        session_id,
                        client_id,
                        reason: error.user_message(),
                    });
                }
                SubmitSignalingDispatch::Forward(Err(ControlError::Service(error)))
            }
            Err(error) => SubmitSignalingDispatch::Forward(Err(error)),
        }
    }

    fn handle_poll_signaling(
        &self,
        token: &str,
        parsed: ParsedPollSignaling,
    ) -> ControlResult<(String, Vec<ServerEvent>)> {
        let ParsedPollSignaling { client_id, request } = parsed;
        let auth = ControlAuth::with_client_id(token, &client_id);
        let session_id = request.session_id.clone();
        let direction = request.direction;
        let since_sequence = request.since_sequence;
        let poll = self
            .control_plane
            .borrow_mut()
            .poll_signaling(&auth, request)?;
        let response = format_signaling_poll_response(&poll);
        let message_count = u32::try_from(poll.messages.len()).unwrap_or(u32::MAX);
        let event = ServerEvent::SignalingPolled {
            session_id,
            client_id,
            direction: direction.label().to_string(),
            since_sequence,
            last_sequence: poll.last_sequence,
            message_count,
        };
        Ok((response, vec![event]))
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

fn pairing_requested_event(pending: &PendingPairing) -> ServerEvent {
    ServerEvent::PairingRequested {
        request_id: pending.request_id.clone(),
        client_id: pending.client.id.clone(),
    }
}

fn format_pairing_revoke_response(client: apprelay_core::AuthorizedClient) -> String {
    format!(
        "OK pairing-revoke client_id={} label={}",
        line_token(&client.id),
        line_token(&client.label)
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

fn video_stream_started_event(client_id: &str, stream: &VideoStreamSession) -> ServerEvent {
    ServerEvent::VideoStreamStarted {
        stream_id: stream.id.clone(),
        session_id: stream.session_id.clone(),
        client_id: client_id.to_string(),
        selected_window_id: stream.selected_window_id.clone(),
    }
}

fn video_stream_stopped_event(client_id: &str, stream: &VideoStreamSession) -> ServerEvent {
    ServerEvent::VideoStreamStopped {
        stream_id: stream.id.clone(),
        session_id: stream.session_id.clone(),
        client_id: client_id.to_string(),
        selected_window_id: stream.selected_window_id.clone(),
    }
}

fn video_stream_reconnected_event(client_id: &str, stream: &VideoStreamSession) -> ServerEvent {
    ServerEvent::VideoStreamReconnected {
        stream_id: stream.id.clone(),
        session_id: stream.session_id.clone(),
        client_id: client_id.to_string(),
        selected_window_id: stream.selected_window_id.clone(),
    }
}

fn audio_stream_started_event(client_id: &str, stream: &AudioStreamSession) -> ServerEvent {
    ServerEvent::AudioStreamStarted {
        stream_id: stream.id.clone(),
        session_id: stream.session_id.clone(),
        client_id: client_id.to_string(),
        selected_window_id: stream.selected_window_id.clone(),
    }
}

fn audio_stream_stopped_event(client_id: &str, stream: &AudioStreamSession) -> ServerEvent {
    ServerEvent::AudioStreamStopped {
        stream_id: stream.id.clone(),
        session_id: stream.session_id.clone(),
        client_id: client_id.to_string(),
        selected_window_id: stream.selected_window_id.clone(),
    }
}

fn audio_stream_updated_event(client_id: &str, stream: &AudioStreamSession) -> ServerEvent {
    ServerEvent::AudioStreamUpdated {
        stream_id: stream.id.clone(),
        session_id: stream.session_id.clone(),
        client_id: client_id.to_string(),
        selected_window_id: stream.selected_window_id.clone(),
        system_audio_muted: stream.mute.system_audio_muted,
        microphone_muted: stream.mute.microphone_muted,
    }
}

fn input_focus_event(
    client_id: &str,
    delivery: &InputDelivery,
    requested_event: &InputEvent,
) -> Option<ServerEvent> {
    match (requested_event, delivery.status) {
        (InputEvent::Focus, InputDeliveryStatus::Focused) => Some(ServerEvent::InputFocusEnabled {
            session_id: delivery.session_id.clone(),
            client_id: client_id.to_string(),
            selected_window_id: delivery.selected_window_id.clone(),
        }),
        (InputEvent::Blur, InputDeliveryStatus::Blurred) => Some(ServerEvent::InputFocusDisabled {
            session_id: delivery.session_id.clone(),
            client_id: client_id.to_string(),
            selected_window_id: delivery.selected_window_id.clone(),
        }),
        _ => None,
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

struct ParsedSignalingSubmit {
    client_id: String,
    request: SubmitSignalingRequest,
}

struct ParsedPollSignaling {
    client_id: String,
    request: PollSignalingRequest,
}

/// Outcome of dispatching a `submit-*` signaling request through
/// [`ForegroundControlServer::handle_submit_signaling`]. `Forward` lets the
/// outer dispatch keep its standard `RequestAuthorized` / `RequestRejected`
/// behaviour; `ShortCircuit` is used for typed flow-control rejections
/// (e.g. backlog-full) where the handler has already recorded the matching
/// audit event and produced the wire response.
enum SubmitSignalingDispatch {
    Forward(ControlResult<(String, Vec<ServerEvent>)>),
    ShortCircuit(String),
}

#[derive(Clone, Copy, Debug)]
enum SignalingArgError {
    BadRequest,
    InvalidBase64,
    PayloadTooLarge,
}

impl SignalingArgError {
    fn to_response(self) -> &'static str {
        match self {
            Self::BadRequest => "ERROR bad-request",
            Self::InvalidBase64 => "ERROR invalid-base64",
            Self::PayloadTooLarge => "ERROR payload-too-large",
        }
    }
}

fn parse_keyed_args<'a>(
    args: &mut std::str::SplitWhitespace<'a>,
) -> Result<HashMap<&'a str, &'a str>, SignalingArgError> {
    let mut entries = HashMap::new();
    for token in args.by_ref() {
        let Some((key, value)) = token.split_once('=') else {
            return Err(SignalingArgError::BadRequest);
        };
        if key.is_empty() {
            return Err(SignalingArgError::BadRequest);
        }
        if entries.insert(key, value).is_some() {
            return Err(SignalingArgError::BadRequest);
        }
    }
    Ok(entries)
}

fn require_arg<'a>(
    entries: &mut HashMap<&'a str, &'a str>,
    key: &str,
) -> Result<&'a str, SignalingArgError> {
    entries
        .remove(key)
        .filter(|value| !value.is_empty())
        .ok_or(SignalingArgError::BadRequest)
}

fn ensure_no_extra_args(entries: &HashMap<&str, &str>) -> Result<(), SignalingArgError> {
    if entries.is_empty() {
        Ok(())
    } else {
        Err(SignalingArgError::BadRequest)
    }
}

fn parse_signaling_offer_args(
    args: &mut std::str::SplitWhitespace<'_>,
) -> Result<ParsedSignalingSubmit, SignalingArgError> {
    let client_id = args
        .next()
        .ok_or(SignalingArgError::BadRequest)?
        .to_string();
    let mut entries = parse_keyed_args(args)?;
    let session_id = require_arg(&mut entries, "session_id")?.to_string();
    let role =
        SdpRole::parse(require_arg(&mut entries, "role")?).ok_or(SignalingArgError::BadRequest)?;
    let sdp_b64 = require_arg(&mut entries, "sdp_b64")?;
    ensure_no_extra_args(&entries)?;
    let sdp_bytes = decode_signaling_payload(sdp_b64)?;
    let sdp = String::from_utf8(sdp_bytes).map_err(|_| SignalingArgError::InvalidBase64)?;
    Ok(ParsedSignalingSubmit {
        client_id,
        request: SubmitSignalingRequest {
            session_id,
            direction: SignalingDirection::OfferToAnswerer,
            envelope: SignalingEnvelope::SdpOffer { sdp, role },
        },
    })
}

fn parse_signaling_answer_args(
    args: &mut std::str::SplitWhitespace<'_>,
) -> Result<ParsedSignalingSubmit, SignalingArgError> {
    let client_id = args
        .next()
        .ok_or(SignalingArgError::BadRequest)?
        .to_string();
    let mut entries = parse_keyed_args(args)?;
    let session_id = require_arg(&mut entries, "session_id")?.to_string();
    let sdp_b64 = require_arg(&mut entries, "sdp_b64")?;
    ensure_no_extra_args(&entries)?;
    let sdp_bytes = decode_signaling_payload(sdp_b64)?;
    let sdp = String::from_utf8(sdp_bytes).map_err(|_| SignalingArgError::InvalidBase64)?;
    Ok(ParsedSignalingSubmit {
        client_id,
        request: SubmitSignalingRequest {
            session_id,
            direction: SignalingDirection::AnswererToOfferer,
            envelope: SignalingEnvelope::SdpAnswer { sdp },
        },
    })
}

fn parse_signaling_candidate_args(
    args: &mut std::str::SplitWhitespace<'_>,
) -> Result<ParsedSignalingSubmit, SignalingArgError> {
    let client_id = args
        .next()
        .ok_or(SignalingArgError::BadRequest)?
        .to_string();
    let mut entries = parse_keyed_args(args)?;
    let session_id = require_arg(&mut entries, "session_id")?.to_string();
    let direction = parse_direction_arg(require_arg(&mut entries, "direction")?)?;
    let candidate_b64 = require_arg(&mut entries, "candidate_b64")?;
    let sdp_mid = decode_token_arg(require_arg(&mut entries, "sdp_mid")?)?;
    let sdp_mline_index: u16 = require_arg(&mut entries, "sdp_mline_index")?
        .parse()
        .map_err(|_| SignalingArgError::BadRequest)?;
    ensure_no_extra_args(&entries)?;
    let candidate_bytes = decode_signaling_payload(candidate_b64)?;
    let candidate =
        String::from_utf8(candidate_bytes).map_err(|_| SignalingArgError::InvalidBase64)?;
    Ok(ParsedSignalingSubmit {
        client_id,
        request: SubmitSignalingRequest {
            session_id,
            direction,
            envelope: SignalingEnvelope::IceCandidate(IceCandidatePayload {
                candidate,
                sdp_mid,
                sdp_mline_index,
            }),
        },
    })
}

fn parse_signaling_end_args(
    args: &mut std::str::SplitWhitespace<'_>,
) -> Result<ParsedSignalingSubmit, SignalingArgError> {
    let client_id = args
        .next()
        .ok_or(SignalingArgError::BadRequest)?
        .to_string();
    let mut entries = parse_keyed_args(args)?;
    let session_id = require_arg(&mut entries, "session_id")?.to_string();
    let direction = parse_direction_arg(require_arg(&mut entries, "direction")?)?;
    ensure_no_extra_args(&entries)?;
    Ok(ParsedSignalingSubmit {
        client_id,
        request: SubmitSignalingRequest {
            session_id,
            direction,
            envelope: SignalingEnvelope::EndOfCandidates,
        },
    })
}

fn parse_poll_signaling_args(
    args: &mut std::str::SplitWhitespace<'_>,
) -> Result<ParsedPollSignaling, SignalingArgError> {
    let client_id = args
        .next()
        .ok_or(SignalingArgError::BadRequest)?
        .to_string();
    let mut entries = parse_keyed_args(args)?;
    let session_id = require_arg(&mut entries, "session_id")?.to_string();
    let direction = parse_direction_arg(require_arg(&mut entries, "direction")?)?;
    let since_sequence: u64 = require_arg(&mut entries, "since_sequence")?
        .parse()
        .map_err(|_| SignalingArgError::BadRequest)?;
    ensure_no_extra_args(&entries)?;
    Ok(ParsedPollSignaling {
        client_id,
        request: PollSignalingRequest {
            session_id,
            direction,
            since_sequence,
        },
    })
}

fn parse_direction_arg(value: &str) -> Result<SignalingDirection, SignalingArgError> {
    match value {
        "offer-to-answerer" => Ok(SignalingDirection::OfferToAnswerer),
        "answerer-to-offerer" => Ok(SignalingDirection::AnswererToOfferer),
        _ => Err(SignalingArgError::BadRequest),
    }
}

fn decode_token_arg(value: &str) -> Result<String, SignalingArgError> {
    let mut decoded = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(SignalingArgError::BadRequest);
            }
            let high = hex_digit(bytes[index + 1])?;
            let low = hex_digit(bytes[index + 2])?;
            decoded.push((high * 16 + low) as char);
            index += 3;
        } else {
            decoded.push(bytes[index] as char);
            index += 1;
        }
    }
    Ok(decoded)
}

fn hex_digit(byte: u8) -> Result<u8, SignalingArgError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(SignalingArgError::BadRequest),
    }
}

fn decode_signaling_payload(value: &str) -> Result<Vec<u8>, SignalingArgError> {
    if value.len() > MAX_SIGNALING_PAYLOAD_BASE64_BYTES {
        return Err(SignalingArgError::PayloadTooLarge);
    }
    let bytes = BASE64_STANDARD
        .decode(value)
        .map_err(|_| SignalingArgError::InvalidBase64)?;
    if bytes.len() > MAX_SIGNALING_PAYLOAD_DECODED_BYTES {
        return Err(SignalingArgError::PayloadTooLarge);
    }
    Ok(bytes)
}

fn encode_signaling_payload(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

fn format_signaling_submit_response(ack: &SignalingSubmitAck) -> String {
    format!(
        "OK signaling-submit session_id={} direction={} sequence={} kind={} payload_byte_length={}",
        line_token(&ack.session_id),
        ack.direction.label(),
        ack.sequence,
        ack.envelope_kind,
        ack.payload_byte_length
    )
}

fn format_signaling_poll_response(poll: &SignalingPoll) -> String {
    if poll.messages.is_empty() {
        return format!(
            "OK signaling session_id={} direction={} sequence={} count=0 empty=true",
            line_token(&poll.session_id),
            poll.direction.label(),
            poll.last_sequence,
        );
    }

    let mut response = format!(
        "OK signaling session_id={} direction={} sequence={} count={} empty=false",
        line_token(&poll.session_id),
        poll.direction.label(),
        poll.last_sequence,
        poll.messages.len()
    );

    for (index, message) in poll.messages.iter().enumerate() {
        let prefix = format!("msg{index}");
        response.push_str(&format!(
            " {prefix}.sequence={} {prefix}.kind={}",
            message.sequence,
            message.envelope.kind_label(),
        ));
        match &message.envelope {
            SignalingEnvelope::SdpOffer { sdp, role } => {
                response.push_str(&format!(
                    " {prefix}.role={} {prefix}.sdp_b64={}",
                    role.label(),
                    encode_signaling_payload(sdp.as_bytes()),
                ));
            }
            SignalingEnvelope::SdpAnswer { sdp } => {
                response.push_str(&format!(
                    " {prefix}.sdp_b64={}",
                    encode_signaling_payload(sdp.as_bytes()),
                ));
            }
            SignalingEnvelope::IceCandidate(payload) => {
                response.push_str(&format!(
                    " {prefix}.sdp_mid={} {prefix}.sdp_mline_index={} {prefix}.candidate_b64={}",
                    line_token(&payload.sdp_mid),
                    payload.sdp_mline_index,
                    encode_signaling_payload(payload.candidate.as_bytes()),
                ));
            }
            SignalingEnvelope::EndOfCandidates => {}
        }
    }
    response
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
    use apprelay_core::{
        ConfigStoreError, FileServerConfigRepository, InMemoryEventSink, MAX_ENVELOPES_PER_SESSION,
    };

    #[derive(Clone, Debug)]
    struct FailingServerConfigRepository;

    impl ServerConfigRepository for FailingServerConfigRepository {
        fn load(&self) -> Result<ServerConfig, ConfigStoreError> {
            Err(ConfigStoreError::CorruptedStore)
        }

        fn save(&self, _config: &ServerConfig) -> Result<(), ConfigStoreError> {
            Err(ConfigStoreError::CorruptedStore)
        }
    }

    fn paired_server_config() -> ServerConfig {
        let mut config = ServerConfig::local("correct-token");
        config.authorized_clients = vec![apprelay_core::AuthorizedClient::new(
            "test-client",
            "Test Client",
        )];
        config
    }

    fn paired_server_config_with_application_grants(application_ids: &[&str]) -> ServerConfig {
        let mut config = ServerConfig::local("correct-token");
        config.authorized_clients = vec![
            apprelay_core::AuthorizedClient::with_allowed_application_ids(
                "test-client",
                "Test Client",
                application_ids.iter().copied(),
            ),
        ];
        config
    }

    fn two_client_server_config() -> ServerConfig {
        let mut config = ServerConfig::local("correct-token");
        config.authorized_clients = vec![
            apprelay_core::AuthorizedClient::new("client-1", "Client One"),
            apprelay_core::AuthorizedClient::new("client-2", "Client Two"),
        ];
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

        let mut file = std::fs::File::create(path).expect("create executable script");
        file.write_all(contents.as_bytes())
            .expect("write executable script");
        file.sync_all().expect("sync executable script");
        drop(file);
        let mut permissions = std::fs::metadata(path)
            .expect("read executable script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark executable script");
        std::thread::sleep(std::time::Duration::from_millis(5));
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

    #[cfg(not(feature = "webrtc-peer"))]
    #[test]
    fn webrtc_peer_default_uses_in_memory_no_op() {
        let services = ServerServices::for_current_platform();
        let mut peer = services
            .webrtc_peer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        peer.start(
            "session-1",
            "stream-1",
            apprelay_protocol::WebRtcPeerRole::Offerer,
        )
        .expect("in-memory peer accepts start in default builds");
        peer.stop("session-1", "stream-1")
            .expect("in-memory peer accepts stop");
        assert!(peer.take_outbound_signaling("session-1").is_empty());
        assert!(peer.take_outbound_rtp().is_empty());
    }

    #[cfg(feature = "webrtc-peer")]
    #[test]
    fn webrtc_peer_feature_swaps_in_str0m_peer() {
        let services = ServerServices::for_current_platform();
        let mut peer = services
            .webrtc_peer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        peer.start(
            "session-1",
            "stream-1",
            apprelay_protocol::WebRtcPeerRole::Offerer,
        )
        .expect("str0m peer accepts start as offerer");
        // The Phase D.1.0 offerer queues a real local SDP offer for the
        // caller to forward; the in-memory peer never produces one. This
        // is the observable signal that the swap landed the str0m peer.
        let outbound = peer.take_outbound_signaling("session-1");
        assert_eq!(
            outbound.len(),
            1,
            "expected exactly one local offer envelope"
        );
        match &outbound[0] {
            apprelay_protocol::SignalingEnvelope::SdpOffer { sdp, role } => {
                assert!(sdp.starts_with("v=0"), "expected real SDP, got: {sdp}");
                assert_eq!(*role, apprelay_protocol::SdpRole::Offerer);
            }
            other => panic!("expected SdpOffer envelope, got {other:?}"),
        }
        peer.stop("session-1", "stream-1")
            .expect("str0m peer stop succeeds");
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
    fn control_plane_allows_create_session_for_client_application_grant() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config_with_application_grants(&["terminal"]),
        );

        let session = control_plane
            .create_session(
                &paired_auth(),
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create granted session");

        assert_eq!(session.application_id, "terminal");
    }

    #[test]
    fn control_plane_rejects_create_session_outside_client_application_grants() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config_with_application_grants(&["terminal"]),
        );

        assert_eq!(
            control_plane.create_session(
                &paired_auth(),
                CreateSessionRequest {
                    application_id: "browser".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client test-client is not authorized for application browser".to_string()
            )))
        );
        assert!(control_plane.services.active_sessions().is_empty());
    }

    #[test]
    fn control_plane_keeps_session_policy_after_client_application_grants() {
        let mut services = ServerServices::new(Platform::Linux, "test");
        services.session_service =
            InMemoryApplicationSessionService::new(SessionPolicy::allow_applications(vec![
                "terminal".to_string(),
            ]));
        let mut control_plane = ServerControlPlane::new(
            services,
            paired_server_config_with_application_grants(&["terminal", "browser"]),
        );

        assert_eq!(
            control_plane.create_session(
                &paired_auth(),
                CreateSessionRequest {
                    application_id: "browser".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "application browser is not allowed".to_string()
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

        let mut events = InMemoryEventSink::default();
        let approved = control_plane
            .locally_approve_pairing_with_audit(
                ApprovePairingRequest {
                    request_id: pending.request_id,
                },
                &mut events,
            )
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
    fn control_plane_persists_runtime_pairing_approval_to_file_config() {
        let root = unique_test_dir("server-runtime-approve-config");
        let repository = FileServerConfigRepository::new(root.join("server.conf"));
        repository
            .save(&ServerConfig::local("correct-token"))
            .expect("save initial server config");
        let mut control_plane = ServerControlPlane::with_config_repository(
            ServerServices::new(Platform::Linux, "test"),
            repository.load().expect("load initial server config"),
            repository.clone(),
        );
        let pending = control_plane
            .request_pairing(
                &ControlAuth::new("correct-token"),
                ControlClientIdentity {
                    id: "client-1".to_string(),
                    label: "Laptop".to_string(),
                },
            )
            .expect("request pairing");

        let mut events = InMemoryEventSink::default();
        control_plane
            .locally_approve_pairing_with_audit(
                ApprovePairingRequest {
                    request_id: pending.request_id,
                },
                &mut events,
            )
            .expect("approve pairing");

        assert_eq!(
            repository
                .load()
                .expect("load persisted server config")
                .authorized_clients,
            vec![apprelay_core::AuthorizedClient::new("client-1", "Laptop")]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn control_plane_revokes_paired_client_before_sensitive_controls() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        );
        let auth = paired_auth();

        let revoked = control_plane
            .locally_revoke_client(RevokeClientRequest {
                client_id: "test-client".to_string(),
            })
            .expect("revoke paired client");

        assert_eq!(revoked.id, "test-client");
        assert!(control_plane.config.authorized_clients.is_empty());
        assert_eq!(
            control_plane.create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client test-client is not paired".to_string()
            )))
        );
    }

    #[test]
    fn control_plane_persists_runtime_pairing_revoke_to_file_config() {
        let root = unique_test_dir("server-runtime-revoke-config");
        let repository = FileServerConfigRepository::new(root.join("server.conf"));
        repository
            .save(&paired_server_config())
            .expect("save initial server config");
        let mut control_plane = ServerControlPlane::with_config_repository(
            ServerServices::new(Platform::Linux, "test"),
            repository.load().expect("load initial server config"),
            repository.clone(),
        );

        control_plane
            .locally_revoke_client(RevokeClientRequest {
                client_id: "test-client".to_string(),
            })
            .expect("revoke paired client");

        assert!(repository
            .load()
            .expect("load persisted server config")
            .authorized_clients
            .is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn control_plane_rejects_unknown_client_revoke() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        );

        assert_eq!(
            control_plane.locally_revoke_client(RevokeClientRequest {
                client_id: "unknown-client".to_string(),
            }),
            Err(ControlError::Service(AppRelayError::NotFound(
                "client unknown-client was not paired".to_string()
            )))
        );
    }

    #[test]
    fn control_plane_failed_revoke_does_not_overwrite_file_config() {
        let root = unique_test_dir("server-runtime-failed-revoke-config");
        let repository = FileServerConfigRepository::new(root.join("server.conf"));
        let initial_config = paired_server_config();
        repository
            .save(&initial_config)
            .expect("save initial server config");
        let mut control_plane = ServerControlPlane::with_config_repository(
            ServerServices::new(Platform::Linux, "test"),
            repository.load().expect("load initial server config"),
            repository.clone(),
        );

        assert_eq!(
            control_plane.locally_revoke_client(RevokeClientRequest {
                client_id: "unknown-client".to_string(),
            }),
            Err(ControlError::Service(AppRelayError::NotFound(
                "client unknown-client was not paired".to_string()
            )))
        );
        assert_eq!(
            repository.load().expect("load server config after failure"),
            initial_config
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn control_plane_revoke_save_failure_does_not_mutate_active_config() {
        let mut control_plane = ServerControlPlane::with_config_repository(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
            FailingServerConfigRepository,
        );

        let error = control_plane
            .locally_revoke_client(RevokeClientRequest {
                client_id: "test-client".to_string(),
            })
            .expect_err("revoke should fail when config save fails");

        assert!(matches!(
            error,
            ControlError::Service(AppRelayError::ServiceUnavailable(message))
                if message.contains("failed to persist server config")
        ));
        assert_eq!(
            control_plane.config.authorized_clients,
            paired_server_config().authorized_clients
        );
        assert_eq!(
            control_plane
                .create_session(
                    &paired_auth(),
                    CreateSessionRequest {
                        application_id: "terminal".to_string(),
                        viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                    },
                )
                .expect("paired client remains authorized")
                .application_id,
            "terminal"
        );
    }

    #[test]
    fn control_plane_revoke_closes_only_revoked_client_sessions() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            two_client_server_config(),
        );
        let auth_1 = ControlAuth::with_client_id("correct-token", "client-1");
        let auth_2 = ControlAuth::with_client_id("correct-token", "client-2");
        let revoked_session = control_plane
            .create_session(
                &auth_1,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create revoked client session before revocation");
        let retained_session = control_plane
            .create_session(
                &auth_2,
                CreateSessionRequest {
                    application_id: "browser".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1024, 768),
                },
            )
            .expect("create retained client session before revocation");

        control_plane
            .locally_revoke_client(RevokeClientRequest {
                client_id: "client-1".to_string(),
            })
            .expect("revoke paired client");

        assert_eq!(
            control_plane.services.active_sessions(),
            vec![retained_session]
        );
        assert_eq!(
            control_plane.resize_session(
                &auth_1,
                ResizeSessionRequest {
                    session_id: revoked_session.id,
                    viewport: apprelay_protocol::ViewportSize::new(800, 600),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client client-1 is not paired".to_string()
            )))
        );
        assert!(control_plane
            .create_session(
                &auth_2,
                CreateSessionRequest {
                    application_id: "editor".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .is_ok());
    }

    #[test]
    fn control_plane_persisted_revoke_closes_revoked_client_sessions() {
        let root = unique_test_dir("server-runtime-revoke-teardown-config");
        let repository = FileServerConfigRepository::new(root.join("server.conf"));
        repository
            .save(&two_client_server_config())
            .expect("save initial server config");
        let mut control_plane = ServerControlPlane::with_config_repository(
            ServerServices::new(Platform::Linux, "test"),
            repository.load().expect("load initial server config"),
            repository.clone(),
        );
        let auth_1 = ControlAuth::with_client_id("correct-token", "client-1");
        let auth_2 = ControlAuth::with_client_id("correct-token", "client-2");
        control_plane
            .create_session(
                &auth_1,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create revoked client session before revocation");
        let retained_session = control_plane
            .create_session(
                &auth_2,
                CreateSessionRequest {
                    application_id: "browser".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1024, 768),
                },
            )
            .expect("create retained client session before revocation");

        control_plane
            .locally_revoke_client(RevokeClientRequest {
                client_id: "client-1".to_string(),
            })
            .expect("revoke paired client");

        assert_eq!(
            repository
                .load()
                .expect("load persisted server config")
                .authorized_clients,
            vec![apprelay_core::AuthorizedClient::new(
                "client-2",
                "Client Two"
            )]
        );
        assert_eq!(
            control_plane.services.active_sessions(),
            vec![retained_session]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn control_plane_rejects_cross_client_session_controls() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            two_client_server_config(),
        );
        let owner_auth = ControlAuth::with_client_id("correct-token", "client-1");
        let other_auth = ControlAuth::with_client_id("correct-token", "client-2");
        let session = control_plane
            .create_session(
                &owner_auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                },
            )
            .expect("create owner session");
        let expected = Err(ControlError::Service(AppRelayError::PermissionDenied(
            "client client-2 is not authorized for session session-1".to_string(),
        )));

        assert_eq!(control_plane.active_sessions(&other_auth), Ok(Vec::new()));
        assert_eq!(
            control_plane.resize_session(
                &other_auth,
                ResizeSessionRequest {
                    session_id: session.id.clone(),
                    viewport: apprelay_protocol::ViewportSize::new(800, 600),
                },
            ),
            expected
        );
        assert_eq!(
            control_plane.forward_input(
                &other_auth,
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: apprelay_protocol::ViewportSize::new(1280, 720),
                    event: apprelay_protocol::InputEvent::Focus,
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client client-2 is not authorized for session session-1".to_string()
            )))
        );
        assert_eq!(
            control_plane.start_video_stream(
                &other_auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client client-2 is not authorized for session session-1".to_string()
            )))
        );

        let stream = control_plane
            .start_video_stream(
                &owner_auth,
                StartVideoStreamRequest {
                    session_id: session.id,
                },
            )
            .expect("owner starts video stream");

        assert_eq!(
            control_plane.active_video_streams(&other_auth),
            Ok(Vec::new())
        );
        assert_eq!(
            control_plane.video_stream_status(&other_auth, &stream.id),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client client-2 is not authorized for session session-1".to_string()
            )))
        );
        assert_eq!(
            control_plane.stop_video_stream(
                &other_auth,
                StopVideoStreamRequest {
                    stream_id: stream.id,
                },
            ),
            Err(ControlError::Service(AppRelayError::PermissionDenied(
                "client client-2 is not authorized for session session-1".to_string()
            )))
        );
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
        assert!(response.contains("window-resize:supported"));
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
            &[
                ServerEvent::RequestAuthorized {
                    operation: "pairing-request".to_string(),
                },
                ServerEvent::PairingRequested {
                    request_id: "pairing-1".to_string(),
                    client_id: "client-1".to_string(),
                },
            ]
        );
    }

    #[test]
    fn control_plane_records_pairing_approval_success() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );
        let pending = control_plane
            .request_pairing(
                &ControlAuth::new("correct-token"),
                ControlClientIdentity {
                    id: "client-1".to_string(),
                    label: "Laptop".to_string(),
                },
            )
            .expect("request pairing");
        let mut events = InMemoryEventSink::default();

        let approved = control_plane
            .locally_approve_pairing_with_audit(
                ApprovePairingRequest {
                    request_id: pending.request_id,
                },
                &mut events,
            )
            .expect("approve pairing");

        assert_eq!(approved.id, "client-1");
        assert_eq!(
            events.events(),
            &[ServerEvent::PairingApproved {
                request_id: "pairing-1".to_string(),
                client_id: "client-1".to_string(),
            }]
        );
    }

    #[test]
    fn control_plane_records_unknown_pairing_approval_failure() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        );
        let mut events = InMemoryEventSink::default();

        let error = control_plane
            .locally_approve_pairing_with_audit(
                ApprovePairingRequest {
                    request_id: "unknown-pairing".to_string(),
                },
                &mut events,
            )
            .expect_err("reject unknown pairing request");

        assert!(matches!(
            error,
            ControlError::Service(AppRelayError::NotFound(_))
        ));
        assert_eq!(
            events.events(),
            &[ServerEvent::PairingApprovalFailed {
                request_id: "unknown-pairing".to_string(),
                reason: "pairing request unknown-pairing was not found".to_string(),
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
    fn foreground_server_records_valid_token_invalid_pairing_request_failure() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-request correct-token client-1 %20", &mut events),
            "ERROR service client label is required"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::PairingRequestFailed {
                client_id: "client-1".to_string(),
                reason: "client label is required".to_string(),
            }]
        );
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
    fn foreground_server_rejects_unauthorized_invalid_pairing_request_without_client_detail() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            ServerConfig::local("correct-token"),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-request wrong-token client-1 %20", &mut events),
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
            .locally_approve_pairing_with_audit(
                ApprovePairingRequest {
                    request_id: "pairing-1".to_string(),
                },
                &mut events,
            )
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
    fn foreground_server_revokes_paired_client_and_denies_future_session_creation() {
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
            server.handle_request("pairing-revoke correct-token test-client", &mut events),
            "OK pairing-revoke client_id=test-client label=Test%20Client"
        );
        assert_eq!(
            server.handle_request(
                "create-session correct-token test-client terminal 1280 720",
                &mut events,
            ),
            "ERROR service client test-client is not paired"
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
                    operation: "pairing-revoke".to_string(),
                },
                ServerEvent::ClientRevoked {
                    client_id: "test-client".to_string(),
                },
                ServerEvent::SessionClosed {
                    session_id: "session-1".to_string(),
                    application_id: "terminal".to_string(),
                    client_id: "test-client".to_string(),
                },
                ServerEvent::RequestRejected {
                    operation: "create-session".to_string(),
                },
            ]
        );
    }

    #[test]
    fn foreground_server_formats_unknown_pairing_revoke_error() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-revoke correct-token unknown-client", &mut events),
            "ERROR service client unknown-client was not paired"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::ClientRevocationFailed {
                client_id: "unknown-client".to_string(),
                reason: "client unknown-client was not paired".to_string(),
            }]
        );
    }

    #[test]
    fn foreground_server_rejects_unauthorized_pairing_revoke() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-revoke wrong-token test-client", &mut events),
            "ERROR unauthorized"
        );
        assert_eq!(
            events.events(),
            &[ServerEvent::RequestRejected {
                operation: "pairing-revoke".to_string(),
            }]
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

    fn create_signaling_test_session(
        server: &ForegroundControlServer,
        events: &mut InMemoryEventSink,
    ) {
        let response = server.handle_request(
            "create-session correct-token test-client terminal 1280 720",
            events,
        );
        assert!(
            response.starts_with("OK create-session"),
            "create-session failed: {response}"
        );
    }

    fn signaling_server() -> ForegroundControlServer {
        ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        ))
    }

    fn b64(value: &str) -> String {
        BASE64_STANDARD.encode(value.as_bytes())
    }

    #[test]
    fn foreground_server_submits_sdp_offer_and_audits_payload_metrics() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);
        let sdp_b64 = b64("v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\ns=-\r\n");

        let request = format!(
            "submit-sdp-offer correct-token test-client session_id=session-1 role=offerer sdp_b64={sdp_b64}"
        );
        let response = server.handle_request(&request, &mut events);

        assert_eq!(
            response,
            "OK signaling-submit session_id=session-1 direction=offer-to-answerer sequence=1 kind=sdp-offer payload_byte_length=34"
        );
        let recorded = events.events();
        assert!(matches!(
            recorded.last(),
            Some(ServerEvent::SignalingEnvelopeSubmitted {
                session_id,
                client_id,
                direction,
                envelope_kind,
                sequence,
                payload_byte_length,
                sdp_mid,
            }) if session_id == "session-1"
                && client_id == "test-client"
                && direction == "offer-to-answerer"
                && envelope_kind == "sdp-offer"
                && *sequence == 1
                && *payload_byte_length == 34
                && sdp_mid.is_none()
        ));
        let serialized = format!("{recorded:?}");
        assert!(!serialized.contains("v=0"));
        assert!(!serialized.contains(&sdp_b64));
    }

    #[test]
    fn foreground_server_submits_ice_candidate_with_sdp_mid_in_audit() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);
        let candidate_b64 = b64("candidate:1 1 udp 2113937151 192.0.2.1 51234 typ host");

        let request = format!(
            "submit-ice-candidate correct-token test-client session_id=session-1 direction=answerer-to-offerer candidate_b64={candidate_b64} sdp_mid=video sdp_mline_index=0"
        );
        let response = server.handle_request(&request, &mut events);

        assert!(response.starts_with("OK signaling-submit"));
        assert!(response.contains("kind=ice-candidate"));
        assert!(matches!(
            events.events().last(),
            Some(ServerEvent::SignalingEnvelopeSubmitted {
                envelope_kind,
                direction,
                sdp_mid,
                ..
            }) if envelope_kind == "ice-candidate"
                && direction == "answerer-to-offerer"
                && sdp_mid.as_deref() == Some("video")
        ));
        let serialized = format!("{:?}", events.events());
        assert!(!serialized.contains("candidate:1 1 udp"));
    }

    #[test]
    fn foreground_server_signals_end_of_candidates_marker() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);

        let response = server.handle_request(
            "signal-end-of-candidates correct-token test-client session_id=session-1 direction=offer-to-answerer",
            &mut events,
        );

        assert_eq!(
            response,
            "OK signaling-submit session_id=session-1 direction=offer-to-answerer sequence=1 kind=end-of-candidates payload_byte_length=0"
        );

        let poll = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );
        assert!(poll.contains("msg0.kind=end-of-candidates"));
    }

    #[test]
    fn foreground_server_polls_signaling_with_since_sequence_resumption() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);
        let offer_b64 = b64("offer-sdp");
        let answer_b64 = b64("answer-sdp");
        let candidate_b64 = b64("candidate:foo");

        server.handle_request(
            &format!(
                "submit-sdp-offer correct-token test-client session_id=session-1 role=offerer sdp_b64={offer_b64}"
            ),
            &mut events,
        );
        server.handle_request(
            &format!(
                "submit-sdp-answer correct-token test-client session_id=session-1 sdp_b64={answer_b64}"
            ),
            &mut events,
        );
        server.handle_request(
            &format!(
                "submit-ice-candidate correct-token test-client session_id=session-1 direction=offer-to-answerer candidate_b64={candidate_b64} sdp_mid=video sdp_mline_index=0"
            ),
            &mut events,
        );

        let initial = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );
        assert!(initial.contains("count=2"));
        assert!(initial.contains("msg0.sequence=1"));
        assert!(initial.contains("msg0.kind=sdp-offer"));
        assert!(initial.contains(&format!("msg0.sdp_b64={offer_b64}")));
        assert!(initial.contains("msg1.sequence=3"));
        assert!(initial.contains("msg1.kind=ice-candidate"));

        let resumed = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=1",
            &mut events,
        );
        assert!(resumed.contains("count=1"));
        assert!(resumed.contains("msg0.sequence=3"));
        assert!(resumed.contains("msg0.kind=ice-candidate"));

        let answer_poll = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=answerer-to-offerer since_sequence=0",
            &mut events,
        );
        assert!(answer_poll.contains("count=1"));
        assert!(answer_poll.contains("msg0.kind=sdp-answer"));

        let drained = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=99",
            &mut events,
        );
        assert!(drained.contains("count=0"));
        assert!(drained.contains("empty=true"));
    }

    #[test]
    fn foreground_server_signaling_records_polled_event_without_sdp_payload() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);
        server.handle_request(
            &format!(
                "submit-sdp-offer correct-token test-client session_id=session-1 role=offerer sdp_b64={}",
                b64("v=0 sample")
            ),
            &mut events,
        );

        events.events();
        let _ = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );

        let recorded = events.events();
        assert!(recorded.iter().any(|event| matches!(
            event,
            ServerEvent::SignalingPolled {
                session_id,
                client_id,
                direction,
                since_sequence,
                last_sequence,
                message_count,
            } if session_id == "session-1"
                && client_id == "test-client"
                && direction == "offer-to-answerer"
                && *since_sequence == 0
                && *last_sequence == 1
                && *message_count == 1
        )));
        let serialized = format!("{recorded:?}");
        assert!(!serialized.contains("v=0 sample"));
    }

    #[test]
    fn foreground_server_signaling_rejects_invalid_base64() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);

        let response = server.handle_request(
            "submit-sdp-offer correct-token test-client session_id=session-1 role=offerer sdp_b64=not_valid",
            &mut events,
        );
        assert_eq!(response, "ERROR invalid-base64");
    }

    #[test]
    fn foreground_server_signaling_rejects_oversized_payload() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);
        let oversized_b64 = "A".repeat(MAX_SIGNALING_PAYLOAD_BASE64_BYTES + 4);

        let response = server.handle_request(
            &format!(
                "submit-sdp-offer correct-token test-client session_id=session-1 role=offerer sdp_b64={oversized_b64}"
            ),
            &mut events,
        );
        assert_eq!(response, "ERROR payload-too-large");
    }

    #[test]
    fn foreground_server_signaling_rejects_unauthorized_client() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);

        let response = server.handle_request(
            &format!(
                "submit-sdp-offer wrong-token test-client session_id=session-1 role=offerer sdp_b64={}",
                b64("offer")
            ),
            &mut events,
        );
        assert_eq!(response, "ERROR unauthorized");

        let poll_response = server.handle_request(
            "poll-signaling wrong-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );
        assert_eq!(poll_response, "ERROR unauthorized");
    }

    #[test]
    fn foreground_server_signaling_rejects_unpaired_client() {
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            two_client_server_config(),
        ));
        let mut events = InMemoryEventSink::default();
        let creation = server.handle_request(
            "create-session correct-token client-1 terminal 1280 720",
            &mut events,
        );
        assert!(creation.starts_with("OK create-session"));

        let response = server.handle_request(
            &format!(
                "submit-sdp-offer correct-token client-2 session_id=session-1 role=offerer sdp_b64={}",
                b64("offer")
            ),
            &mut events,
        );
        assert_eq!(
            response,
            "ERROR service client client-2 is not authorized for session session-1"
        );
    }

    #[test]
    fn foreground_server_signaling_close_session_drops_queue() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);
        server.handle_request(
            &format!(
                "submit-sdp-offer correct-token test-client session_id=session-1 role=offerer sdp_b64={}",
                b64("offer")
            ),
            &mut events,
        );

        let close = server.handle_request(
            "close-session correct-token test-client session-1",
            &mut events,
        );
        assert!(close.starts_with("OK close-session"));

        // After close, paired client owns no session anymore. A poll for a
        // non-owned existing session id is denied. A poll for an unowned
        // (no-record) id is allowed but the queue is empty because
        // close-session dropped it. Re-create to verify the sequence reset.
        create_signaling_test_session(&server, &mut events);
        let poll = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );
        assert!(poll.contains("count=0"));
        assert!(poll.contains("empty=true"));
    }

    #[test]
    fn foreground_server_signaling_rejects_bad_request_args() {
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);

        // Missing role.
        let no_role = server.handle_request(
            &format!(
                "submit-sdp-offer correct-token test-client session_id=session-1 sdp_b64={}",
                b64("offer")
            ),
            &mut events,
        );
        assert_eq!(no_role, "ERROR bad-request");

        // Unknown direction.
        let bad_direction = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 direction=sideways since_sequence=0",
            &mut events,
        );
        assert_eq!(bad_direction, "ERROR bad-request");

        // Duplicate keys.
        let dup = server.handle_request(
            "poll-signaling correct-token test-client session_id=session-1 session_id=session-2 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );
        assert_eq!(dup, "ERROR bad-request");
    }

    #[test]
    fn foreground_server_signaling_rejects_submit_for_unknown_session_and_leaves_no_trace() {
        // Regression test for the queue-poison vector. Predictable monotonic
        // session ids let an attacker submit envelopes for a session that
        // does not exist yet; without strict ownership checks, the legitimate
        // owner who later creates the session would poll a pre-poisoned queue.
        let server = ForegroundControlServer::new(ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            two_client_server_config(),
        ));
        let mut events = InMemoryEventSink::default();

        // client-A creates session-1.
        let creation = server.handle_request(
            "create-session correct-token client-1 terminal 1280 720",
            &mut events,
        );
        assert!(creation.starts_with("OK create-session"));

        // client-B tries to stage signaling for session-2 *before* it exists.
        let attempted_offer = b64("attacker-offer-sdp");
        let poison = server.handle_request(
            &format!(
                "submit-sdp-offer correct-token client-2 session_id=session-2 role=offerer sdp_b64={attempted_offer}"
            ),
            &mut events,
        );
        assert_eq!(
            poison, "ERROR service client client-2 is not authorized for session session-2",
            "unknown session must be rejected, not implicitly created"
        );
        let recorded_after_poison = events.events();
        assert!(
            recorded_after_poison.iter().any(|event| matches!(
                event,
                ServerEvent::RequestRejected { operation } if operation == "submit-sdp-offer"
            )),
            "queue-poison attempt must emit RequestRejected; events were: {recorded_after_poison:?}"
        );
        // The rejection must not have left a SignalingEnvelopeSubmitted audit
        // record for session-2 (which would imply the envelope reached the
        // queue).
        assert!(
            !recorded_after_poison.iter().any(|event| matches!(
                event,
                ServerEvent::SignalingEnvelopeSubmitted { session_id, .. } if session_id == "session-2"
            )),
            "no signaling submit event should have been recorded for session-2"
        );

        // client-A now legitimately creates session-2 and polls it.
        let second_creation = server.handle_request(
            "create-session correct-token client-1 terminal 1280 720",
            &mut events,
        );
        assert!(
            second_creation.starts_with("OK create-session"),
            "second create-session failed: {second_creation}"
        );

        let poll = server.handle_request(
            "poll-signaling correct-token client-1 session_id=session-2 direction=offer-to-answerer since_sequence=0",
            &mut events,
        );
        // The queue must be empty: the attacker's pre-poisoned envelope left
        // no trace.
        assert!(
            poll.contains("count=0"),
            "expected empty backlog, got: {poll}"
        );
        assert!(
            poll.contains("empty=true"),
            "expected empty backlog, got: {poll}"
        );
    }

    #[test]
    fn foreground_server_signaling_rejects_submit_when_session_backlog_is_full() {
        // A legitimate paired client owns the session, so Fix 1's auth gate
        // does not apply. Fix 3 still bounds the per-session backlog to
        // protect server memory.
        let server = signaling_server();
        let mut events = InMemoryEventSink::default();
        create_signaling_test_session(&server, &mut events);

        // Fill the per-session backlog to the cap. End-of-candidates carries
        // no payload, so this is the cheapest envelope to enqueue.
        for _ in 0..MAX_ENVELOPES_PER_SESSION {
            let response = server.handle_request(
                "signal-end-of-candidates correct-token test-client session_id=session-1 direction=offer-to-answerer",
                &mut events,
            );
            assert!(
                response.starts_with("OK signaling-submit"),
                "filling backlog failed: {response}"
            );
        }

        // The next submit must be rejected with the typed wire response and
        // a SignalingBacklogFull audit event.
        let baseline_event_count = events.events().len();
        let rejection = server.handle_request(
            "signal-end-of-candidates correct-token test-client session_id=session-1 direction=offer-to-answerer",
            &mut events,
        );
        assert_eq!(rejection, "ERROR signaling-backlog-full");
        let new_events = &events.events()[baseline_event_count..];
        assert!(
            new_events.iter().any(|event| matches!(
                event,
                ServerEvent::SignalingBacklogFull {
                    session_id,
                    paired_client,
                    current_depth,
                } if session_id == "session-1"
                    && paired_client == "test-client"
                    && *current_depth as usize == MAX_ENVELOPES_PER_SESSION
            )),
            "missing SignalingBacklogFull audit event; recorded: {new_events:?}"
        );
        // Backlog rejections are flow-control, not auth failures: there must
        // be no RequestAuthorized entry for the rejected op (it never
        // reached the queue).
        assert!(
            !new_events.iter().any(|event| matches!(
                event,
                ServerEvent::RequestAuthorized { operation } if operation == "signal-end-of-candidates"
            )),
            "rejected submit should not record RequestAuthorized; recorded: {new_events:?}"
        );

        // The legitimate consumer drains the queue. since_sequence is the
        // ack cursor: passing the high-water mark frees every slot.
        let drain = server.handle_request(
            &format!(
                "poll-signaling correct-token test-client session_id=session-1 direction=offer-to-answerer since_sequence={}",
                u64::MAX
            ),
            &mut events,
        );
        assert!(
            drain.contains("count=0"),
            "expected drained queue, got: {drain}"
        );

        // After draining, submits succeed again — i.e. the cap is on
        // backlog depth, not lifetime count.
        let after_drain = server.handle_request(
            "signal-end-of-candidates correct-token test-client session_id=session-1 direction=offer-to-answerer",
            &mut events,
        );
        assert!(
            after_drain.starts_with("OK signaling-submit"),
            "submit after drain should succeed, got: {after_drain}"
        );
    }

    /// Phase D.1.1.0 wires the WebRTC peer into the video-stream and
    /// signaling control-plane flows. The four tests below codify the
    /// audit-event ordering guarantees on default builds (the in-memory
    /// no-op peer succeeds on every call). The `webrtc-peer` feature
    /// test below covers the real `str0m` round-trip.
    fn webrtc_audit_control_plane() -> ServerControlPlane {
        ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            paired_server_config(),
        )
    }

    fn webrtc_audit_session(
        control_plane: &mut ServerControlPlane,
        auth: &ControlAuth,
    ) -> ApplicationSession {
        control_plane
            .create_session(
                auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
            )
            .expect("create session")
    }

    #[test]
    fn start_video_stream_emits_webrtc_peer_started() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);

        let mut events = InMemoryEventSink::default();
        let stream = control_plane
            .start_video_stream_with_audit(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
                &mut events,
            )
            .expect("start video stream");

        let recorded = events.events();
        assert!(
            matches!(
                recorded.first(),
                Some(ServerEvent::VideoStreamStarted { stream_id, .. }) if stream_id == &stream.id
            ),
            "expected VideoStreamStarted as first event, got: {recorded:?}"
        );
        assert!(
            matches!(
                recorded.get(1),
                Some(ServerEvent::WebRtcPeerStarted {
                    session_id,
                    stream_id,
                    role,
                    paired_client,
                }) if session_id == &session.id
                    && stream_id == &stream.id
                    && role == "answerer"
                    && paired_client == "test-client"
            ),
            "expected WebRtcPeerStarted as second event, got: {recorded:?}"
        );
    }

    #[test]
    fn stop_video_stream_emits_webrtc_peer_stopped() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);
        let stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");

        let mut events = InMemoryEventSink::default();
        control_plane
            .stop_video_stream_with_audit(
                &auth,
                StopVideoStreamRequest {
                    stream_id: stream.id.clone(),
                },
                &mut events,
            )
            .expect("stop video stream");

        let recorded = events.events();
        assert!(
            matches!(
                recorded.first(),
                Some(ServerEvent::VideoStreamStopped { stream_id, .. }) if stream_id == &stream.id
            ),
            "expected VideoStreamStopped first, got: {recorded:?}"
        );
        assert!(
            matches!(
                recorded.get(1),
                Some(ServerEvent::WebRtcPeerStopped {
                    session_id,
                    stream_id,
                    paired_client,
                }) if session_id == &session.id
                    && stream_id == &stream.id
                    && paired_client == "test-client"
            ),
            "expected WebRtcPeerStopped second, got: {recorded:?}"
        );
    }

    #[test]
    fn submit_signaling_offer_to_answerer_emits_webrtc_peer_signaling_consumed() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);

        let mut events = InMemoryEventSink::default();
        control_plane
            .submit_signaling_with_audit(
                &auth,
                SubmitSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::OfferToAnswerer,
                    envelope: SignalingEnvelope::SdpOffer {
                        sdp: "v=0\r\n".to_string(),
                        role: SdpRole::Offerer,
                    },
                },
                &mut events,
            )
            .expect("submit signaling");

        let recorded = events.events();
        assert!(
            recorded.iter().any(|event| matches!(
                event,
                ServerEvent::SignalingEnvelopeSubmitted { session_id, direction, envelope_kind, .. }
                    if session_id == &session.id
                        && direction == "offer-to-answerer"
                        && envelope_kind == "sdp-offer"
            )),
            "missing SignalingEnvelopeSubmitted event; got: {recorded:?}"
        );
        assert!(
            recorded.iter().any(|event| matches!(
                event,
                ServerEvent::WebRtcPeerSignalingConsumed { session_id, paired_client, envelope_kind }
                    if session_id == &session.id
                        && paired_client == "test-client"
                        && envelope_kind == "sdp-offer"
            )),
            "missing WebRtcPeerSignalingConsumed event; got: {recorded:?}"
        );
    }

    #[test]
    fn submit_signaling_answerer_to_offerer_does_not_consume_into_peer() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);

        let mut events = InMemoryEventSink::default();
        control_plane
            .submit_signaling_with_audit(
                &auth,
                SubmitSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::AnswererToOfferer,
                    envelope: SignalingEnvelope::EndOfCandidates,
                },
                &mut events,
            )
            .expect("submit polling-direction signaling");

        let recorded = events.events();
        assert!(
            recorded
                .iter()
                .any(|event| matches!(event, ServerEvent::SignalingEnvelopeSubmitted { .. })),
            "missing SignalingEnvelopeSubmitted event; got: {recorded:?}"
        );
        assert!(
            !recorded
                .iter()
                .any(|event| matches!(event, ServerEvent::WebRtcPeerSignalingConsumed { .. })),
            "polling-direction submit must not consume into peer; got: {recorded:?}"
        );
    }

    #[cfg(feature = "webrtc-peer")]
    #[test]
    fn feature_gated_peer_outbound_envelope_lands_in_answerer_to_offerer_queue() {
        use apprelay_core::WebRtcPeer as _;
        // Phase D.1.0 swaps the no-op in-memory peer for `Str0mWebRtcPeer`
        // when the `webrtc-peer` feature is on. With D.1.1.0 wiring, an
        // `OfferToAnswerer` SDP offer flowed through `submit_signaling`
        // must drive the peer to produce an `SdpAnswer`, which the
        // control plane re-injects into the `AnswererToOfferer` queue
        // for the client's normal `poll_signaling` flow.
        let mut control_plane = ServerControlPlane::new(
            ServerServices::for_current_platform(),
            paired_server_config(),
        );
        let auth = paired_auth();
        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");
        let _stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");

        // Drive a real, str0m-parseable SDP offer through the peer. We
        // build one by spinning up an offerer instance off to the side
        // — the str0m peer's parser is strict, so a hand-rolled `v=0`
        // is not enough.
        let mut bootstrap = apprelay_core::Str0mWebRtcPeer::new();
        bootstrap
            .start(
                "bootstrap",
                "bootstrap",
                apprelay_protocol::WebRtcPeerRole::Offerer,
            )
            .expect("bootstrap offerer");
        let bootstrap_offer = bootstrap
            .take_outbound_signaling("bootstrap")
            .into_iter()
            .next()
            .expect("bootstrap offerer produced an offer");
        let bootstrap_sdp = match bootstrap_offer {
            SignalingEnvelope::SdpOffer { sdp, .. } => sdp,
            other => panic!("expected SdpOffer, got {other:?}"),
        };

        control_plane
            .submit_signaling(
                &auth,
                SubmitSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::OfferToAnswerer,
                    envelope: SignalingEnvelope::SdpOffer {
                        sdp: bootstrap_sdp,
                        role: SdpRole::Offerer,
                    },
                },
            )
            .expect("submit signaling offer");

        // The control plane should have re-injected the peer's local
        // SDP answer into the AnswererToOfferer queue. Polling that
        // direction must surface at least one SdpAnswer envelope.
        let poll = control_plane
            .poll_signaling(
                &auth,
                PollSignalingRequest {
                    session_id: session.id,
                    direction: SignalingDirection::AnswererToOfferer,
                    since_sequence: 0,
                },
            )
            .expect("poll signaling");
        assert!(
            poll.messages
                .iter()
                .any(|message| matches!(message.envelope, SignalingEnvelope::SdpAnswer { .. })),
            "expected at least one SdpAnswer in the answerer-to-offerer queue, got: {poll:?}"
        );
    }

    #[test]
    fn close_session_cascades_webrtc_peer_stopped_for_active_streams() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);
        let stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");

        let mut events = InMemoryEventSink::default();
        let closed = control_plane
            .close_session_with_audit(&auth, &session.id, &mut events)
            .expect("close session");
        assert_eq!(closed.id, session.id);

        let recorded = events.events();
        assert!(
            recorded.iter().any(|event| matches!(
                event,
                ServerEvent::WebRtcPeerStopped {
                    session_id,
                    stream_id,
                    paired_client,
                } if session_id == &session.id
                    && stream_id == &stream.id
                    && paired_client == "test-client"
            )),
            "expected WebRtcPeerStopped cascade event, got: {recorded:?}"
        );
    }

    #[test]
    fn revoke_client_cascades_webrtc_peer_stopped_for_owned_streams() {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "test"),
            two_client_server_config(),
        );
        let auth_1 = ControlAuth::with_client_id("correct-token", "client-1");
        let auth_2 = ControlAuth::with_client_id("correct-token", "client-2");
        let session_1 = control_plane
            .create_session(
                &auth_1,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
            )
            .expect("create client-1 session");
        let session_2 = control_plane
            .create_session(
                &auth_2,
                CreateSessionRequest {
                    application_id: "browser".to_string(),
                    viewport: ViewportSize::new(1024, 768),
                },
            )
            .expect("create client-2 session");
        let stream_1 = control_plane
            .start_video_stream(
                &auth_1,
                StartVideoStreamRequest {
                    session_id: session_1.id.clone(),
                },
            )
            .expect("start client-1 stream");
        let stream_2 = control_plane
            .start_video_stream(
                &auth_2,
                StartVideoStreamRequest {
                    session_id: session_2.id.clone(),
                },
            )
            .expect("start client-2 stream");

        let mut events = InMemoryEventSink::default();
        let (revoked, closed_sessions) = control_plane
            .locally_revoke_client_with_audit(
                RevokeClientRequest {
                    client_id: "client-1".to_string(),
                },
                &mut events,
            )
            .expect("revoke client-1 with audit");
        assert_eq!(revoked.id, "client-1");
        assert!(closed_sessions
            .iter()
            .any(|session| session.id == session_1.id));

        let recorded = events.events();
        assert!(
            recorded.iter().any(|event| matches!(
                event,
                ServerEvent::WebRtcPeerStopped {
                    session_id,
                    stream_id,
                    paired_client,
                } if session_id == &session_1.id
                    && stream_id == &stream_1.id
                    && paired_client == "client-1"
            )),
            "expected WebRtcPeerStopped for client-1 stream, got: {recorded:?}"
        );
        assert!(
            !recorded.iter().any(|event| matches!(
                event,
                ServerEvent::WebRtcPeerStopped { stream_id, .. }
                    if stream_id == &stream_2.id
            )),
            "client-2 stream must NOT be stopped by client-1 revoke, got: {recorded:?}"
        );
    }

    #[test]
    fn poll_signaling_pumps_new_encoded_frame_into_peer() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);
        let stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");
        let sequence = control_plane
            .advance_encoded_frame_for_test(&stream.id)
            .expect("advance encoded frame");

        let mut events = InMemoryEventSink::default();
        control_plane
            .poll_signaling_with_audit(
                &auth,
                PollSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::AnswererToOfferer,
                    since_sequence: 0,
                },
                &mut events,
            )
            .expect("poll signaling with audit");

        let recorded = events.events();
        let outbound: Vec<_> = recorded
            .iter()
            .filter_map(|event| match event {
                ServerEvent::WebRtcPeerOutboundFrame {
                    session_id,
                    stream_id,
                    paired_client,
                    sequence,
                    keyframe,
                    ..
                } if session_id == &session.id
                    && stream_id == &stream.id
                    && paired_client == "test-client" =>
                {
                    Some((*sequence, *keyframe))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            outbound,
            vec![(sequence, true)],
            "expected exactly one WebRtcPeerOutboundFrame event with the encoder's sequence, got: {recorded:?}"
        );
    }

    #[test]
    fn poll_signaling_pump_dedups_unchanged_frame_sequence() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);
        let stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");
        control_plane
            .advance_encoded_frame_for_test(&stream.id)
            .expect("advance encoded frame");

        let mut events = InMemoryEventSink::default();
        for _ in 0..2 {
            control_plane
                .poll_signaling_with_audit(
                    &auth,
                    PollSignalingRequest {
                        session_id: session.id.clone(),
                        direction: SignalingDirection::AnswererToOfferer,
                        since_sequence: 0,
                    },
                    &mut events,
                )
                .expect("poll signaling with audit");
        }

        let outbound_count = events
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ServerEvent::WebRtcPeerOutboundFrame { stream_id, .. }
                        if stream_id == &stream.id
                )
            })
            .count();
        assert_eq!(
            outbound_count,
            1,
            "pump must dedup unchanged sequence; got events: {:?}",
            events.events()
        );
    }

    #[test]
    fn poll_signaling_pump_emits_again_when_sequence_advances() {
        let mut control_plane = webrtc_audit_control_plane();
        let auth = paired_auth();
        let session = webrtc_audit_session(&mut control_plane, &auth);
        let stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");
        let first_sequence = control_plane
            .advance_encoded_frame_for_test(&stream.id)
            .expect("advance encoded frame");

        let mut events = InMemoryEventSink::default();
        control_plane
            .poll_signaling_with_audit(
                &auth,
                PollSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::AnswererToOfferer,
                    since_sequence: 0,
                },
                &mut events,
            )
            .expect("first poll");

        let second_sequence = control_plane
            .advance_encoded_frame_for_test(&stream.id)
            .expect("advance encoded frame again");
        assert!(
            second_sequence > first_sequence,
            "encoder must advance sequence: first={first_sequence}, second={second_sequence}"
        );

        control_plane
            .poll_signaling_with_audit(
                &auth,
                PollSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::AnswererToOfferer,
                    since_sequence: 0,
                },
                &mut events,
            )
            .expect("second poll");

        let outbound_sequences: Vec<u64> = events
            .events()
            .iter()
            .filter_map(|event| match event {
                ServerEvent::WebRtcPeerOutboundFrame {
                    stream_id,
                    sequence,
                    ..
                } if stream_id == &stream.id => Some(*sequence),
                _ => None,
            })
            .collect();
        assert_eq!(
            outbound_sequences,
            vec![first_sequence, second_sequence],
            "expected two outbound-frame events with distinct sequences, got: {:?}",
            events.events()
        );
    }

    #[cfg(feature = "webrtc-peer")]
    #[test]
    fn poll_signaling_pump_silently_skips_when_str0m_negotiation_pending() {
        // With the `webrtc-peer` feature on, the server-side peer is
        // an `Str0mWebRtcPeer` Answerer. Without a client SDP offer,
        // its `push_encoded_frame` returns a typed
        // `ServiceUnavailable("negotiation pending: ...")` error. The
        // pump must silently skip without emitting either an outbound
        // or a rejected audit event so audit logs aren't flooded with
        // expected-idle-state noise.
        let mut control_plane = ServerControlPlane::new(
            ServerServices::for_current_platform(),
            paired_server_config(),
        );
        let auth = paired_auth();
        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");
        let stream = control_plane
            .start_video_stream(
                &auth,
                StartVideoStreamRequest {
                    session_id: session.id.clone(),
                },
            )
            .expect("start video stream");
        control_plane
            .advance_encoded_frame_for_test(&stream.id)
            .expect("advance encoded frame");

        let mut events = InMemoryEventSink::default();
        control_plane
            .poll_signaling_with_audit(
                &auth,
                PollSignalingRequest {
                    session_id: session.id.clone(),
                    direction: SignalingDirection::AnswererToOfferer,
                    since_sequence: 0,
                },
                &mut events,
            )
            .expect("poll signaling with audit");

        let recorded = events.events();
        assert!(
            !recorded
                .iter()
                .any(|event| matches!(event, ServerEvent::WebRtcPeerOutboundFrame { .. })),
            "negotiation-pending peer must not produce outbound-frame events; got: {recorded:?}"
        );
        assert!(
            !recorded
                .iter()
                .any(|event| matches!(event, ServerEvent::WebRtcPeerRejected { .. })),
            "negotiation-pending error must be silently skipped, not surfaced as a rejection; got: {recorded:?}"
        );
    }
}
