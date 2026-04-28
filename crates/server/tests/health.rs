use swavan_protocol::{HealthStatus, Platform};
use swavan_server::ServerServices;

#[test]
fn server_health_contract_is_stable() {
    let services = ServerServices::new(Platform::Linux, "integration-test");

    assert_eq!(
        services.health(),
        HealthStatus::healthy("swavan-server", "integration-test")
    );
}
