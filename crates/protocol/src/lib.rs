//! Shared protocol types for Swavan AppRelay.
//!
//! Phase 1 intentionally keeps these types transport-neutral. The server can
//! expose them over SSH-tunneled HTTP, WebSocket, gRPC, or another control
//! protocol without changing the domain model.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthStatus {
    pub service: String,
    pub healthy: bool,
    pub version: String,
}

impl HealthStatus {
    pub fn healthy(service: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            healthy: true,
            version: version.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Android,
    Ios,
    Linux,
    Macos,
    Windows,
    Unknown,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Feature {
    AppDiscovery,
    WindowResize,
    WindowVideoStream,
    SystemAudioStream,
    ClientMicrophoneInput,
    KeyboardInput,
    MouseInput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformCapability {
    pub platform: Platform,
    pub feature: Feature,
    pub supported: bool,
    pub reason: Option<String>,
}

impl PlatformCapability {
    pub fn supported(platform: Platform, feature: Feature) -> Self {
        Self {
            platform,
            feature,
            supported: true,
            reason: None,
        }
    }

    pub fn unsupported(platform: Platform, feature: Feature, reason: impl Into<String>) -> Self {
        Self {
            platform,
            feature,
            supported: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationSummary {
    pub id: String,
    pub name: String,
    pub icon: Option<AppIcon>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppIcon {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwavanError {
    UnsupportedPlatform {
        platform: Platform,
        feature: Feature,
    },
    ServiceUnavailable(String),
}

impl SwavanError {
    pub fn unsupported(platform: Platform, feature: Feature) -> Self {
        Self::UnsupportedPlatform { platform, feature }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_marks_service_healthy() {
        let status = HealthStatus::healthy("server", "0.1.0");

        assert_eq!(status.service, "server");
        assert!(status.healthy);
        assert_eq!(status.version, "0.1.0");
    }

    #[test]
    fn unsupported_capability_keeps_reason() {
        let capability = PlatformCapability::unsupported(
            Platform::Linux,
            Feature::WindowVideoStream,
            "capture backend not implemented",
        );

        assert!(!capability.supported);
        assert_eq!(
            capability.reason.as_deref(),
            Some("capture backend not implemented")
        );
    }
}
