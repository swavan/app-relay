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
    #[cfg(any(test, feature = "pipewire-capture"))]
    pipewire_capture_adapter: Option<PipeWireCaptureAdapterRuntime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeAudioMediaBackend {
    platform: Platform,
    kind: AudioBackendKind,
    legs: Vec<NativeAudioMediaBackendLeg>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct NativeAudioMediaRuntime {
    sessions: Vec<NativeAudioMediaSession>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeAudioMediaSession {
    stream_id: String,
    legs: Vec<NativeAudioMediaSessionLeg>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeAudioMediaSessionLeg {
    leg: AudioBackendLeg,
    media: AudioBackendMediaStats,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeAudioMediaBackendLeg {
    leg: AudioBackendLeg,
    state: NativeAudioMediaBackendLegState,
}

#[cfg(any(test, feature = "pipewire-capture"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PipeWireCaptureAdapterRuntime {
    state: PipeWireCaptureAdapterState,
}

#[cfg(any(test, feature = "pipewire-capture"))]
#[derive(Clone, Debug, Eq, PartialEq)]
enum PipeWireCaptureAdapterState {
    BoundaryOnly,
    #[cfg(all(test, feature = "pipewire-capture"))]
    FakeCapture {
        media: AudioBackendMediaStats,
    },
}

#[cfg(any(test, feature = "pipewire-capture"))]
pub(crate) trait PipeWireCaptureRuntimeAdapter {
    fn can_start_capture(&self) -> bool;
    fn start_capture(&self, stream_id: &str) -> Option<NativeAudioMediaSession>;
    fn stop_capture(&self, stream_id: &str);
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum NativeAudioMediaBackendLegState {
    NotImplemented,
    #[cfg(any(test, feature = "pipewire-capture"))]
    PipeWireCaptureAdapterUnavailable(PipeWireCaptureAdapterRuntime),
    #[cfg(any(test, feature = "pipewire-capture"))]
    PipeWireCaptureAdapterAvailable(PipeWireCaptureAdapterRuntime),
    #[cfg(test)]
    AvailableForTest,
}

impl AudioBackendNativeReadiness {
    pub fn unavailable() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "pipewire-capture"))]
    pub fn with_linux_pipewire_capture_adapter_boundary() -> Self {
        Self {
            available_legs: Vec::new(),
            pipewire_capture_adapter: Some(PipeWireCaptureAdapterRuntime::boundary_only()),
        }
    }

    #[cfg(all(test, feature = "pipewire-capture"))]
    fn with_linux_pipewire_capture_runtime_for_test(media: AudioBackendMediaStats) -> Self {
        Self {
            available_legs: Vec::new(),
            pipewire_capture_adapter: Some(PipeWireCaptureAdapterRuntime::fake_capture(media)),
        }
    }

    #[cfg(test)]
    fn with_available_legs(available_legs: impl IntoIterator<Item = AudioBackendLeg>) -> Self {
        let mut available_legs = available_legs.into_iter().collect::<Vec<_>>();
        available_legs.sort_by_key(Self::leg_sort_key);
        available_legs.dedup();
        Self {
            available_legs,
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn is_available(&self, leg: &AudioBackendLeg) -> bool {
        self.available_legs.contains(leg)
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

impl NativeAudioMediaBackend {
    fn for_platform(platform: Platform) -> Option<Self> {
        match platform {
            Platform::Linux => Some(Self::linux_pipewire()),
            Platform::Macos => Some(Self::macos_core_audio()),
            Platform::Windows => Some(Self::windows_wasapi()),
            Platform::Android | Platform::Ios | Platform::Unknown => None,
        }
    }

    fn linux_pipewire() -> Self {
        Self::planned_desktop_backend(Platform::Linux, AudioBackendKind::PipeWire)
    }

    fn macos_core_audio() -> Self {
        Self::planned_desktop_backend(Platform::Macos, AudioBackendKind::CoreAudio)
    }

    fn windows_wasapi() -> Self {
        Self::planned_desktop_backend(Platform::Windows, AudioBackendKind::Wasapi)
    }

    fn planned_desktop_backend(platform: Platform, kind: AudioBackendKind) -> Self {
        Self {
            platform,
            kind,
            legs: AudioBackendNativeReadiness::native_legs()
                .into_iter()
                .map(|leg| NativeAudioMediaBackendLeg {
                    leg,
                    state: NativeAudioMediaBackendLegState::NotImplemented,
                })
                .collect(),
        }
    }

    #[cfg(test)]
    fn for_platform_with_readiness(
        platform: Platform,
        native_readiness: &AudioBackendNativeReadiness,
    ) -> Option<Self> {
        let mut backend = Self::for_platform(platform)?;
        backend.apply_pipewire_capture_adapter(native_readiness);
        for leg in &mut backend.legs {
            if native_readiness.is_available(&leg.leg) {
                leg.state = NativeAudioMediaBackendLegState::AvailableForTest;
            }
        }
        Some(backend)
    }

    #[cfg(not(test))]
    fn for_platform_with_readiness(
        platform: Platform,
        native_readiness: &AudioBackendNativeReadiness,
    ) -> Option<Self> {
        let mut backend = Self::for_platform(platform)?;
        backend.apply_pipewire_capture_adapter(native_readiness);
        Some(backend)
    }

    fn kind(&self) -> AudioBackendKind {
        self.kind.clone()
    }

    fn all_legs_available(&self) -> bool {
        self.legs
            .iter()
            .all(NativeAudioMediaBackendLeg::is_available)
    }

    fn no_legs_available(&self) -> bool {
        self.legs
            .iter()
            .all(|leg| !NativeAudioMediaBackendLeg::is_available(leg))
    }

    fn leg_available(&self, leg: &AudioBackendLeg) -> bool {
        self.legs
            .iter()
            .find(|backend_leg| &backend_leg.leg == leg)
            .is_some_and(NativeAudioMediaBackendLeg::is_available)
    }

    fn readiness(&self) -> AudioBackendReadiness {
        if self.all_legs_available() {
            AudioBackendReadiness::NativeAvailable
        } else {
            AudioBackendReadiness::ControlPlaneOnly
        }
    }

    fn statuses(&self, media_session: Option<&NativeAudioMediaSession>) -> Vec<AudioBackendStatus> {
        self.legs
            .iter()
            .map(|backend_leg| AudioBackendStatus {
                leg: backend_leg.leg.clone(),
                backend: self.kind.clone(),
                available: backend_leg.is_available(),
                readiness: if backend_leg.is_available() {
                    AudioBackendReadiness::NativeAvailable
                } else {
                    AudioBackendReadiness::PlannedNative
                },
                media: backend_leg.media_stats(media_session),
                failure: if backend_leg.is_available() {
                    None
                } else {
                    Some(AudioBackendFailure {
                        kind: AudioBackendFailureKind::NativeBackendNotImplemented,
                        message: backend_leg.unavailable_message(self.platform),
                        recovery: backend_leg.unavailable_recovery(),
                    })
                },
            })
            .collect()
    }

    fn notes(&self) -> Vec<String> {
        if self.all_legs_available() {
            vec![
                "all native audio backend legs are configured available for transport-neutral service tests"
                    .to_string(),
            ]
        } else if self.has_pipewire_capture_runtime() {
            vec![
                "Linux PipeWire capture runtime contract is configured for the capture leg; playback, client microphone capture, and server-side microphone injection remain planned".to_string(),
            ]
        } else if self.has_pipewire_capture_adapter_boundary() {
            vec![
                "Linux PipeWire capture has an adapter boundary configured, but it remains unavailable until a real PipeWire capture runtime is wired; playback, client microphone capture, and server-side microphone injection remain planned".to_string(),
            ]
        } else if self.no_legs_available() {
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

    fn apply_pipewire_capture_adapter(&mut self, native_readiness: &AudioBackendNativeReadiness) {
        #[cfg(not(any(test, feature = "pipewire-capture")))]
        {
            let _ = native_readiness;
        }

        #[cfg(any(test, feature = "pipewire-capture"))]
        {
            if self.platform != Platform::Linux {
                return;
            }

            let Some(adapter) = native_readiness.pipewire_capture_adapter.clone() else {
                return;
            };

            if let Some(capture_leg) = self
                .legs
                .iter_mut()
                .find(|backend_leg| backend_leg.leg == AudioBackendLeg::Capture)
            {
                capture_leg.state = if adapter.can_start_capture() {
                    NativeAudioMediaBackendLegState::PipeWireCaptureAdapterAvailable(adapter)
                } else {
                    NativeAudioMediaBackendLegState::PipeWireCaptureAdapterUnavailable(adapter)
                };
            }
        }
    }

    fn has_pipewire_capture_adapter_boundary(&self) -> bool {
        self.legs
            .iter()
            .any(NativeAudioMediaBackendLeg::is_pipewire_capture_adapter_boundary)
    }

    fn has_pipewire_capture_runtime(&self) -> bool {
        self.legs
            .iter()
            .any(NativeAudioMediaBackendLeg::is_pipewire_capture_runtime)
    }
}

#[cfg(any(test, feature = "pipewire-capture"))]
impl PipeWireCaptureAdapterRuntime {
    fn boundary_only() -> Self {
        Self {
            state: PipeWireCaptureAdapterState::BoundaryOnly,
        }
    }

    #[cfg(all(test, feature = "pipewire-capture"))]
    fn fake_capture(media: AudioBackendMediaStats) -> Self {
        Self {
            state: PipeWireCaptureAdapterState::FakeCapture { media },
        }
    }
}

#[cfg(any(test, feature = "pipewire-capture"))]
impl PipeWireCaptureRuntimeAdapter for PipeWireCaptureAdapterRuntime {
    fn can_start_capture(&self) -> bool {
        match self.state {
            PipeWireCaptureAdapterState::BoundaryOnly => false,
            #[cfg(all(test, feature = "pipewire-capture"))]
            PipeWireCaptureAdapterState::FakeCapture { .. } => true,
        }
    }

    fn start_capture(&self, _stream_id: &str) -> Option<NativeAudioMediaSession> {
        match &self.state {
            PipeWireCaptureAdapterState::BoundaryOnly => None,
            #[cfg(all(test, feature = "pipewire-capture"))]
            PipeWireCaptureAdapterState::FakeCapture { media } => Some(NativeAudioMediaSession {
                stream_id: _stream_id.to_string(),
                legs: vec![NativeAudioMediaSessionLeg {
                    leg: AudioBackendLeg::Capture,
                    media: AudioBackendMediaStats {
                        available: true,
                        ..media.clone()
                    },
                }],
            }),
        }
    }

    fn stop_capture(&self, _stream_id: &str) {}
}

impl NativeAudioMediaBackendLeg {
    fn media_stats(
        &self,
        media_session: Option<&NativeAudioMediaSession>,
    ) -> AudioBackendMediaStats {
        if !self.is_available() {
            return AudioBackendMediaStats::default();
        }

        media_session
            .and_then(|session| session.media_stats(&self.leg))
            .cloned()
            .unwrap_or_default()
    }

    fn is_available(&self) -> bool {
        match self.state {
            NativeAudioMediaBackendLegState::NotImplemented => false,
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterUnavailable(_) => false,
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterAvailable(_) => true,
            #[cfg(test)]
            NativeAudioMediaBackendLegState::AvailableForTest => true,
        }
    }

    fn is_pipewire_capture_adapter_boundary(&self) -> bool {
        match self.state {
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterUnavailable(_) => true,
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterAvailable(_) => false,
            #[cfg(test)]
            NativeAudioMediaBackendLegState::AvailableForTest => false,
            NativeAudioMediaBackendLegState::NotImplemented => false,
        }
    }

    fn is_pipewire_capture_runtime(&self) -> bool {
        match self.state {
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterAvailable(_) => true,
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterUnavailable(_) => false,
            #[cfg(test)]
            NativeAudioMediaBackendLegState::AvailableForTest => false,
            NativeAudioMediaBackendLegState::NotImplemented => false,
        }
    }

    fn unavailable_message(&self, platform: Platform) -> String {
        match self.state {
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterUnavailable(_) => {
                "desktop audio capture has a Linux PipeWire adapter boundary, but the real PipeWire capture runtime is not wired yet".to_string()
            }
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterAvailable(_) => {
                NativeAudioMediaBackend::native_backend_gap_message(&self.leg, platform)
            }
            NativeAudioMediaBackendLegState::NotImplemented => {
                NativeAudioMediaBackend::native_backend_gap_message(&self.leg, platform)
            }
            #[cfg(test)]
            NativeAudioMediaBackendLegState::AvailableForTest => {
                NativeAudioMediaBackend::native_backend_gap_message(&self.leg, platform)
            }
        }
    }

    fn unavailable_recovery(&self) -> String {
        match self.state {
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterUnavailable(_) => {
                "keep the control-plane stream active for state negotiation, but do not expect PipeWire audio packets until the capture runtime is implemented and enabled".to_string()
            }
            #[cfg(any(test, feature = "pipewire-capture"))]
            NativeAudioMediaBackendLegState::PipeWireCaptureAdapterAvailable(_) => {
                "keep the control-plane stream active for state negotiation, but do not expect audio packets until the native backend is implemented".to_string()
            }
            NativeAudioMediaBackendLegState::NotImplemented => {
                "keep the control-plane stream active for state negotiation, but do not expect audio packets until the native backend is implemented".to_string()
            }
            #[cfg(test)]
            NativeAudioMediaBackendLegState::AvailableForTest => {
                "keep the control-plane stream active for state negotiation, but do not expect audio packets until the native backend is implemented".to_string()
            }
        }
    }
}

impl NativeAudioMediaRuntime {
    fn session_for_stream(&self, stream_id: &str) -> Option<&NativeAudioMediaSession> {
        self.sessions
            .iter()
            .find(|session| session.stream_id == stream_id)
    }

    fn clear_stream(&mut self, stream_id: &str) {
        self.sessions
            .retain(|session| session.stream_id != stream_id);
    }

    fn start_session(&mut self, session: NativeAudioMediaSession) {
        if let Some(existing_session) = self
            .sessions
            .iter_mut()
            .find(|existing| existing.stream_id == session.stream_id)
        {
            for session_leg in session.legs {
                existing_session
                    .legs
                    .retain(|existing_leg| existing_leg.leg != session_leg.leg);
                existing_session.legs.push(session_leg);
            }
        } else {
            self.sessions.push(session);
        }
    }

    #[cfg(test)]
    fn clear_stream_leg(&mut self, stream_id: &str, leg: &AudioBackendLeg) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.stream_id == stream_id)
        {
            session.legs.retain(|session_leg| &session_leg.leg != leg);
        }
        self.sessions
            .retain(|session| session.stream_id != stream_id || !session.legs.is_empty());
    }

    #[cfg(test)]
    fn start_test_session(
        &mut self,
        stream_id: impl Into<String>,
        legs: impl IntoIterator<Item = (AudioBackendLeg, AudioBackendMediaStats)>,
    ) {
        let stream_id = stream_id.into();
        self.sessions
            .retain(|session| session.stream_id != stream_id);
        self.sessions.push(NativeAudioMediaSession {
            stream_id,
            legs: legs
                .into_iter()
                .filter_map(|(leg, media)| {
                    media
                        .available
                        .then_some(NativeAudioMediaSessionLeg { leg, media })
                })
                .collect(),
        });
    }
}

impl NativeAudioMediaSession {
    fn media_stats(&self, leg: &AudioBackendLeg) -> Option<&AudioBackendMediaStats> {
        self.legs
            .iter()
            .find(|session_leg| &session_leg.leg == leg)
            .map(|session_leg| &session_leg.media)
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

    #[cfg(any(test, feature = "pipewire-capture"))]
    pub fn for_platform_with_native_readiness(
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
                platform,
                native_readiness,
            } => {
                let Some(native_backend) = NativeAudioMediaBackend::for_platform_with_readiness(
                    *platform,
                    native_readiness,
                ) else {
                    return Self::unsupported_capabilities(*platform);
                };
                let microphone_injection_available =
                    native_backend.leg_available(&AudioBackendLeg::ServerMicrophoneInjection);
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
            Self::Unsupported { platform } => Self::unsupported_capabilities(*platform),
        }
    }

    pub fn backend_contract(&self) -> AudioBackendContract {
        self.backend_contract_for_media_session(None)
    }

    fn backend_contract_for_media_session(
        &self,
        media_session: Option<&NativeAudioMediaSession>,
    ) -> AudioBackendContract {
        match self {
            Self::DesktopControl {
                platform,
                native_readiness,
            } => {
                let Some(native_backend) = NativeAudioMediaBackend::for_platform_with_readiness(
                    *platform,
                    native_readiness,
                ) else {
                    return Self::unsupported_backend_contract(*platform);
                };
                let native_backend_kind = native_backend.kind();

                AudioBackendContract {
                    control_plane: AudioBackendKind::ControlPlane,
                    planned_capture: native_backend_kind.clone(),
                    planned_playback: native_backend_kind.clone(),
                    planned_microphone: native_backend_kind,
                    statuses: native_backend.statuses(media_session),
                    readiness: native_backend.readiness(),
                    notes: native_backend.notes(),
                }
            }
            Self::Unsupported { platform } => Self::unsupported_backend_contract(*platform),
        }
    }

    fn unsupported_capabilities(platform: Platform) -> AudioStreamCapabilities {
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

    fn unsupported_backend_contract(platform: Platform) -> AudioBackendContract {
        AudioBackendContract {
            control_plane: AudioBackendKind::Unsupported,
            planned_capture: AudioBackendKind::Unsupported,
            planned_playback: AudioBackendKind::Unsupported,
            planned_microphone: AudioBackendKind::Unsupported,
            statuses: Self::unsupported_backend_statuses(platform),
            readiness: AudioBackendReadiness::Unsupported,
            notes: vec![format!(
                "audio native backend contract is unsupported on {platform:?}"
            )],
        }
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

    fn microphone_injection_readiness(&self) -> AudioBackendReadiness {
        match self {
            Self::DesktopControl {
                platform,
                native_readiness,
            } => {
                let Some(native_backend) = NativeAudioMediaBackend::for_platform_with_readiness(
                    *platform,
                    native_readiness,
                ) else {
                    return AudioBackendReadiness::Unsupported;
                };
                if native_backend.leg_available(&AudioBackendLeg::ServerMicrophoneInjection) {
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
            Self::DesktopControl { platform, .. } => {
                if NativeAudioMediaBackend::for_platform(*platform).is_some() {
                    Ok(())
                } else {
                    Err(AppRelayError::unsupported(
                        *platform,
                        Feature::SystemAudioStream,
                    ))
                }
            }
            Self::Unsupported { platform } => Err(AppRelayError::unsupported(
                *platform,
                Feature::SystemAudioStream,
            )),
        }
    }

    #[cfg(any(test, feature = "pipewire-capture"))]
    fn start_pipewire_capture(&self, stream_id: &str) -> Option<NativeAudioMediaSession> {
        let Self::DesktopControl {
            platform: Platform::Linux,
            native_readiness,
        } = self
        else {
            return None;
        };

        native_readiness
            .pipewire_capture_adapter
            .as_ref()
            .and_then(|adapter| adapter.start_capture(stream_id))
    }

    #[cfg(not(any(test, feature = "pipewire-capture")))]
    fn start_pipewire_capture(&self, _stream_id: &str) -> Option<NativeAudioMediaSession> {
        None
    }

    #[cfg(any(test, feature = "pipewire-capture"))]
    fn stop_pipewire_capture(&self, stream_id: &str) {
        let Self::DesktopControl {
            platform: Platform::Linux,
            native_readiness,
        } = self
        else {
            return;
        };

        if let Some(adapter) = &native_readiness.pipewire_capture_adapter {
            adapter.stop_capture(stream_id);
        }
    }

    #[cfg(not(any(test, feature = "pipewire-capture")))]
    fn stop_pipewire_capture(&self, _stream_id: &str) {}
}

#[derive(Clone, Debug)]
pub struct InMemoryAudioStreamService {
    backend: AudioBackendService,
    native_runtime: NativeAudioMediaRuntime,
    streams: Vec<AudioStreamSession>,
    next_stream_number: u64,
}

impl InMemoryAudioStreamService {
    pub fn new(backend: AudioBackendService) -> Self {
        Self {
            backend,
            native_runtime: NativeAudioMediaRuntime::default(),
            streams: Vec::new(),
            next_stream_number: 1,
        }
    }

    #[cfg(test)]
    fn configure_native_readiness(&mut self, native_readiness: AudioBackendNativeReadiness) {
        self.backend.configure_native_readiness(native_readiness);
        self.reconcile_pipewire_capture_sessions();
        self.refresh_active_stream_backend_state();
    }

    #[cfg(test)]
    fn start_native_media_session_for_test(
        &mut self,
        stream_id: &str,
        legs: impl IntoIterator<Item = (AudioBackendLeg, AudioBackendMediaStats)>,
    ) {
        self.native_runtime.start_test_session(stream_id, legs);
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
    fn reconcile_pipewire_capture_sessions(&mut self) {
        let active_stream_ids = self
            .streams
            .iter()
            .filter(|stream| stream.state != AudioStreamState::Stopped)
            .map(|stream| stream.id.clone())
            .collect::<Vec<_>>();

        for stream_id in active_stream_ids {
            if let Some(native_session) = self.backend.start_pipewire_capture(&stream_id) {
                self.native_runtime.start_session(native_session);
            } else {
                self.backend.stop_pipewire_capture(&stream_id);
                self.native_runtime
                    .clear_stream_leg(&stream_id, &AudioBackendLeg::Capture);
            }
        }
    }

    #[cfg(test)]
    fn refresh_active_stream_backend_state(&mut self) {
        let backend = self.backend.clone();
        let capabilities = self.backend.capabilities();
        for stream in self
            .streams
            .iter_mut()
            .filter(|stream| stream.state != AudioStreamState::Stopped)
        {
            stream.backend = Some(self.backend.backend_contract_for_media_session(
                self.native_runtime.session_for_stream(&stream.id),
            ));
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
        let stream_id = self.next_stream_id();
        if let Some(native_session) = self.backend.start_pipewire_capture(&stream_id) {
            self.native_runtime.start_session(native_session);
        }
        let source = Self::source_from_session(session);

        let stream = AudioStreamSession {
            backend: Some(self.backend.backend_contract_for_media_session(
                self.native_runtime.session_for_stream(&stream_id),
            )),
            id: stream_id,
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            source,
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
        self.backend.stop_pipewire_capture(&stream.id);
        self.native_runtime.clear_stream(&stream.id);
        stream.backend = Some(self.backend.backend_contract_for_media_session(None));
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
            self.backend.stop_pipewire_capture(&stream.id);
            self.native_runtime.clear_stream(&stream.id);
            stream.backend = Some(self.backend.backend_contract_for_media_session(None));
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
            let native_backend =
                NativeAudioMediaBackend::for_platform(platform).expect("native backend scaffold");
            assert_eq!(native_backend.platform, platform);
            assert_eq!(native_backend.kind(), expected_backend);
            assert_eq!(
                native_backend
                    .legs
                    .iter()
                    .map(|leg| leg.leg.clone())
                    .collect::<Vec<_>>(),
                vec![
                    AudioBackendLeg::Capture,
                    AudioBackendLeg::Playback,
                    AudioBackendLeg::ClientMicrophoneCapture,
                    AudioBackendLeg::ServerMicrophoneInjection,
                ]
            );
            assert!(native_backend
                .legs
                .iter()
                .all(|leg| leg.state == NativeAudioMediaBackendLegState::NotImplemented));

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
    fn audio_backend_default_production_media_status_stays_unavailable() {
        for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
            let contract = AudioBackendService::for_platform(platform).backend_contract();

            assert_eq!(contract.readiness, AudioBackendReadiness::ControlPlaneOnly);
            assert_eq!(contract.statuses.len(), 4);
            assert!(contract.statuses.iter().all(|status| {
                !status.available
                    && status.readiness == AudioBackendReadiness::PlannedNative
                    && status.media == AudioBackendMediaStats::default()
                    && status.failure.as_ref().is_some_and(|failure| {
                        failure.kind == AudioBackendFailureKind::NativeBackendNotImplemented
                    })
            }));
        }
    }

    #[test]
    fn audio_backend_linux_pipewire_capture_adapter_boundary_is_capture_only() {
        let native_backend = NativeAudioMediaBackend::for_platform_with_readiness(
            Platform::Linux,
            &AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
        )
        .expect("linux pipewire backend");

        assert_eq!(
            native_backend
                .legs
                .iter()
                .filter(|leg| leg.is_pipewire_capture_adapter_boundary())
                .map(|leg| leg.leg.clone())
                .collect::<Vec<_>>(),
            vec![AudioBackendLeg::Capture]
        );
        assert!(native_backend.legs.iter().all(|leg| !leg.is_available()));

        let contract = AudioBackendService::for_platform_with_native_readiness(
            Platform::Linux,
            AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
        )
        .backend_contract();

        assert_eq!(contract.readiness, AudioBackendReadiness::ControlPlaneOnly);
        assert!(contract
            .notes
            .iter()
            .any(|note| note.contains("PipeWire capture has an adapter boundary")));

        for status in &contract.statuses {
            assert!(!status.available);
            assert_eq!(status.readiness, AudioBackendReadiness::PlannedNative);
            assert_eq!(status.media, AudioBackendMediaStats::default());
            assert_eq!(
                status.failure.as_ref().map(|failure| &failure.kind),
                Some(&AudioBackendFailureKind::NativeBackendNotImplemented)
            );
        }

        let capture = contract
            .statuses
            .iter()
            .find(|status| status.leg == AudioBackendLeg::Capture)
            .expect("capture status");
        assert!(capture
            .failure
            .as_ref()
            .expect("capture failure")
            .message
            .contains("PipeWire adapter boundary"));

        for planned_leg in [
            AudioBackendLeg::Playback,
            AudioBackendLeg::ClientMicrophoneCapture,
            AudioBackendLeg::ServerMicrophoneInjection,
        ] {
            let status = contract
                .statuses
                .iter()
                .find(|status| status.leg == planned_leg)
                .expect("planned leg status");
            assert!(status
                .failure
                .as_ref()
                .expect("planned leg failure")
                .message
                .contains("is not implemented yet"));
            assert!(!status
                .failure
                .as_ref()
                .expect("planned leg failure")
                .message
                .contains("adapter boundary"));
        }
    }

    #[test]
    fn audio_backend_pipewire_capture_adapter_boundary_does_not_affect_macos_or_windows() {
        for (platform, expected_backend) in [
            (Platform::Macos, AudioBackendKind::CoreAudio),
            (Platform::Windows, AudioBackendKind::Wasapi),
        ] {
            let native_backend = NativeAudioMediaBackend::for_platform_with_readiness(
                platform,
                &AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
            )
            .expect("desktop backend");

            assert_eq!(native_backend.kind(), expected_backend);
            assert!(native_backend
                .legs
                .iter()
                .all(|leg| leg.state == NativeAudioMediaBackendLegState::NotImplemented));

            let contract = AudioBackendService::for_platform_with_native_readiness(
                platform,
                AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
            )
            .backend_contract();

            assert_eq!(contract.readiness, AudioBackendReadiness::ControlPlaneOnly);
            assert!(contract.statuses.iter().all(|status| {
                status.backend == expected_backend
                    && !status.available
                    && status.readiness == AudioBackendReadiness::PlannedNative
                    && status.media == AudioBackendMediaStats::default()
                    && status.failure.as_ref().is_some_and(|failure| {
                        failure.kind == AudioBackendFailureKind::NativeBackendNotImplemented
                            && !failure.message.contains("adapter boundary")
                    })
            }));
        }
    }

    #[cfg(feature = "pipewire-capture")]
    #[test]
    fn audio_backend_pipewire_capture_runtime_reports_capture_telemetry_only() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::new(
            AudioBackendService::for_platform_with_native_readiness(
                Platform::Linux,
                AudioBackendNativeReadiness::with_linux_pipewire_capture_runtime_for_test(
                    pipewire_capture_media_for_test(),
                ),
            ),
        );

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
        let backend = stream.backend.as_ref().expect("backend contract");

        assert_eq!(backend.readiness, AudioBackendReadiness::ControlPlaneOnly);
        assert_pipewire_capture_runtime_status(backend);
        assert!(!stream.microphone_injection.active);

        let stopped = stream_service
            .stop_stream(StopAudioStreamRequest {
                stream_id: stream.id.clone(),
            })
            .expect("stop audio stream");

        assert_eq!(stopped.state, AudioStreamState::Stopped);
        assert_pipewire_capture_runtime_media_cleared(&stopped);
        let status = stream_service
            .stream_status(&stream.id)
            .expect("stopped stream status");
        assert_pipewire_capture_runtime_media_cleared(&status);
    }

    #[cfg(feature = "pipewire-capture")]
    #[test]
    fn audio_backend_pipewire_capture_runtime_session_close_clears_capture_telemetry() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::new(
            AudioBackendService::for_platform_with_native_readiness(
                Platform::Linux,
                AudioBackendNativeReadiness::with_linux_pipewire_capture_runtime_for_test(
                    pipewire_capture_media_for_test(),
                ),
            ),
        );
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

        stream_service.record_session_closed(&session.id);

        let status = stream_service
            .stream_status(&stream.id)
            .expect("closed stream status");
        assert_eq!(status.state, AudioStreamState::Stopped);
        assert_pipewire_capture_runtime_media_cleared(&status);
    }

    #[cfg(feature = "pipewire-capture")]
    #[test]
    fn audio_backend_pipewire_capture_runtime_refresh_starts_active_stream_capture() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::new(
            AudioBackendService::for_platform_with_native_readiness(
                Platform::Linux,
                AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
            ),
        );
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

        stream_service.configure_native_readiness(
            AudioBackendNativeReadiness::with_linux_pipewire_capture_runtime_for_test(
                pipewire_capture_media_for_test(),
            ),
        );

        let refreshed = stream_service
            .stream_status(&stream.id)
            .expect("refreshed stream status");
        assert_pipewire_capture_runtime_status(
            refreshed.backend.as_ref().expect("backend contract"),
        );
        assert_eq!(
            refreshed.health.message.as_deref(),
            Some("audio backend readiness updated")
        );
    }

    #[cfg(feature = "pipewire-capture")]
    #[test]
    fn audio_backend_pipewire_capture_runtime_refresh_clears_downgraded_capture() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::new(
            AudioBackendService::for_platform_with_native_readiness(
                Platform::Linux,
                AudioBackendNativeReadiness::with_linux_pipewire_capture_runtime_for_test(
                    pipewire_capture_media_for_test(),
                ),
            ),
        );
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

        stream_service.configure_native_readiness(
            AudioBackendNativeReadiness::with_linux_pipewire_capture_adapter_boundary(),
        );

        let refreshed = stream_service
            .stream_status(&stream.id)
            .expect("refreshed stream status");
        let backend = refreshed.backend.as_ref().expect("backend contract");
        let capture = backend
            .statuses
            .iter()
            .find(|status| status.leg == AudioBackendLeg::Capture)
            .expect("capture status");
        assert!(!capture.available);
        assert_eq!(capture.readiness, AudioBackendReadiness::PlannedNative);
        assert_eq!(capture.media, AudioBackendMediaStats::default());
        assert!(capture
            .failure
            .as_ref()
            .expect("capture failure")
            .message
            .contains("PipeWire adapter boundary"));
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
    fn audio_backend_runtime_media_session_reports_test_telemetry() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::default();
        stream_service.configure_native_readiness(
            AudioBackendNativeReadiness::with_available_legs(
                AudioBackendNativeReadiness::native_legs(),
            ),
        );
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

        stream_service.start_native_media_session_for_test(
            &stream.id,
            AudioBackendNativeReadiness::native_legs()
                .into_iter()
                .enumerate()
                .map(|(index, leg)| {
                    let offset = index as u64 + 1;
                    (
                        leg,
                        AudioBackendMediaStats {
                            available: true,
                            packets_sent: 10 * offset,
                            packets_received: 20 * offset,
                            bytes_sent: 1000 * offset,
                            bytes_received: 2000 * offset,
                            latency_ms: 12 + index as u32,
                        },
                    )
                }),
        );

        let refreshed = stream_service
            .stream_status(&stream.id)
            .expect("stream status after media session start");
        let backend = refreshed.backend.expect("backend contract");

        assert_eq!(backend.readiness, AudioBackendReadiness::NativeAvailable);
        assert!(backend.statuses.iter().all(|status| {
            status.available
                && status.readiness == AudioBackendReadiness::NativeAvailable
                && status.failure.is_none()
                && status.media.available
                && status.media.packets_sent > 0
                && status.media.packets_received > 0
                && status.media.bytes_sent > 0
                && status.media.bytes_received > 0
                && status.media.latency_ms > 0
        }));
        assert!(refreshed.microphone_injection.active);
    }

    #[test]
    fn audio_stream_stop_clears_native_media_session_status() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::default();
        stream_service.configure_native_readiness(
            AudioBackendNativeReadiness::with_available_legs(
                AudioBackendNativeReadiness::native_legs(),
            ),
        );
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
        stream_service
            .start_native_media_session_for_test(&stream.id, native_media_stats_for_test());

        let stopped = stream_service
            .stop_stream(StopAudioStreamRequest {
                stream_id: stream.id.clone(),
            })
            .expect("stop audio stream");

        assert_eq!(stopped.state, AudioStreamState::Stopped);
        assert_backend_media_cleared(&stopped);
        let status = stream_service
            .stream_status(&stream.id)
            .expect("stopped stream status");
        assert_backend_media_cleared(&status);
    }

    #[test]
    fn audio_stream_session_close_clears_native_media_session_status() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service = InMemoryAudioStreamService::default();
        stream_service.configure_native_readiness(
            AudioBackendNativeReadiness::with_available_legs(
                AudioBackendNativeReadiness::native_legs(),
            ),
        );
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
        let second_stream = stream_service
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
            .expect("start second audio stream");
        stream_service
            .start_native_media_session_for_test(&stream.id, native_media_stats_for_test());
        stream_service
            .start_native_media_session_for_test(&second_stream.id, native_media_stats_for_test());

        stream_service.record_session_closed(&session.id);

        for stream_id in [stream.id, second_stream.id] {
            let status = stream_service
                .stream_status(&stream_id)
                .expect("closed session stream status");
            assert_eq!(status.state, AudioStreamState::Stopped);
            assert_backend_media_cleared(&status);
        }
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

    fn native_media_stats_for_test(
    ) -> impl Iterator<Item = (AudioBackendLeg, AudioBackendMediaStats)> {
        AudioBackendNativeReadiness::native_legs()
            .into_iter()
            .enumerate()
            .map(|(index, leg)| {
                let offset = index as u64 + 1;
                (
                    leg,
                    AudioBackendMediaStats {
                        available: true,
                        packets_sent: 10 * offset,
                        packets_received: 20 * offset,
                        bytes_sent: 1000 * offset,
                        bytes_received: 2000 * offset,
                        latency_ms: 12 + index as u32,
                    },
                )
            })
    }

    #[cfg(feature = "pipewire-capture")]
    fn pipewire_capture_media_for_test() -> AudioBackendMediaStats {
        AudioBackendMediaStats {
            available: true,
            packets_sent: 11,
            packets_received: 23,
            bytes_sent: 1100,
            bytes_received: 2300,
            latency_ms: 9,
        }
    }

    #[cfg(feature = "pipewire-capture")]
    fn assert_pipewire_capture_runtime_status(backend: &AudioBackendContract) {
        let capture = backend
            .statuses
            .iter()
            .find(|status| status.leg == AudioBackendLeg::Capture)
            .expect("capture status");
        assert!(capture.available);
        assert_eq!(capture.readiness, AudioBackendReadiness::NativeAvailable);
        assert_eq!(capture.failure, None);
        assert!(capture.media.available);
        assert!(capture.media.packets_sent > 0);
        assert!(capture.media.packets_received > 0);
        assert!(capture.media.bytes_sent > 0);
        assert!(capture.media.bytes_received > 0);
        assert!(capture.media.latency_ms > 0);

        for planned_leg in [
            AudioBackendLeg::Playback,
            AudioBackendLeg::ClientMicrophoneCapture,
            AudioBackendLeg::ServerMicrophoneInjection,
        ] {
            let status = backend
                .statuses
                .iter()
                .find(|status| status.leg == planned_leg)
                .expect("planned status");
            assert!(!status.available);
            assert_eq!(status.readiness, AudioBackendReadiness::PlannedNative);
            assert_eq!(status.media, AudioBackendMediaStats::default());
            assert_eq!(
                status.failure.as_ref().map(|failure| &failure.kind),
                Some(&AudioBackendFailureKind::NativeBackendNotImplemented)
            );
        }
    }

    #[cfg(feature = "pipewire-capture")]
    fn assert_pipewire_capture_runtime_media_cleared(stream: &AudioStreamSession) {
        let backend = stream.backend.as_ref().expect("backend contract");
        let capture = backend
            .statuses
            .iter()
            .find(|status| status.leg == AudioBackendLeg::Capture)
            .expect("capture status");
        assert!(capture.available);
        assert_eq!(capture.readiness, AudioBackendReadiness::NativeAvailable);
        assert_eq!(capture.failure, None);
        assert_eq!(capture.media, AudioBackendMediaStats::default());

        for planned_leg in [
            AudioBackendLeg::Playback,
            AudioBackendLeg::ClientMicrophoneCapture,
            AudioBackendLeg::ServerMicrophoneInjection,
        ] {
            let status = backend
                .statuses
                .iter()
                .find(|status| status.leg == planned_leg)
                .expect("planned status");
            assert!(!status.available);
            assert_eq!(status.readiness, AudioBackendReadiness::PlannedNative);
            assert_eq!(status.media, AudioBackendMediaStats::default());
        }
    }

    fn assert_backend_media_cleared(stream: &AudioStreamSession) {
        let backend = stream.backend.as_ref().expect("backend contract");
        assert_eq!(backend.readiness, AudioBackendReadiness::NativeAvailable);
        assert!(backend.statuses.iter().all(|status| {
            status.available
                && status.readiness == AudioBackendReadiness::NativeAvailable
                && status.failure.is_none()
                && status.media == AudioBackendMediaStats::default()
        }));
    }

    #[test]
    fn audio_backend_contract_marks_mobile_and_unknown_platforms_unsupported() {
        for platform in [Platform::Android, Platform::Ios, Platform::Unknown] {
            assert!(NativeAudioMediaBackend::for_platform(platform).is_none());

            let contract = AudioBackendService::for_platform(platform).backend_contract();

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
    }

    #[test]
    fn invalid_desktop_control_platform_reports_unsupported_backend_state() {
        for platform in [Platform::Android, Platform::Ios, Platform::Unknown] {
            let backend = AudioBackendService::DesktopControl {
                platform,
                native_readiness: AudioBackendNativeReadiness::unavailable(),
            };

            let capabilities = backend.capabilities();
            let expected_reason = format!("audio streaming is unsupported on {platform:?}");
            assert!(!capabilities.system_audio.supported);
            assert!(!capabilities.microphone_capture.supported);
            assert!(!capabilities.microphone_injection.supported);
            assert!(!capabilities.echo_cancellation.supported);
            assert!(!capabilities.device_selection.supported);
            assert_eq!(
                capabilities.system_audio.reason.as_deref(),
                Some(expected_reason.as_str())
            );

            let contract = backend.backend_contract();
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
                    && status.failure.as_ref().is_some_and(|failure| {
                        failure.kind == AudioBackendFailureKind::UnsupportedPlatform
                    })
            }));
            assert_eq!(
                backend.microphone_injection_readiness(),
                AudioBackendReadiness::Unsupported
            );
        }
    }

    #[test]
    fn invalid_desktop_control_platform_stream_start_reports_unsupported() {
        let mut session_service = InMemoryApplicationSessionService::default();
        let session = session_service
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut stream_service =
            InMemoryAudioStreamService::new(AudioBackendService::DesktopControl {
                platform: Platform::Android,
                native_readiness: AudioBackendNativeReadiness::unavailable(),
            });

        let error = stream_service
            .start_stream(
                StartAudioStreamRequest {
                    session_id: session.id.clone(),
                    microphone: MicrophoneMode::Disabled,
                    system_audio_muted: false,
                    microphone_muted: false,
                    output_device_id: None,
                    input_device_id: None,
                },
                &session,
            )
            .expect_err("invalid desktop control platform should be unsupported");

        assert_eq!(
            error,
            AppRelayError::unsupported(Platform::Android, Feature::SystemAudioStream)
        );
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
