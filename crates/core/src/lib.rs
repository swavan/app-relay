//! Core service contracts for Swavan AppRelay.

use swavan_protocol::{
    ApplicationSummary, Feature, HealthStatus, Platform, PlatformCapability, SwavanError,
};

pub trait HealthService {
    fn status(&self) -> HealthStatus;
}

pub trait CapabilityService {
    fn platform_capabilities(&self) -> Vec<PlatformCapability>;
}

pub trait ApplicationDiscovery {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError>;
}

#[derive(Clone, Debug)]
pub struct StaticHealthService {
    service: String,
    version: String,
}

impl StaticHealthService {
    pub fn new(service: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            version: version.into(),
        }
    }
}

impl HealthService for StaticHealthService {
    fn status(&self) -> HealthStatus {
        HealthStatus::healthy(self.service.clone(), self.version.clone())
    }
}

#[derive(Clone, Debug)]
pub struct DefaultCapabilityService {
    platform: Platform,
}

impl DefaultCapabilityService {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }
}

impl CapabilityService for DefaultCapabilityService {
    fn platform_capabilities(&self) -> Vec<PlatformCapability> {
        let unsupported_reason = "feature planned but not implemented in Phase 1";

        vec![
            PlatformCapability::unsupported(
                self.platform,
                Feature::AppDiscovery,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::WindowResize,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::WindowVideoStream,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::SystemAudioStream,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::ClientMicrophoneInput,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(
                self.platform,
                Feature::KeyboardInput,
                unsupported_reason,
            ),
            PlatformCapability::unsupported(self.platform, Feature::MouseInput, unsupported_reason),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct UnsupportedApplicationDiscovery {
    platform: Platform,
}

impl UnsupportedApplicationDiscovery {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }
}

impl ApplicationDiscovery for UnsupportedApplicationDiscovery {
    fn available_applications(&self) -> Result<Vec<ApplicationSummary>, SwavanError> {
        Err(SwavanError::unsupported(
            self.platform,
            Feature::AppDiscovery,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_health_service_returns_configured_status() {
        let service = StaticHealthService::new("swavan-server", "0.1.0");

        assert_eq!(
            service.status(),
            HealthStatus::healthy("swavan-server", "0.1.0")
        );
    }

    #[test]
    fn default_capabilities_are_explicitly_unsupported() {
        let service = DefaultCapabilityService::new(Platform::Macos);
        let capabilities = service.platform_capabilities();

        assert_eq!(capabilities.len(), 7);
        assert!(capabilities.iter().all(|capability| !capability.supported));
        assert!(capabilities
            .iter()
            .all(|capability| capability.platform == Platform::Macos));
    }

    #[test]
    fn unsupported_application_discovery_returns_typed_error() {
        let discovery = UnsupportedApplicationDiscovery::new(Platform::Windows);

        assert_eq!(
            discovery.available_applications(),
            Err(SwavanError::unsupported(
                Platform::Windows,
                Feature::AppDiscovery
            ))
        );
    }
}
