#[cfg(feature = "pipewire-capture")]
use apprelay_core::AudioBackendNativeReadiness;
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
                    std::env::var("APPRELAY_PIPEWIRE_CAPTURE_COMMAND")
                        .unwrap_or_else(|_| "pw-record".to_string()),
                    std::env::var("APPRELAY_PIPEWIRE_CAPTURE_TARGET").ok(),
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
fn pipewire_capture_enabled_from_env() -> bool {
    matches!(
        std::env::var("APPRELAY_PIPEWIRE_CAPTURE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}
