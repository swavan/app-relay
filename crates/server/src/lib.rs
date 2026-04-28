//! Server composition for Swavan AppRelay.

use swavan_core::{
    ApplicationDiscovery, CapabilityService, DefaultCapabilityService, HealthService,
    StaticHealthService, UnsupportedApplicationDiscovery,
};
use swavan_protocol::{
    ApplicationSummary, HealthStatus, Platform, PlatformCapability, SwavanError,
};

pub struct ServerServices {
    health_service: StaticHealthService,
    capability_service: DefaultCapabilityService,
    application_discovery: UnsupportedApplicationDiscovery,
}

impl ServerServices {
    pub fn new(platform: Platform, version: impl Into<String>) -> Self {
        Self {
            health_service: StaticHealthService::new("swavan-server", version),
            capability_service: DefaultCapabilityService::new(platform),
            application_discovery: UnsupportedApplicationDiscovery::new(platform),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_services_report_health() {
        let services = ServerServices::new(Platform::Linux, "test");

        assert_eq!(
            services.health(),
            HealthStatus::healthy("swavan-server", "test")
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
}
