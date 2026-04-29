use apprelay_protocol::{HealthStatus, Platform};
use apprelay_server::ServerServices;

#[test]
fn server_health_contract_is_stable() {
    let services = ServerServices::new(Platform::Linux, "integration-test");

    assert_eq!(
        services.health(),
        HealthStatus::healthy("apprelay-server", "integration-test")
    );
}
