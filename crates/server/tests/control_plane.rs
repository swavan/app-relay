use apprelay_core::ServerConfig;
use apprelay_protocol::{
    AppRelayError, AudioBackendFailureKind, AudioBackendKind, AudioBackendLeg,
    AudioBackendReadiness, AudioStreamSession, AudioStreamState, ButtonAction, ClientPoint,
    ControlAuth, ControlError, CreateSessionRequest, Feature, ForwardInputRequest,
    InputDeliveryStatus, InputEvent, LaunchIntentStatus, MappedInputEvent, MicrophoneMode,
    NegotiateVideoStreamRequest, Platform, PointerButton, ReconnectVideoStreamRequest,
    ResizeSessionRequest, ServerPoint, ServerVersion, SessionState, StartAudioStreamRequest,
    StartVideoStreamRequest, StopAudioStreamRequest, StopVideoStreamRequest,
    UpdateAudioStreamRequest, VideoEncodingPipelineState, VideoResolutionAdaptationReason,
    VideoStreamFailureKind, VideoStreamNegotiationState, VideoStreamRecoveryAction,
    VideoStreamState, ViewportSize, WebRtcIceCandidate, WebRtcSdpType, WebRtcSessionDescription,
    WindowSelectionMethod,
};
use apprelay_server::{ServerControlPlane, ServerServices};

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
fn pipewire_capture_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static PIPEWIRE_CAPTURE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    PIPEWIRE_CAPTURE_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
struct PipeWireCaptureEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    vars: Vec<(&'static str, Option<String>)>,
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
impl PipeWireCaptureEnvGuard {
    fn set(vars: &[(&'static str, String)]) -> Self {
        Self::set_and_clear(vars, &[])
    }

    fn set_and_clear(vars: &[(&'static str, String)], clear: &[&'static str]) -> Self {
        let lock = pipewire_capture_env_lock();
        let mut keys = vars.iter().map(|(key, _)| *key).collect::<Vec<_>>();
        keys.extend(clear.iter().copied());
        keys.sort_unstable();
        keys.dedup();
        let guard = Self {
            _lock: lock,
            vars: keys
                .into_iter()
                .map(|key| (key, std::env::var(key).ok()))
                .collect(),
        };
        for key in clear {
            std::env::remove_var(key);
        }
        for (key, value) in vars {
            std::env::set_var(key, value);
        }
        guard
    }
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
impl Drop for PipeWireCaptureEnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.vars {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
struct PipeWireTempFiles {
    paths: Vec<std::path::PathBuf>,
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
impl Drop for PipeWireTempFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
struct AudioStreamCleanup<'a> {
    control_plane: &'a mut ServerControlPlane,
    auth: ControlAuth,
    stream_id: String,
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
impl Drop for AudioStreamCleanup<'_> {
    fn drop(&mut self) {
        let _ = self.control_plane.stop_audio_stream(
            &self.auth,
            StopAudioStreamRequest {
                stream_id: self.stream_id.clone(),
            },
        );
    }
}

fn server_services_for_platform(platform: Platform, version: impl Into<String>) -> ServerServices {
    #[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
    let _env_guard = pipewire_capture_env_lock();

    ServerServices::new(platform, version)
}

#[cfg(unix)]
fn unique_test_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
}

#[cfg(unix)]
fn write_executable_script(path: &std::path::Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, contents).expect("write executable script");
    let mut permissions = std::fs::metadata(path)
        .expect("read executable script metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("mark executable script");
}

#[cfg(unix)]
fn wait_for_path(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    panic!("timed out waiting for {}", path.display());
}

fn start_audio_stream_for_platform(platform: Platform) -> AudioStreamSession {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(platform, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");

    control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id,
                microphone: MicrophoneMode::Enabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: None,
                input_device_id: None,
            },
        )
        .expect("start audio stream")
}

#[test]
fn control_plane_rejects_unauthorized_requests() {
    let control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.version(&ControlAuth::new("wrong-token")),
        Err(ControlError::Unauthorized)
    );
}

#[test]
fn control_plane_exposes_version_for_authorized_requests() {
    let control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Macos, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.version(&ControlAuth::new("correct-token")),
        Ok(ServerVersion::new(
            "apprelay-server",
            "integration-test",
            Platform::Macos
        ))
    );
}

#[test]
fn control_plane_reports_linux_app_discovery_capability() {
    let control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let capabilities = control_plane
        .capabilities(&ControlAuth::new("correct-token"))
        .expect("authorized capabilities response");

    assert!(capabilities
        .iter()
        .any(|capability| capability.feature == Feature::AppDiscovery && capability.supported));
}

#[test]
fn control_plane_reports_desktop_audio_capabilities() {
    for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
        let control_plane = ServerControlPlane::new(
            server_services_for_platform(platform, "integration-test"),
            ServerConfig::local("correct-token"),
        );
        let capabilities = control_plane
            .capabilities(&ControlAuth::new("correct-token"))
            .expect("authorized capabilities response");

        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::SystemAudioStream && capability.supported
        }));
        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::ClientMicrophoneInput && capability.supported
        }));
    }
}

#[test]
fn control_plane_reports_macos_app_discovery_capability() {
    let control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Macos, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let capabilities = control_plane
        .capabilities(&ControlAuth::new("correct-token"))
        .expect("authorized capabilities response");

    assert!(capabilities
        .iter()
        .any(|capability| capability.feature == Feature::AppDiscovery && capability.supported));
}

#[test]
fn control_plane_maps_unsupported_application_discovery_errors() {
    let control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Windows, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.available_applications(&ControlAuth::new("correct-token")),
        Err(ControlError::Service(AppRelayError::unsupported(
            Platform::Windows,
            Feature::AppDiscovery
        )))
    );
}

#[test]
fn control_plane_heartbeat_supports_reconnect_checks() {
    let control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");

    let first = control_plane
        .heartbeat(&auth)
        .expect("first heartbeat response");
    let second = control_plane
        .heartbeat(&auth)
        .expect("second heartbeat response");

    assert!(first.healthy);
    assert!(second.healthy);
    assert_eq!(first.sequence + 1, second.sequence);
}

#[test]
fn control_plane_manages_application_session_lifecycle() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");

    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let resized = control_plane
        .resize_session(
            &auth,
            ResizeSessionRequest {
                session_id: session.id.clone(),
                viewport: ViewportSize::new(1440, 900),
            },
        )
        .expect("resize session");
    let closed = control_plane
        .close_session(&auth, &session.id)
        .expect("close session");

    assert_eq!(resized.viewport, ViewportSize::new(1440, 900));
    assert_eq!(closed.state, SessionState::Closed);
    assert_eq!(control_plane.active_sessions(&auth), Ok(Vec::new()));
}

#[test]
#[cfg(unix)]
fn control_plane_launches_linux_desktop_entry_session() {
    let root = unique_test_dir("control-plane-linux-launch");
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

    let mut control_plane = ServerControlPlane::new(
        ServerServices::with_linux_desktop_entry_roots("integration-test", vec![applications]),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "fake".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create launched session");

    wait_for_path(&marker);
    assert_eq!(
        std::fs::read_to_string(&marker).expect("read launch marker"),
        "--label\nFake App\n"
    );
    assert_eq!(
        session.selected_window.selection_method,
        WindowSelectionMethod::LaunchIntent
    );
    assert_eq!(
        session.launch_intent.expect("launch intent").status,
        LaunchIntentStatus::Recorded
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[cfg(unix)]
fn control_plane_launches_macos_app_bundle_session() {
    let root = unique_test_dir("control-plane-macos-launch");
    let applications = root.join("Applications");
    let app_contents = applications.join("Fake.app/Contents");
    std::fs::create_dir_all(&app_contents).expect("create app bundle");
    std::fs::write(
        app_contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>dev.apprelay.fake</string>
  <key>CFBundleDisplayName</key>
  <string>Fake Mac App</string>
</dict>
</plist>
"#,
    )
    .expect("write info plist");
    let marker = root.join("open-marker");
    let open_command = root.join("fake-open");
    write_executable_script(
        &open_command,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" > {}\n",
            marker.display()
        ),
    );

    let mut control_plane = ServerControlPlane::new(
        ServerServices::with_macos_application_roots_and_open_command(
            "integration-test",
            vec![applications],
            open_command,
        ),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "dev.apprelay.fake".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create launched macOS session");

    wait_for_path(&marker);
    assert_eq!(
        std::fs::read_to_string(&marker).expect("read open marker"),
        format!("-n\n{}\n", root.join("Applications/Fake.app").display())
    );
    assert_eq!(
        session.selected_window.selection_method,
        WindowSelectionMethod::LaunchIntent
    );
    assert_eq!(
        session.launch_intent.expect("launch intent").status,
        LaunchIntentStatus::Recorded
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn control_plane_authorizes_and_forwards_input_to_session() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1920, 1080),
            },
        )
        .expect("create session");

    let focused = control_plane
        .forward_input(
            &auth,
            ForwardInputRequest {
                session_id: session.id.clone(),
                client_viewport: ViewportSize::new(960, 540),
                event: InputEvent::Focus,
            },
        )
        .expect("focus input");
    let clicked = control_plane
        .forward_input(
            &auth,
            ForwardInputRequest {
                session_id: session.id,
                client_viewport: ViewportSize::new(960, 540),
                event: InputEvent::PointerButton {
                    position: ClientPoint::new(480.0, 270.0),
                    button: PointerButton::Primary,
                    action: ButtonAction::Press,
                },
            },
        )
        .expect("forward input");

    assert_eq!(focused.status, InputDeliveryStatus::Focused);
    assert_eq!(
        clicked.mapped_event,
        MappedInputEvent::PointerButton {
            position: ServerPoint::new(960, 540),
            button: PointerButton::Primary,
            action: ButtonAction::Press,
        }
    );
    assert_eq!(clicked.status, InputDeliveryStatus::Delivered);
}

#[test]
fn control_plane_rejects_unauthorized_input_requests() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.forward_input(
            &ControlAuth::new("wrong-token"),
            ForwardInputRequest {
                session_id: "session-1".to_string(),
                client_viewport: ViewportSize::new(1280, 720),
                event: InputEvent::Focus,
            },
        ),
        Err(ControlError::Unauthorized)
    );
}

#[test]
fn control_plane_rejects_input_for_unknown_session() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.forward_input(
            &ControlAuth::new("correct-token"),
            ForwardInputRequest {
                session_id: "session-unknown".to_string(),
                client_viewport: ViewportSize::new(1280, 720),
                event: InputEvent::Focus,
            },
        ),
        Err(ControlError::Service(AppRelayError::PermissionDenied(
            "input is not authorized for session session-unknown".to_string()
        )))
    );
}

#[test]
fn control_plane_manages_video_stream_lifecycle() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
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

    assert_eq!(stream.session_id, session.id);
    assert_eq!(stream.selected_window_id, session.selected_window.id);
    assert_eq!(stream.state, VideoStreamState::Starting);
    assert_eq!(
        stream.encoding.state,
        VideoEncodingPipelineState::Configured
    );
    assert_eq!(
        stream.encoding.contract.target.resolution,
        ViewportSize::new(1280, 720)
    );
    assert_eq!(
        stream.encoding.contract.adaptation.current_target,
        ViewportSize::new(1280, 720)
    );
    assert_eq!(
        stream.encoding.contract.adaptation.reason,
        VideoResolutionAdaptationReason::MatchesViewport
    );
    assert_eq!(
        control_plane.video_stream_status(&auth, &stream.id),
        Ok(stream.clone())
    );

    let stopped = control_plane
        .stop_video_stream(
            &auth,
            StopVideoStreamRequest {
                stream_id: stream.id,
            },
        )
        .expect("stop video stream");

    assert_eq!(stopped.state, VideoStreamState::Stopped);
    assert_eq!(stopped.encoding.state, VideoEncodingPipelineState::Drained);
}

#[test]
fn control_plane_negotiates_video_stream() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
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

    let negotiated = control_plane
        .negotiate_video_stream(
            &auth,
            NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: vec![WebRtcIceCandidate {
                    candidate: "candidate:client stream-1 typ host".to_string(),
                    sdp_mid: Some("video".to_string()),
                    sdp_m_line_index: Some(0),
                }],
            },
        )
        .expect("negotiate video stream");

    assert_eq!(negotiated.state, VideoStreamState::Streaming);
    assert_eq!(
        negotiated.encoding.state,
        VideoEncodingPipelineState::Encoding
    );
    assert_eq!(negotiated.encoding.output.frames_encoded, 1);
    assert_eq!(negotiated.encoding.output.keyframes_encoded, 1);
    assert_eq!(negotiated.stats.frames_encoded, 1);
    assert_eq!(negotiated.stats.bitrate_kbps, 2764);
    assert_eq!(
        negotiated.signaling.negotiation_state,
        VideoStreamNegotiationState::Negotiated
    );
    assert_eq!(
        negotiated.signaling.answer,
        Some(WebRtcSessionDescription {
            sdp_type: WebRtcSdpType::Answer,
            sdp: "client-answer".to_string(),
        })
    );
    assert_eq!(negotiated.signaling.ice_candidates.len(), 2);
}

#[test]
fn control_plane_reconnects_and_resizes_video_stream() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
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

    let reconnected = control_plane
        .reconnect_video_stream(
            &auth,
            ReconnectVideoStreamRequest {
                stream_id: stream.id.clone(),
            },
        )
        .expect("reconnect video stream");
    assert_eq!(reconnected.stats.reconnect_attempts, 1);

    control_plane
        .resize_session(
            &auth,
            ResizeSessionRequest {
                session_id: session.id,
                viewport: ViewportSize::new(2560, 1440),
            },
        )
        .expect("resize session");

    let status = control_plane
        .video_stream_status(&auth, &stream.id)
        .expect("stream status");
    assert_eq!(status.viewport, ViewportSize::new(2560, 1440));
    assert_eq!(
        status.encoding.contract.target.resolution,
        ViewportSize::new(1920, 1080)
    );
    assert_eq!(status.encoding.contract.target.target_bitrate_kbps, 6220);
    assert_eq!(
        status.encoding.contract.adaptation.requested_viewport,
        ViewportSize::new(2560, 1440)
    );
    assert_eq!(
        status.encoding.contract.adaptation.reason,
        VideoResolutionAdaptationReason::CappedToLimits
    );
    assert_eq!(
        status.health.message.as_deref(),
        Some("stream viewport updated")
    );
}

#[test]
fn control_plane_keeps_negotiated_video_encoding_coherent_after_resize() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
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
        .negotiate_video_stream(
            &auth,
            NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            },
        )
        .expect("negotiate video stream");

    control_plane
        .resize_session(
            &auth,
            ResizeSessionRequest {
                session_id: session.id,
                viewport: ViewportSize::new(2560, 1440),
            },
        )
        .expect("resize session");

    let status = control_plane
        .video_stream_status(&auth, &stream.id)
        .expect("stream status");
    assert_eq!(status.state, VideoStreamState::Streaming);
    assert_eq!(status.encoding.state, VideoEncodingPipelineState::Encoding);
    assert_eq!(status.viewport, ViewportSize::new(2560, 1440));
    assert_eq!(
        status.encoding.contract.target.resolution,
        ViewportSize::new(1920, 1080)
    );
    assert_eq!(
        status.encoding.contract.adaptation.reason,
        VideoResolutionAdaptationReason::CappedToLimits
    );
    assert_eq!(status.stats.bitrate_kbps, 6220);
}

#[test]
fn control_plane_marks_stream_failed_when_session_closes() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
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
        .close_session(&auth, &session.id)
        .expect("close session");

    let status = control_plane
        .video_stream_status(&auth, &stream.id)
        .expect("stream status");
    assert_eq!(status.state, VideoStreamState::Failed);
    assert_eq!(status.encoding.state, VideoEncodingPipelineState::Drained);
    assert_eq!(status.stats.bitrate_kbps, 0);
    assert_eq!(status.stats.latency_ms, 0);
    assert_eq!(
        status.failure.as_ref().map(|failure| &failure.kind),
        Some(&VideoStreamFailureKind::AppClosed)
    );
    assert_eq!(
        status
            .failure
            .as_ref()
            .map(|failure| &failure.recovery.action),
        Some(&VideoStreamRecoveryAction::RestartApplicationSession)
    );
    assert_eq!(
        control_plane.reconnect_video_stream(
            &auth,
            ReconnectVideoStreamRequest {
                stream_id: stream.id
            }
        ),
        Err(apprelay_protocol::ControlError::Service(
            AppRelayError::InvalidRequest(
                "stream stream-1 cannot reconnect because its application session closed"
                    .to_string()
            )
        ))
    );
}

#[test]
fn control_plane_manages_audio_stream_independently_from_video() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let video = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id.clone(),
                microphone: MicrophoneMode::Enabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: Some("speakers".to_string()),
                input_device_id: Some("mic".to_string()),
            },
        )
        .expect("start audio stream");

    assert_eq!(audio.session_id, session.id);
    assert_eq!(audio.selected_window_id, session.selected_window.id);
    assert_eq!(audio.microphone, MicrophoneMode::Enabled);
    assert!(audio.microphone_injection.requested);
    assert!(!audio.microphone_injection.active);
    assert_eq!(
        audio.microphone_injection.readiness,
        AudioBackendReadiness::PlannedNative
    );
    assert!(!audio.mute.system_audio_muted);
    assert!(audio.mute.microphone_muted);
    assert!(audio.capabilities.system_audio.supported);
    assert!(audio.capabilities.microphone_capture.supported);
    assert_eq!(audio.state, AudioStreamState::Streaming);
    assert_eq!(
        control_plane.video_stream_status(&auth, &video.id),
        Ok(video)
    );

    let stopped = control_plane
        .stop_audio_stream(
            &auth,
            StopAudioStreamRequest {
                stream_id: audio.id,
            },
        )
        .expect("stop audio stream");

    assert_eq!(stopped.state, AudioStreamState::Stopped);
}

#[test]
fn control_plane_starts_audio_stream_on_desktop_platforms() {
    for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
        let mut control_plane = ServerControlPlane::new(
            server_services_for_platform(platform, "integration-test"),
            ServerConfig::local("correct-token"),
        );
        let auth = ControlAuth::new("correct-token");
        let session = control_plane
            .create_session(
                &auth,
                CreateSessionRequest {
                    application_id: "terminal".to_string(),
                    viewport: ViewportSize::new(1280, 720),
                },
            )
            .expect("create session");

        let audio = control_plane
            .start_audio_stream(
                &auth,
                StartAudioStreamRequest {
                    session_id: session.id,
                    microphone: MicrophoneMode::Enabled,
                    system_audio_muted: false,
                    microphone_muted: true,
                    output_device_id: None,
                    input_device_id: None,
                },
            )
            .expect("start audio stream");

        assert_eq!(audio.state, AudioStreamState::Streaming);
        assert!(audio.microphone_injection.requested);
        assert!(!audio.microphone_injection.active);
        assert_eq!(
            audio.microphone_injection.readiness,
            AudioBackendReadiness::PlannedNative
        );
        assert!(audio.capabilities.system_audio.supported);
        assert!(audio.capabilities.microphone_capture.supported);
        let backend = audio.backend.as_ref().expect("audio backend contract");
        let expected_backend = match platform {
            Platform::Linux => AudioBackendKind::PipeWire,
            Platform::Macos => AudioBackendKind::CoreAudio,
            Platform::Windows => AudioBackendKind::Wasapi,
            Platform::Android | Platform::Ios | Platform::Unknown => AudioBackendKind::Unsupported,
        };
        assert_eq!(
            backend
                .statuses
                .iter()
                .map(|status| status.leg.clone())
                .collect::<Vec<_>>(),
            vec![
                AudioBackendLeg::Capture,
                AudioBackendLeg::Playback,
                AudioBackendLeg::ClientMicrophoneCapture,
                AudioBackendLeg::ServerMicrophoneInjection,
            ]
        );
        assert!(backend.statuses.iter().all(|status| {
            status.backend == expected_backend
                && !status.available
                && status.readiness == AudioBackendReadiness::PlannedNative
                && status.failure.as_ref().is_some_and(|failure| {
                    failure.kind == AudioBackendFailureKind::NativeBackendNotImplemented
                })
        }));
    }
}

#[cfg(not(feature = "pipewire-capture"))]
#[test]
fn control_plane_default_linux_audio_stream_has_no_pipewire_capture_boundary() {
    let audio = start_audio_stream_for_platform(Platform::Linux);
    let backend = audio.backend.as_ref().expect("audio backend contract");

    assert!(backend
        .notes
        .iter()
        .all(|note| !note.contains("PipeWire capture has an adapter boundary")));
    let capture = backend
        .statuses
        .iter()
        .find(|status| status.leg == AudioBackendLeg::Capture)
        .expect("capture status");
    assert!(!capture
        .failure
        .as_ref()
        .expect("capture failure")
        .message
        .contains("adapter boundary"));
}

#[cfg(feature = "pipewire-capture")]
#[test]
fn control_plane_pipewire_capture_feature_reports_linux_adapter_boundary_only() {
    let audio = start_audio_stream_for_platform(Platform::Linux);
    let backend = audio.backend.as_ref().expect("audio backend contract");

    assert_eq!(backend.readiness, AudioBackendReadiness::ControlPlaneOnly);
    assert!(backend
        .notes
        .iter()
        .any(|note| note.contains("PipeWire capture has an adapter boundary")));

    let capture = backend
        .statuses
        .iter()
        .find(|status| status.leg == AudioBackendLeg::Capture)
        .expect("capture status");
    assert!(!capture.available);
    assert!(!capture.media.available);
    assert_eq!(capture.media.packets_sent, 0);
    assert_eq!(capture.media.packets_received, 0);
    assert_eq!(capture.media.bytes_sent, 0);
    assert_eq!(capture.media.bytes_received, 0);
    assert!(capture
        .failure
        .as_ref()
        .expect("capture failure")
        .message
        .contains("PipeWire adapter boundary"));

    for planned_leg in [
        AudioBackendLeg::Playback,
        AudioBackendLeg::ClientMicrophoneCapture,
        AudioBackendLeg::ServerMicrophoneInjection,
    ] {
        let status = backend
            .statuses
            .iter()
            .find(|status| status.leg == planned_leg)
            .expect("planned leg status");
        assert!(!status.available);
        assert!(!status.media.available);
        assert!(!status
            .failure
            .as_ref()
            .expect("planned leg failure")
            .message
            .contains("adapter boundary"));
    }
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
#[test]
fn control_plane_pipewire_capture_env_overrides_pw_record_arguments() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let test_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );
    let script_path = std::env::temp_dir().join(format!("apprelay-server-pipewire-{test_id}"));
    let args_path = std::env::temp_dir().join(format!("apprelay-server-pipewire-{test_id}.txt"));
    let _temp_files = PipeWireTempFiles {
        paths: vec![script_path.clone(), args_path.clone()],
    };
    fs::write(
        &script_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\nwhile :; do printf audio-data; sleep 1; done\n",
            args_path.display()
        ),
    )
    .expect("write script");
    let mut permissions = fs::metadata(&script_path)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script_path, permissions).expect("script permissions");

    let mut control_plane = {
        let _env = PipeWireCaptureEnvGuard::set(&[
            ("APPRELAY_PIPEWIRE_CAPTURE", "1".to_string()),
            (
                "APPRELAY_PIPEWIRE_CAPTURE_COMMAND",
                script_path.to_string_lossy().to_string(),
            ),
            (
                "APPRELAY_PIPEWIRE_CAPTURE_TARGET",
                "bluez_output.test.monitor".to_string(),
            ),
            ("APPRELAY_PIPEWIRE_CAPTURE_RATE", "44100".to_string()),
            ("APPRELAY_PIPEWIRE_CAPTURE_CHANNELS", "1".to_string()),
            ("APPRELAY_PIPEWIRE_CAPTURE_FORMAT", "f32".to_string()),
        ]);
        ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "integration-test"),
            ServerConfig::local("correct-token"),
        )
    };
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id,
                microphone: MicrophoneMode::Disabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: None,
                input_device_id: None,
            },
        )
        .expect("start audio stream");
    let _audio_cleanup = AudioStreamCleanup {
        control_plane: &mut control_plane,
        auth: auth.clone(),
        stream_id: audio.id.clone(),
    };
    assert_eq!(audio.state, AudioStreamState::Streaming);

    let args = fs::read_to_string(&args_path).expect("captured command arguments");
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        vec![
            "--rate",
            "44100",
            "--channels",
            "1",
            "--format",
            "f32",
            "--target",
            "bluez_output.test.monitor",
            "-"
        ]
    );
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
#[test]
fn control_plane_pipewire_capture_uses_output_device_as_pw_record_target_fallback() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let test_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );
    let script_path = std::env::temp_dir().join(format!("apprelay-server-pipewire-{test_id}"));
    let args_path = std::env::temp_dir().join(format!("apprelay-server-pipewire-{test_id}.txt"));
    let _temp_files = PipeWireTempFiles {
        paths: vec![script_path.clone(), args_path.clone()],
    };
    fs::write(
        &script_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\nwhile :; do printf audio-data; sleep 1; done\n",
            args_path.display()
        ),
    )
    .expect("write script");
    let mut permissions = fs::metadata(&script_path)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script_path, permissions).expect("script permissions");

    let mut control_plane = {
        let _env = PipeWireCaptureEnvGuard::set(&[
            ("APPRELAY_PIPEWIRE_CAPTURE", "1".to_string()),
            (
                "APPRELAY_PIPEWIRE_CAPTURE_COMMAND",
                script_path.to_string_lossy().to_string(),
            ),
            ("APPRELAY_PIPEWIRE_CAPTURE_TARGET", String::new()),
        ]);
        ServerControlPlane::new(
            ServerServices::new(Platform::Linux, "integration-test"),
            ServerConfig::local("correct-token"),
        )
    };
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id,
                microphone: MicrophoneMode::Disabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: Some("alsa_output.fallback.monitor".to_string()),
                input_device_id: None,
            },
        )
        .expect("start audio stream");
    let _audio_cleanup = AudioStreamCleanup {
        control_plane: &mut control_plane,
        auth: auth.clone(),
        stream_id: audio.id.clone(),
    };
    assert_eq!(audio.state, AudioStreamState::Streaming);

    let args = fs::read_to_string(&args_path).expect("captured command arguments");
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        vec![
            "--rate",
            "48000",
            "--channels",
            "2",
            "--format",
            "s16",
            "--target",
            "alsa_output.fallback.monitor",
            "-"
        ]
    );
}

#[cfg(feature = "pipewire-capture")]
#[test]
fn control_plane_pipewire_capture_feature_does_not_affect_macos_or_windows() {
    for platform in [Platform::Macos, Platform::Windows] {
        let audio = start_audio_stream_for_platform(platform);
        let backend = audio.backend.as_ref().expect("audio backend contract");

        assert!(backend
            .notes
            .iter()
            .all(|note| !note.contains("PipeWire capture has an adapter boundary")));
        assert!(backend.statuses.iter().all(|status| {
            !status.available
                && status
                    .failure
                    .as_ref()
                    .is_some_and(|failure| !failure.message.contains("PipeWire adapter boundary"))
        }));
    }
}

#[test]
fn control_plane_updates_audio_mute_and_devices() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id.clone(),
                microphone: MicrophoneMode::Disabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: None,
                input_device_id: None,
            },
        )
        .expect("start audio stream");

    let updated = control_plane
        .update_audio_stream(
            &auth,
            UpdateAudioStreamRequest {
                stream_id: audio.id.clone(),
                system_audio_muted: true,
                microphone_muted: true,
                output_device_id: Some("headphones".to_string()),
                input_device_id: None,
            },
        )
        .expect("update audio stream");

    assert!(updated.mute.system_audio_muted);
    assert!(updated.mute.microphone_muted);
    assert_eq!(
        updated.devices.output_device_id.as_deref(),
        Some("headphones")
    );
    assert_eq!(
        control_plane.audio_stream_status(&auth, &audio.id),
        Ok(updated)
    );
}

#[test]
fn control_plane_stops_audio_stream_when_session_closes() {
    let mut control_plane = ServerControlPlane::new(
        server_services_for_platform(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id.clone(),
                microphone: MicrophoneMode::Disabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: None,
                input_device_id: None,
            },
        )
        .expect("start audio stream");

    control_plane
        .close_session(&auth, &session.id)
        .expect("close session");

    let status = control_plane
        .audio_stream_status(&auth, &audio.id)
        .expect("audio stream status");
    assert_eq!(status.state, AudioStreamState::Stopped);
    assert_eq!(
        status.health.message.as_deref(),
        Some("application session session-1 closed")
    );
}
