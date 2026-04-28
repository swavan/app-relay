use swavan_core::ServerConfig;
use swavan_protocol::{ControlAuth, ControlError, Feature, Platform, ServerVersion, SwavanError};
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
