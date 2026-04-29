use swavan_core::ServerConfig;
use swavan_protocol::{
    ControlAuth, ControlError, CreateSessionRequest, Feature, Platform, ResizeSessionRequest,
    ServerVersion, SessionState, StartVideoStreamRequest, StopVideoStreamRequest, SwavanError,
    VideoStreamState, ViewportSize,
};
use swavan_server::{ServerControlPlane, ServerServices};

#[test]
fn control_plane_rejects_unauthorized_requests() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
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
        ServerServices::new(Platform::Macos, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.version(&ControlAuth::new("correct-token")),
        Ok(ServerVersion::new(
            "swavan-server",
            "integration-test",
            Platform::Macos
        ))
    );
}

#[test]
fn control_plane_reports_linux_app_discovery_capability() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
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
fn control_plane_reports_macos_app_discovery_capability() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Macos, "integration-test"),
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
        ServerServices::new(Platform::Windows, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.available_applications(&ControlAuth::new("correct-token")),
        Err(ControlError::Service(SwavanError::unsupported(
            Platform::Windows,
            Feature::AppDiscovery
        )))
    );
}

#[test]
fn control_plane_heartbeat_supports_reconnect_checks() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
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
        ServerServices::new(Platform::Linux, "integration-test"),
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
fn control_plane_manages_video_stream_lifecycle() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
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
}
