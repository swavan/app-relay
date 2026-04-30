#[cfg(feature = "pipewire-capture")]
use apprelay_core::AudioBackendNativeReadiness;
#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
use apprelay_core::PipeWireCaptureCommandConfig;
use apprelay_core::{AudioBackendService, AudioStreamService, InMemoryAudioStreamService};
use apprelay_protocol::{
    AppRelayError, ApplicationSession, AudioStreamSession, Platform, StartAudioStreamRequest,
    StopAudioStreamRequest, UpdateAudioStreamRequest,
};

#[derive(Debug)]
pub struct AudioStreamControl {
    stream_service: InMemoryAudioStreamService,
}

impl AudioStreamControl {
    pub fn new() -> Self {
        Self {
            stream_service: InMemoryAudioStreamService::default(),
        }
    }

    pub fn for_platform(platform: Platform) -> Self {
        Self {
            stream_service: InMemoryAudioStreamService::new(audio_backend_for_platform(platform)),
        }
    }

    pub fn start(
        &mut self,
        request: StartAudioStreamRequest,
        sessions: &[ApplicationSession],
    ) -> Result<AudioStreamSession, AppRelayError> {
        let session = sessions
            .iter()
            .find(|session| session.id == request.session_id)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("session {} was not found", request.session_id))
            })?;

        self.stream_service.start_stream(request, session)
    }

    pub fn stop(
        &mut self,
        request: StopAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.stream_service.stop_stream(request)
    }

    pub fn update(
        &mut self,
        request: UpdateAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.stream_service.update_stream(request)
    }

    pub fn record_session_closed(&mut self, session_id: &str) {
        self.stream_service.record_session_closed(session_id);
    }

    pub fn status(&self, stream_id: &str) -> Result<AudioStreamSession, AppRelayError> {
        self.stream_service.stream_status(stream_id)
    }
}

impl Default for AudioStreamControl {
    fn default() -> Self {
        Self::new()
    }
}

fn audio_backend_for_platform(platform: Platform) -> AudioBackendService {
    #[cfg(feature = "pipewire-capture")]
    if platform == Platform::Linux {
        #[cfg(all(not(test), target_os = "linux"))]
        if pipewire_capture_enabled_from_env() {
            return AudioBackendService::for_platform_with_native_readiness(
                platform,
                AudioBackendNativeReadiness::with_linux_pipewire_command_capture(
                    pipewire_capture_command_config_from_env(),
                ),
            );
        }

        return AudioBackendService::for_platform_with_native_readiness(
            platform,
            AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
        );
    }

    AudioBackendService::for_platform(platform)
}

#[cfg(all(feature = "pipewire-capture", not(test), target_os = "linux"))]
fn pipewire_capture_command_config_from_env() -> PipeWireCaptureCommandConfig {
    let mut config = PipeWireCaptureCommandConfig::new(
        pipewire_capture_command_from_env_value(
            std::env::var("APPRELAY_PIPEWIRE_CAPTURE_COMMAND")
                .ok()
                .as_deref(),
        ),
        pipewire_capture_target_from_env_value(
            std::env::var("APPRELAY_PIPEWIRE_CAPTURE_TARGET")
                .ok()
                .as_deref(),
        ),
    );
    config.rate = pipewire_capture_rate_from_env_value(
        std::env::var("APPRELAY_PIPEWIRE_CAPTURE_RATE")
            .ok()
            .as_deref(),
    );
    config.channels = pipewire_capture_channels_from_env_value(
        std::env::var("APPRELAY_PIPEWIRE_CAPTURE_CHANNELS")
            .ok()
            .as_deref(),
    );
    config.format = pipewire_capture_format_from_env_value(
        std::env::var("APPRELAY_PIPEWIRE_CAPTURE_FORMAT")
            .ok()
            .as_deref(),
    );
    config
}

#[cfg(all(feature = "pipewire-capture", not(test), target_os = "linux"))]
fn pipewire_capture_enabled_from_env() -> bool {
    matches!(
        std::env::var("APPRELAY_PIPEWIRE_CAPTURE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
fn pipewire_capture_command_from_env_value(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("pw-record")
        .to_string()
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
fn pipewire_capture_rate_from_env_value(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rate| *rate > 0)
        .unwrap_or(PipeWireCaptureCommandConfig::DEFAULT_RATE)
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
fn pipewire_capture_channels_from_env_value(value: Option<&str>) -> u16 {
    value
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|channels| *channels > 0)
        .unwrap_or(PipeWireCaptureCommandConfig::DEFAULT_CHANNELS)
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
fn pipewire_capture_format_from_env_value(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(PipeWireCaptureCommandConfig::DEFAULT_FORMAT)
        .to_string()
}

#[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
fn pipewire_capture_target_from_env_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
    use super::{
        pipewire_capture_channels_from_env_value, pipewire_capture_command_from_env_value,
        pipewire_capture_format_from_env_value, pipewire_capture_rate_from_env_value,
        pipewire_capture_target_from_env_value,
    };

    #[cfg(all(feature = "pipewire-capture", target_os = "linux"))]
    #[test]
    fn pipewire_capture_env_parameter_parsing_falls_back_conservatively() {
        assert_eq!(
            pipewire_capture_command_from_env_value(Some("pw-cat")),
            "pw-cat"
        );
        assert_eq!(
            pipewire_capture_command_from_env_value(Some("  pw-cat  ")),
            "pw-cat"
        );
        assert_eq!(
            pipewire_capture_command_from_env_value(Some("")),
            "pw-record"
        );
        assert_eq!(
            pipewire_capture_command_from_env_value(Some("   ")),
            "pw-record"
        );
        assert_eq!(pipewire_capture_command_from_env_value(None), "pw-record");

        assert_eq!(pipewire_capture_rate_from_env_value(Some("44100")), 44_100);
        assert_eq!(pipewire_capture_rate_from_env_value(Some("0")), 48_000);
        assert_eq!(pipewire_capture_rate_from_env_value(Some("bad")), 48_000);
        assert_eq!(pipewire_capture_rate_from_env_value(None), 48_000);

        assert_eq!(pipewire_capture_channels_from_env_value(Some("1")), 1);
        assert_eq!(pipewire_capture_channels_from_env_value(Some("0")), 2);
        assert_eq!(pipewire_capture_channels_from_env_value(Some("-1")), 2);
        assert_eq!(pipewire_capture_channels_from_env_value(None), 2);

        assert_eq!(pipewire_capture_format_from_env_value(Some("f32")), "f32");
        assert_eq!(
            pipewire_capture_format_from_env_value(Some("  f32  ")),
            "f32"
        );
        assert_eq!(pipewire_capture_format_from_env_value(Some("")), "s16");
        assert_eq!(pipewire_capture_format_from_env_value(Some("   ")), "s16");
        assert_eq!(pipewire_capture_format_from_env_value(None), "s16");

        assert_eq!(
            pipewire_capture_target_from_env_value(Some("bluez_output.test.monitor")),
            Some("bluez_output.test.monitor".to_string())
        );
        assert_eq!(
            pipewire_capture_target_from_env_value(Some("  bluez_output.test.monitor  ")),
            Some("bluez_output.test.monitor".to_string())
        );
        assert_eq!(pipewire_capture_target_from_env_value(Some("")), None);
        assert_eq!(pipewire_capture_target_from_env_value(Some("   ")), None);
        assert_eq!(pipewire_capture_target_from_env_value(None), None);
    }
}
