use apprelay_protocol::{
    AppRelayError, ApplicationSession, AudioBackendContract, AudioBackendFailure,
    AudioBackendFailureKind, AudioBackendKind, AudioBackendLeg, AudioBackendMediaStats,
    AudioBackendReadiness, AudioBackendStatus, AudioCapability, AudioCaptureScope,
    AudioDeviceSelection, AudioMuteState, AudioSource, AudioStreamCapabilities, AudioStreamHealth,
    AudioStreamSession, AudioStreamState, AudioStreamStats, Feature, MicrophoneInjectionState,
    MicrophoneMode, Platform, SessionState, StartAudioStreamRequest, StopAudioStreamRequest,
    UpdateAudioStreamRequest,
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
    DesktopControl {
        platform: Platform,
        native_readiness: AudioBackendNativeReadiness,
    },
    Unsupported {
        platform: Platform,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioBackendNativeReadiness {
    available_legs: Vec<AudioBackendLeg>,
}

impl AudioBackendNativeReadiness {
    pub fn unavailable() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn with_available_legs(available_legs: impl IntoIterator<Item = AudioBackendLeg>) -> Self {
        let mut available_legs = available_legs.into_iter().collect::<Vec<_>>();
        available_legs.sort_by_key(Self::leg_sort_key);
        available_legs.dedup();
        Self { available_legs }
    }

    fn is_available(&self, leg: &AudioBackendLeg) -> bool {
        self.available_legs.contains(leg)
    }

    fn all_native_legs_available(&self) -> bool {
        Self::native_legs().iter().all(|leg| self.is_available(leg))
    }

    fn no_native_legs_available(&self) -> bool {
        self.available_legs.is_empty()
    }

    fn native_legs() -> [AudioBackendLeg; 4] {
        [
            AudioBackendLeg::Capture,
            AudioBackendLeg::Playback,
            AudioBackendLeg::ClientMicrophoneCapture,
            AudioBackendLeg::ServerMicrophoneInjection,
        ]
    }

    #[cfg(test)]
    fn leg_sort_key(leg: &AudioBackendLeg) -> u8 {
        match leg {
            AudioBackendLeg::Capture => 0,
            AudioBackendLeg::Playback => 1,
            AudioBackendLeg::ClientMicrophoneCapture => 2,
            AudioBackendLeg::ServerMicrophoneInjection => 3,
        }
    }
}

impl AudioBackendService {
    pub fn for_platform(platform: Platform) -> Self {
        match platform {
            Platform::Linux | Platform::Macos | Platform::Windows => Self::DesktopControl {
                platform,
                native_readiness: AudioBackendNativeReadiness::unavailable(),
            },
            Platform::Android | Platform::Ios | Platform::Unknown => Self::Unsupported { platform },
        }
    }

    #[cfg(test)]
    fn for_platform_with_native_readiness(
        platform: Platform,
        native_readiness: AudioBackendNativeReadiness,
    ) -> Self {
        match platform {
            Platform::Linux | Platform::Macos | Platform::Windows => Self::DesktopControl {
                platform,
                native_readiness,
            },
            Platform::Android | Platform::Ios | Platform::Unknown => Self::Unsupported { platform },
        }
    }

    #[cfg(test)]
    fn configure_native_readiness(&mut self, native_readiness: AudioBackendNativeReadiness) {
        if let Self::DesktopControl {
            native_readiness: current,
            ..
        } = self
        {
            *current = native_readiness;
        }
    }

    pub fn capabilities(&self) -> AudioStreamCapabilities {
        match self {
            Self::DesktopControl {
                native_readiness, ..
            } => {
                let microphone_injection_available =
                    native_readiness.is_available(&AudioBackendLeg::ServerMicrophoneInjection);
                AudioStreamCapabilities {
                    system_audio: AudioCapability {
                        supported: true,
                        reason: Some(
                            "desktop audio control-plane support is available".to_string(),
                        ),
                    },
                    microphone_capture: AudioCapability {
                        supported: true,
                        reason: Some(
                            "desktop microphone control-plane support is available".to_string(),
                        ),
                    },
                    microphone_injection: AudioCapability {
                        supported: microphone_injection_available,
                        reason: if microphone_injection_available {
                            None
                        } else {
                            Some(
                                "server-side microphone injection backend is not implemented yet"
                                    .to_string(),
                            )
                        },
                    },
                    echo_cancellation: AudioCapability {
                        supported: true,
                        reason: None,
                    },
                    device_selection: AudioCapability {
                        supported: true,
                        reason: None,
                    },
                }
            }
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
            Self::DesktopControl {
                platform,
                native_readiness,
            } => {
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
                    statuses: Self::desktop_backend_statuses(*platform, native_readiness),
                    readiness: if native_readiness.all_native_legs_available() {
                        AudioBackendReadiness::NativeAvailable
                    } else {
                        AudioBackendReadiness::ControlPlaneOnly
                    },
                    notes: Self::desktop_backend_notes(native_readiness),
                }
            }
            Self::Unsupported { platform } => AudioBackendContract {
                control_plane: AudioBackendKind::Unsupported,
                planned_capture: AudioBackendKind::Unsupported,
                planned_playback: AudioBackendKind::Unsupported,
                planned_microphone: AudioBackendKind::Unsupported,
                statuses: Self::unsupported_backend_statuses(*platform),
                readiness: AudioBackendReadiness::Unsupported,
                notes: vec![format!(
                    "audio native backend contract is unsupported on {platform:?}"
                )],
            },
        }
    }

    fn desktop_backend_notes(native_readiness: &AudioBackendNativeReadiness) -> Vec<String> {
        if native_readiness.all_native_legs_available() {
            vec![
                "all native audio backend legs are configured available for transport-neutral service tests"
                    .to_string(),
            ]
        } else if native_readiness.no_native_legs_available() {
            vec![
                "current stream enforces control-plane state only; native capture, playback, client microphone capture, and server microphone injection are not implemented"
                    .to_string(),
            ]
        } else {
            vec![
                "current stream enforces control-plane state for unavailable native legs; configured native leg availability is reported per backend status"
                    .to_string(),
            ]
        }
    }

    fn desktop_backend_statuses(
        platform: Platform,
        native_readiness: &AudioBackendNativeReadiness,
    ) -> Vec<AudioBackendStatus> {
        let backend = match platform {
            Platform::Linux => AudioBackendKind::PipeWire,
            Platform::Macos => AudioBackendKind::CoreAudio,
            Platform::Windows => AudioBackendKind::Wasapi,
            Platform::Android | Platform::Ios | Platform::Unknown => AudioBackendKind::Unsupported,
        };

        AudioBackendNativeReadiness::native_legs()
        .into_iter()
        .map(|leg| AudioBackendStatus {
            leg: leg.clone(),
            backend: backend.clone(),
            available: native_readiness.is_available(&leg),
            readiness: if native_readiness.is_available(&leg) {
                AudioBackendReadiness::NativeAvailable
            } else {
                AudioBackendReadiness::PlannedNative
            },
            media: AudioBackendMediaStats::default(),
            failure: if native_readiness.is_available(&leg) {
                None
            } else {
                Some(AudioBackendFailure {
                    kind: AudioBackendFailureKind::NativeBackendNotImplemented,
                    message: Self::native_backend_gap_message(&leg, platform),
                    recovery: "keep the control-plane stream active for state negotiation, but do not expect audio packets until the native backend is implemented".to_string(),
                })
            },
        })
        .collect()
    }

    fn unsupported_backend_statuses(platform: Platform) -> Vec<AudioBackendStatus> {
        [
            AudioBackendLeg::Capture,
            AudioBackendLeg::Playback,
            AudioBackendLeg::ClientMicrophoneCapture,
            AudioBackendLeg::ServerMicrophoneInjection,
        ]
        .into_iter()
        .map(|leg| AudioBackendStatus {
            leg,
            backend: AudioBackendKind::Unsupported,
            available: false,
            readiness: AudioBackendReadiness::Unsupported,
            media: AudioBackendMediaStats::default(),
            failure: Some(AudioBackendFailure {
                kind: AudioBackendFailureKind::UnsupportedPlatform,
                message: format!("audio native backend is unsupported on {platform:?}"),
                recovery: "run the desktop server on Linux, macOS, or Windows".to_string(),
            }),
        })
        .collect()
    }

    fn native_backend_gap_message(leg: &AudioBackendLeg, platform: Platform) -> String {
        let backend = match platform {
            Platform::Linux => "PipeWire",
            Platform::Macos => "CoreAudio",
            Platform::Windows => "WASAPI",
            Platform::Android | Platform::Ios | Platform::Unknown => "native",
        };
        let capability = match leg {
            AudioBackendLeg::Capture => "desktop audio capture",
            AudioBackendLeg::Playback => "client playback",
            AudioBackendLeg::ClientMicrophoneCapture => "client microphone capture",
            AudioBackendLeg::ServerMicrophoneInjection => "server-side microphone injection",
        };

        format!("{capability} via {backend} is not implemented yet")
    }

    fn microphone_injection_readiness(&self) -> AudioBackendReadiness {
        match self {
            Self::DesktopControl {
                native_readiness, ..
            } => {
                if native_readiness.is_available(&AudioBackendLeg::ServerMicrophoneInjection) {
                    AudioBackendReadiness::NativeAvailable
                } else {
                    AudioBackendReadiness::PlannedNative
                }
            }
            Self::Unsupported { .. } => AudioBackendReadiness::Unsupported,
        }
    }

    fn microphone_injection_state(
        &self,
        microphone: &MicrophoneMode,
        capabilities: &AudioStreamCapabilities,
    ) -> MicrophoneInjectionState {
        let requested = microphone == &MicrophoneMode::Enabled;
        let active = requested
            && capabilities.microphone_injection.supported
            && self.microphone_injection_readiness() == AudioBackendReadiness::NativeAvailable;
        let reason = if !requested {
            Some("microphone input is disabled for this session".to_string())
        } else if active {
            None
        } else if !capabilities.microphone_injection.supported {
            capabilities.microphone_injection.reason.clone()
        } else {
            Some("microphone injection is waiting for transport media".to_string())
        };

        MicrophoneInjectionState {
            requested,
            active,
            readiness: self.microphone_injection_readiness(),
            reason,
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

    #[cfg(test)]
    fn configure_native_readiness(&mut self, native_readiness: AudioBackendNativeReadiness) {
        self.backend.configure_native_readiness(native_readiness);
        self.refresh_active_stream_backend_state();
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

    #[cfg(test)]
    fn refresh_active_stream_backend_state(&mut self) {
        let backend = self.backend.clone();
        let backend_contract = self.backend.backend_contract();
        let capabilities = self.backend.capabilities();
        for stream in self
            .streams
            .iter_mut()
            .filter(|stream| stream.state != AudioStreamState::Stopped)
        {
            stream.backend = Some(backend_contract.clone());
            stream.capabilities = capabilities.clone();
            stream.microphone_injection =
                backend.microphone_injection_state(&stream.microphone, &capabilities);
            stream.health = AudioStreamHealth {
                healthy: true,
                message: Some("audio backend readiness updated".to_string()),
            };
        }
    }
}

impl Default for InMemoryAudioStreamService {
    fn default() -> Self {
        Self::new(AudioBackendService::DesktopControl {
            platform: Platform::Linux,
            native_readiness: AudioBackendNativeReadiness::default(),
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

        let microphone_injection = self
            .backend
            .microphone_injection_state(&request.microphone, &capabilities);

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
            microphone_injection,
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
        assert!(stream.microphone_injection.requested);
        assert!(!stream.microphone_injection.active);
        assert_eq!(
            stream.microphone_injection.readiness,
            AudioBackendReadiness::PlannedNative
        );
        assert_eq!(
            stream.microphone_injection.reason.as_deref(),
            Some("server-side microphone injection backend is not implemented yet")
        );
        assert!(!stream.mute.system_audio_muted);
        assert!(stream.mute.microphone_muted);
        assert!(stream.capabilities.system_audio.supported);
        let backend = stream.backend.as_ref().expect("backend contract");
        assert_eq!(backend.control_plane, AudioBackendKind::ControlPlane);
        assert_eq!(backend.planned_capture, AudioBackendKind::PipeWire);
        assert_eq!(backend.planned_playback, AudioBackendKind::PipeWire);
        assert_eq!(backend.planned_microphone, AudioBackendKind::PipeWire);
        assert_eq!(backend.readiness, AudioBackendReadiness::ControlPlaneOnly);
        assert_eq!(backend.statuses.len(), 4);
        assert!(backend.statuses.iter().all(|status| !status.available));
        assert!(backend.statuses.iter().all(|status| {
            status.media
                == AudioBackendMediaStats {
                    available: false,
                    packets_sent: 0,
                    packets_received: 0,
                    bytes_sent: 0,
                    bytes_received: 0,
                    latency_ms: 0,
                }
        }));
        assert!(backend.statuses.iter().all(|status| {
            status.failure.as_ref().is_some_and(|failure| {
                failure.kind == AudioBackendFailureKind::NativeBackendNotImplemented
            })
        }));
        assert_eq!(stream.state, AudioStreamState::Streaming);
    }

    #[test]
    fn audio_stream_reports_microphone_injection_not_requested_when_microphone_disabled() {
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

        assert!(!stream.microphone_injection.requested);
        assert!(!stream.microphone_injection.active);
        assert_eq!(
            stream.microphone_injection.readiness,
            AudioBackendReadiness::PlannedNative
        );
        assert_eq!(
            stream.microphone_injection.reason.as_deref(),
            Some("microphone input is disabled for this session")
        );
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
            assert_eq!(
                contract
                    .statuses
                    .iter()
                    .map(|status| status.leg.clone())
                    .collect::<Vec<_>>(),
                vec![
                    AudioBackendLeg::Capture,
                    AudioBackendLeg::Playback,
                    AudioBackendLeg::ClientMicrophoneCapture,
                    AudioBackendLeg::ServerMicrophoneInjection,
                ]
            );
            assert!(contract.statuses.iter().all(|status| {
                status.backend == expected_backend
                    && !status.available
                    && status.readiness == AudioBackendReadiness::PlannedNative
                    && !status.media.available
                    && status.media.packets_sent == 0
                    && status.media.packets_received == 0
                    && status.media.bytes_sent == 0
                    && status.media.bytes_received == 0
                    && status.media.latency_ms == 0
                    && status.failure.as_ref().is_some_and(|failure| {
                        failure.kind == AudioBackendFailureKind::NativeBackendNotImplemented
                    })
            }));
        }
    }

    #[test]
    fn audio_backend_contract_can_model_native_leg_readiness() {
        let contract = AudioBackendService::for_platform_with_native_readiness(
            Platform::Linux,
            AudioBackendNativeReadiness::with_available_legs([
                AudioBackendLeg::Capture,
                AudioBackendLeg::Playback,
                AudioBackendLeg::ClientMicrophoneCapture,
            ]),
        )
        .backend_contract();

        assert_eq!(contract.readiness, AudioBackendReadiness::ControlPlaneOnly);
        for available_leg in [
            AudioBackendLeg::Capture,
            AudioBackendLeg::Playback,
            AudioBackendLeg::ClientMicrophoneCapture,
        ] {
            let status = contract
                .statuses
                .iter()
                .find(|status| status.leg == available_leg)
                .expect("available leg status");
            assert!(status.available);
            assert_eq!(status.readiness, AudioBackendReadiness::NativeAvailable);
            assert!(!status.media.available);
            assert_eq!(status.media.packets_sent, 0);
            assert_eq!(status.media.packets_received, 0);
            assert_eq!(status.media.bytes_sent, 0);
            assert_eq!(status.media.bytes_received, 0);
            assert_eq!(status.media.latency_ms, 0);
            assert_eq!(status.failure, None);
        }

        let microphone_injection = contract
            .statuses
            .iter()
            .find(|status| status.leg == AudioBackendLeg::ServerMicrophoneInjection)
            .expect("microphone injection status");
        assert!(!microphone_injection.available);
        assert_eq!(
            microphone_injection.readiness,
            AudioBackendReadiness::PlannedNative
        );
        assert!(microphone_injection.failure.is_some());
    }

    #[test]
    fn audio_backend_readiness_configuration_refreshes_active_streams() {
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
                    microphone_muted: false,
                    output_device_id: None,
                    input_device_id: None,
                },
                &session,
            )
            .expect("start audio stream");

        stream_service.configure_native_readiness(
            AudioBackendNativeReadiness::with_available_legs([
                AudioBackendLeg::Capture,
                AudioBackendLeg::Playback,
                AudioBackendLeg::ClientMicrophoneCapture,
                AudioBackendLeg::ServerMicrophoneInjection,
            ]),
        );

        let refreshed = stream_service
            .stream_status(&stream.id)
            .expect("stream status after readiness update");
        let backend = refreshed.backend.expect("backend contract");

        assert_eq!(backend.readiness, AudioBackendReadiness::NativeAvailable);
        assert!(backend.statuses.iter().all(|status| {
            status.available
                && status.readiness == AudioBackendReadiness::NativeAvailable
                && !status.media.available
                && status.media.packets_sent == 0
                && status.media.packets_received == 0
                && status.media.bytes_sent == 0
                && status.media.bytes_received == 0
                && status.media.latency_ms == 0
                && status.failure.is_none()
        }));
        assert!(refreshed.capabilities.microphone_injection.supported);
        assert!(refreshed.microphone_injection.active);
        assert_eq!(
            refreshed.microphone_injection.readiness,
            AudioBackendReadiness::NativeAvailable
        );
        assert_eq!(refreshed.microphone_injection.reason, None);
        assert_eq!(
            refreshed.health.message.as_deref(),
            Some("audio backend readiness updated")
        );
    }

    #[test]
    fn audio_backend_contract_marks_mobile_platforms_unsupported() {
        let contract = AudioBackendService::for_platform(Platform::Ios).backend_contract();

        assert_eq!(contract.control_plane, AudioBackendKind::Unsupported);
        assert_eq!(contract.planned_capture, AudioBackendKind::Unsupported);
        assert_eq!(contract.planned_playback, AudioBackendKind::Unsupported);
        assert_eq!(contract.planned_microphone, AudioBackendKind::Unsupported);
        assert_eq!(contract.readiness, AudioBackendReadiness::Unsupported);
        assert_eq!(contract.statuses.len(), 4);
        assert!(contract.statuses.iter().all(|status| {
            status.backend == AudioBackendKind::Unsupported
                && !status.available
                && status.readiness == AudioBackendReadiness::Unsupported
                && status.media == AudioBackendMediaStats::default()
                && status.failure.as_ref().is_some_and(|failure| {
                    failure.kind == AudioBackendFailureKind::UnsupportedPlatform
                })
        }));
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
