use apprelay_protocol::{
    AppRelayError, ApplicationSession, AudioBackendContract, AudioBackendKind,
    AudioBackendReadiness, AudioCapability, AudioCaptureScope, AudioDeviceSelection,
    AudioMuteState, AudioSource, AudioStreamCapabilities, AudioStreamHealth, AudioStreamSession,
    AudioStreamState, AudioStreamStats, Feature, MicrophoneMode, Platform, SessionState,
    StartAudioStreamRequest, StopAudioStreamRequest, UpdateAudioStreamRequest,
};

pub trait AudioStreamService {
    fn start_stream(
        &mut self,
        request: StartAudioStreamRequest,
        session: &ApplicationSession,
    ) -> Result<AudioStreamSession, AppRelayError>;
    fn stop_stream(
        &mut self,
        request: StopAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError>;
    fn update_stream(
        &mut self,
        request: UpdateAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError>;
    fn stream_status(&self, stream_id: &str) -> Result<AudioStreamSession, AppRelayError>;
    fn record_session_closed(&mut self, session_id: &str);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioBackendService {
    DesktopControl { platform: Platform },
    Unsupported { platform: Platform },
}

impl AudioBackendService {
    pub fn for_platform(platform: Platform) -> Self {
        match platform {
            Platform::Linux | Platform::Macos | Platform::Windows => {
                Self::DesktopControl { platform }
            }
            Platform::Android | Platform::Ios | Platform::Unknown => Self::Unsupported { platform },
        }
    }

    pub fn capabilities(&self) -> AudioStreamCapabilities {
        match self {
            Self::DesktopControl { .. } => AudioStreamCapabilities {
                system_audio: AudioCapability {
                    supported: true,
                    reason: Some("desktop audio control-plane support is available".to_string()),
                },
                microphone_capture: AudioCapability {
                    supported: true,
                    reason: Some(
                        "desktop microphone control-plane support is available".to_string(),
                    ),
                },
                microphone_injection: AudioCapability {
                    supported: false,
                    reason: Some(
                        "server-side microphone injection backend is not implemented yet"
                            .to_string(),
                    ),
                },
                echo_cancellation: AudioCapability {
                    supported: true,
                    reason: None,
                },
                device_selection: AudioCapability {
                    supported: true,
                    reason: None,
                },
            },
            Self::Unsupported { platform } => {
                let reason = format!("audio streaming is unsupported on {platform:?}");
                AudioStreamCapabilities {
                    system_audio: AudioCapability {
                        supported: false,
                        reason: Some(reason.clone()),
                    },
                    microphone_capture: AudioCapability {
                        supported: false,
                        reason: Some(reason.clone()),
                    },
                    microphone_injection: AudioCapability {
                        supported: false,
                        reason: Some(reason.clone()),
                    },
                    echo_cancellation: AudioCapability {
                        supported: false,
                        reason: Some(reason.clone()),
                    },
                    device_selection: AudioCapability {
                        supported: false,
                        reason: Some(reason),
                    },
                }
            }
        }
    }

    pub fn backend_contract(&self) -> AudioBackendContract {
        match self {
            Self::DesktopControl { platform } => {
                let native_backend = match platform {
                    Platform::Linux => AudioBackendKind::PipeWire,
                    Platform::Macos => AudioBackendKind::CoreAudio,
                    Platform::Windows => AudioBackendKind::Wasapi,
                    Platform::Android | Platform::Ios | Platform::Unknown => {
                        AudioBackendKind::Unsupported
                    }
                };

                AudioBackendContract {
                    control_plane: AudioBackendKind::ControlPlane,
                    planned_capture: native_backend.clone(),
                    planned_playback: native_backend.clone(),
                    planned_microphone: native_backend,
                    readiness: AudioBackendReadiness::ControlPlaneOnly,
                    notes: vec![
                        "current stream enforces control-plane state only; native audio backend fields are planned"
                            .to_string(),
                    ],
                }
            }
            Self::Unsupported { platform } => AudioBackendContract {
                control_plane: AudioBackendKind::Unsupported,
                planned_capture: AudioBackendKind::Unsupported,
                planned_playback: AudioBackendKind::Unsupported,
                planned_microphone: AudioBackendKind::Unsupported,
                readiness: AudioBackendReadiness::Unsupported,
                notes: vec![format!(
                    "audio native backend contract is unsupported on {platform:?}"
                )],
            },
        }
    }

    fn ensure_supported(&self) -> Result<(), AppRelayError> {
        match self {
            Self::DesktopControl { .. } => Ok(()),
            Self::Unsupported { platform } => Err(AppRelayError::unsupported(
                *platform,
                Feature::SystemAudioStream,
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InMemoryAudioStreamService {
    backend: AudioBackendService,
    streams: Vec<AudioStreamSession>,
    next_stream_number: u64,
}

impl InMemoryAudioStreamService {
    pub fn new(backend: AudioBackendService) -> Self {
        Self {
            backend,
            streams: Vec::new(),
            next_stream_number: 1,
        }
    }

    fn next_stream_id(&mut self) -> String {
        let stream_id = format!("audio-stream-{}", self.next_stream_number);
        self.next_stream_number += 1;
        stream_id
    }

    fn source_from_session(session: &ApplicationSession) -> AudioSource {
        AudioSource {
            scope: AudioCaptureScope::SelectedApplication,
            selected_window_id: session.selected_window.id.clone(),
            application_id: session.application_id.clone(),
            title: session.selected_window.title.clone(),
        }
    }
}

impl Default for InMemoryAudioStreamService {
    fn default() -> Self {
        Self::new(AudioBackendService::DesktopControl {
            platform: Platform::Linux,
        })
    }
}

impl AudioStreamService for InMemoryAudioStreamService {
    fn start_stream(
        &mut self,
        request: StartAudioStreamRequest,
        session: &ApplicationSession,
    ) -> Result<AudioStreamSession, AppRelayError> {
        self.backend.ensure_supported()?;

        if session.id != request.session_id || session.state == SessionState::Closed {
            return Err(AppRelayError::NotFound(format!(
                "session {} was not found",
                request.session_id
            )));
        }

        let capabilities = self.backend.capabilities();
        if request.microphone == MicrophoneMode::Enabled
            && !capabilities.microphone_capture.supported
        {
            return Err(AppRelayError::PermissionDenied(format!(
                "microphone input is not available for session {}",
                request.session_id
            )));
        }

        let stream = AudioStreamSession {
            id: self.next_stream_id(),
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            source: Self::source_from_session(session),
            backend: Some(self.backend.backend_contract()),
            devices: AudioDeviceSelection {
                output_device_id: request.output_device_id,
                input_device_id: request.input_device_id,
            },
            microphone: request.microphone,
            mute: AudioMuteState {
                system_audio_muted: request.system_audio_muted,
                microphone_muted: request.microphone_muted,
            },
            capabilities,
            stats: AudioStreamStats {
                packets_sent: 0,
                packets_received: 0,
                latency_ms: 0,
            },
            health: AudioStreamHealth {
                healthy: true,
                message: Some("audio stream started".to_string()),
            },
            state: AudioStreamState::Streaming,
        };

        self.streams.push(stream.clone());
        Ok(stream)
    }

    fn stop_stream(
        &mut self,
        request: StopAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| {
                stream.id == request.stream_id && stream.state != AudioStreamState::Stopped
            })
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("audio stream {} was not found", request.stream_id))
            })?;

        stream.state = AudioStreamState::Stopped;
        stream.health = AudioStreamHealth {
            healthy: false,
            message: Some("audio stream stopped by client".to_string()),
        };
        Ok(stream.clone())
    }

    fn update_stream(
        &mut self,
        request: UpdateAudioStreamRequest,
    ) -> Result<AudioStreamSession, AppRelayError> {
        let stream = self
            .streams
            .iter_mut()
            .find(|stream| stream.id == request.stream_id)
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("audio stream {} was not found", request.stream_id))
            })?;

        if stream.state == AudioStreamState::Stopped {
            return Err(AppRelayError::InvalidRequest(format!(
                "audio stream {} has been stopped",
                request.stream_id
            )));
        }

        stream.mute = AudioMuteState {
            system_audio_muted: request.system_audio_muted,
            microphone_muted: request.microphone_muted,
        };
        stream.devices = AudioDeviceSelection {
            output_device_id: request.output_device_id,
            input_device_id: request.input_device_id,
        };
        stream.health = AudioStreamHealth {
            healthy: true,
            message: Some("audio stream controls updated".to_string()),
        };
        Ok(stream.clone())
    }

    fn stream_status(&self, stream_id: &str) -> Result<AudioStreamSession, AppRelayError> {
        self.streams
            .iter()
            .find(|stream| stream.id == stream_id)
            .cloned()
            .ok_or_else(|| {
                AppRelayError::NotFound(format!("audio stream {stream_id} was not found"))
            })
    }

    fn record_session_closed(&mut self, session_id: &str) {
        for stream in self.streams.iter_mut().filter(|stream| {
            stream.session_id == session_id && stream.state != AudioStreamState::Stopped
        }) {
            stream.state = AudioStreamState::Stopped;
            stream.health = AudioStreamHealth {
                healthy: false,
                message: Some(format!("application session {session_id} closed")),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApplicationSessionService, InMemoryApplicationSessionService};
    use apprelay_protocol::{CreateSessionRequest, ViewportSize};

    #[test]
    fn audio_stream_starts_with_opt_in_microphone_and_mute_state() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::default();

        let stream = stream_service
            .start_stream(
                StartAudioStreamRequest {
                    session_id: session.id.clone(),
                    microphone: MicrophoneMode::Enabled,
                    system_audio_muted: false,
                    microphone_muted: true,
                    output_device_id: Some("speakers".to_string()),
                    input_device_id: Some("mic".to_string()),
                },
                &session,
            )
            .expect("start audio stream");

        assert_eq!(stream.session_id, session.id);
        assert_eq!(stream.microphone, MicrophoneMode::Enabled);
        assert!(!stream.mute.system_audio_muted);
        assert!(stream.mute.microphone_muted);
        assert!(stream.capabilities.system_audio.supported);
        let backend = stream.backend.as_ref().expect("backend contract");
        assert_eq!(backend.control_plane, AudioBackendKind::ControlPlane);
        assert_eq!(backend.planned_capture, AudioBackendKind::PipeWire);
        assert_eq!(backend.planned_playback, AudioBackendKind::PipeWire);
        assert_eq!(backend.planned_microphone, AudioBackendKind::PipeWire);
        assert_eq!(backend.readiness, AudioBackendReadiness::ControlPlaneOnly);
        assert_eq!(stream.state, AudioStreamState::Streaming);
    }

    #[test]
    fn audio_backend_contract_maps_native_backends_by_platform() {
        let cases = [
            (Platform::Linux, AudioBackendKind::PipeWire),
            (Platform::Macos, AudioBackendKind::CoreAudio),
            (Platform::Windows, AudioBackendKind::Wasapi),
        ];

        for (platform, expected_backend) in cases {
            let contract = AudioBackendService::for_platform(platform).backend_contract();

            assert_eq!(contract.control_plane, AudioBackendKind::ControlPlane);
            assert_eq!(contract.planned_capture, expected_backend);
            assert_eq!(contract.planned_playback, expected_backend);
            assert_eq!(contract.planned_microphone, expected_backend);
            assert_eq!(contract.readiness, AudioBackendReadiness::ControlPlaneOnly);
        }
    }

    #[test]
    fn audio_backend_contract_marks_mobile_platforms_unsupported() {
        let contract = AudioBackendService::for_platform(Platform::Ios).backend_contract();

        assert_eq!(contract.control_plane, AudioBackendKind::Unsupported);
        assert_eq!(contract.planned_capture, AudioBackendKind::Unsupported);
        assert_eq!(contract.planned_playback, AudioBackendKind::Unsupported);
        assert_eq!(contract.planned_microphone, AudioBackendKind::Unsupported);
        assert_eq!(contract.readiness, AudioBackendReadiness::Unsupported);
    }

    #[test]
    fn audio_stream_updates_mute_and_devices() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::default();
        let stream = stream_service
            .start_stream(
                StartAudioStreamRequest {
                    session_id: session.id.clone(),
                    microphone: MicrophoneMode::Disabled,
                    system_audio_muted: false,
                    microphone_muted: true,
                    output_device_id: None,
                    input_device_id: None,
                },
                &session,
            )
            .expect("start audio stream");

        let updated = stream_service
            .update_stream(UpdateAudioStreamRequest {
                stream_id: stream.id,
                system_audio_muted: true,
                microphone_muted: true,
                output_device_id: Some("headphones".to_string()),
                input_device_id: None,
            })
            .expect("update audio stream");

        assert!(updated.mute.system_audio_muted);
        assert_eq!(
            updated.devices.output_device_id.as_deref(),
            Some("headphones")
        );
        assert_eq!(
            updated.health.message.as_deref(),
            Some("audio stream controls updated")
        );
    }
}
