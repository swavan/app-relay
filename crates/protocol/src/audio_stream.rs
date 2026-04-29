#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAudioStreamRequest {
    pub session_id: String,
    pub microphone: MicrophoneMode,
    pub system_audio_muted: bool,
    pub microphone_muted: bool,
    pub output_device_id: Option<String>,
    pub input_device_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopAudioStreamRequest {
    pub stream_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAudioStreamRequest {
    pub stream_id: String,
    pub system_audio_muted: bool,
    pub microphone_muted: bool,
    pub output_device_id: Option<String>,
    pub input_device_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamSession {
    pub id: String,
    pub session_id: String,
    pub selected_window_id: String,
    pub source: AudioSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<AudioBackendContract>,
    pub devices: AudioDeviceSelection,
    pub microphone: MicrophoneMode,
    #[serde(default)]
    pub microphone_injection: MicrophoneInjectionState,
    pub mute: AudioMuteState,
    pub capabilities: AudioStreamCapabilities,
    pub stats: AudioStreamStats,
    pub health: AudioStreamHealth,
    pub state: AudioStreamState,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSource {
    pub scope: AudioCaptureScope,
    pub selected_window_id: String,
    pub application_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioCaptureScope {
    SelectedApplication,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MicrophoneMode {
    Disabled,
    Enabled,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneInjectionState {
    pub requested: bool,
    pub active: bool,
    pub readiness: AudioBackendReadiness,
    pub reason: Option<String>,
}

impl Default for MicrophoneInjectionState {
    fn default() -> Self {
        Self {
            requested: false,
            active: false,
            readiness: AudioBackendReadiness::ControlPlaneOnly,
            reason: Some("microphone injection state was not reported".to_string()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioBackendContract {
    pub control_plane: AudioBackendKind,
    pub planned_capture: AudioBackendKind,
    pub planned_playback: AudioBackendKind,
    pub planned_microphone: AudioBackendKind,
    #[serde(default)]
    pub statuses: Vec<AudioBackendStatus>,
    pub readiness: AudioBackendReadiness,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioBackendStatus {
    pub leg: AudioBackendLeg,
    pub backend: AudioBackendKind,
    pub available: bool,
    pub readiness: AudioBackendReadiness,
    pub failure: Option<AudioBackendFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioBackendLeg {
    Capture,
    Playback,
    ClientMicrophoneCapture,
    ServerMicrophoneInjection,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioBackendFailure {
    pub kind: AudioBackendFailureKind,
    pub message: String,
    pub recovery: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioBackendFailureKind {
    NativeBackendNotImplemented,
    UnsupportedPlatform,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioBackendKind {
    ControlPlane,
    PipeWire,
    CoreAudio,
    Wasapi,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioBackendReadiness {
    ControlPlaneOnly,
    PlannedNative,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceSelection {
    pub output_device_id: Option<String>,
    pub input_device_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMuteState {
    pub system_audio_muted: bool,
    pub microphone_muted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamCapabilities {
    pub system_audio: AudioCapability,
    pub microphone_capture: AudioCapability,
    pub microphone_injection: AudioCapability,
    pub echo_cancellation: AudioCapability,
    pub device_selection: AudioCapability,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCapability {
    pub supported: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub latency_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamHealth {
    pub healthy: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioStreamState {
    Starting,
    Streaming,
    Stopped,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_backend_kind_json_wire_names_are_stable() {
        let cases = [
            (AudioBackendKind::ControlPlane, "\"controlPlane\""),
            (AudioBackendKind::PipeWire, "\"pipeWire\""),
            (AudioBackendKind::CoreAudio, "\"coreAudio\""),
            (AudioBackendKind::Wasapi, "\"wasapi\""),
            (AudioBackendKind::Unsupported, "\"unsupported\""),
        ];

        for (kind, expected_json) in cases {
            assert_eq!(
                serde_json::to_string(&kind).expect("serialize"),
                expected_json
            );
            assert_eq!(
                serde_json::from_str::<AudioBackendKind>(expected_json).expect("deserialize"),
                kind
            );
        }
    }

    #[test]
    fn audio_backend_leg_json_wire_names_are_stable() {
        let cases = [
            (AudioBackendLeg::Capture, "\"capture\""),
            (AudioBackendLeg::Playback, "\"playback\""),
            (
                AudioBackendLeg::ClientMicrophoneCapture,
                "\"clientMicrophoneCapture\"",
            ),
            (
                AudioBackendLeg::ServerMicrophoneInjection,
                "\"serverMicrophoneInjection\"",
            ),
        ];

        for (leg, expected_json) in cases {
            assert_eq!(
                serde_json::to_string(&leg).expect("serialize"),
                expected_json
            );
            assert_eq!(
                serde_json::from_str::<AudioBackendLeg>(expected_json).expect("deserialize"),
                leg
            );
        }
    }

    #[test]
    fn audio_backend_failure_kind_json_wire_names_are_stable() {
        let cases = [
            (
                AudioBackendFailureKind::NativeBackendNotImplemented,
                "\"nativeBackendNotImplemented\"",
            ),
            (
                AudioBackendFailureKind::UnsupportedPlatform,
                "\"unsupportedPlatform\"",
            ),
        ];

        for (kind, expected_json) in cases {
            assert_eq!(
                serde_json::to_string(&kind).expect("serialize"),
                expected_json
            );
            assert_eq!(
                serde_json::from_str::<AudioBackendFailureKind>(expected_json)
                    .expect("deserialize"),
                kind
            );
        }
    }

    #[test]
    fn audio_backend_readiness_json_wire_names_are_stable() {
        let cases = [
            (
                AudioBackendReadiness::ControlPlaneOnly,
                "\"controlPlaneOnly\"",
            ),
            (AudioBackendReadiness::PlannedNative, "\"plannedNative\""),
            (AudioBackendReadiness::Unsupported, "\"unsupported\""),
        ];

        for (readiness, expected_json) in cases {
            assert_eq!(
                serde_json::to_string(&readiness).expect("serialize"),
                expected_json
            );
            assert_eq!(
                serde_json::from_str::<AudioBackendReadiness>(expected_json).expect("deserialize"),
                readiness
            );
        }
    }

    #[test]
    fn microphone_injection_state_serializes_explicit_request_state() {
        let state = MicrophoneInjectionState {
            requested: true,
            active: false,
            readiness: AudioBackendReadiness::PlannedNative,
            reason: Some("server-side microphone injection backend is not implemented yet".into()),
        };

        assert_eq!(
            serde_json::to_value(&state).expect("serialize"),
            serde_json::json!({
                "requested": true,
                "active": false,
                "readiness": "plannedNative",
                "reason": "server-side microphone injection backend is not implemented yet"
            })
        );
    }

    #[test]
    fn audio_backend_contract_accepts_legacy_payload_without_statuses() {
        let payload = r#"{
            "controlPlane": "controlPlane",
            "plannedCapture": "pipeWire",
            "plannedPlayback": "pipeWire",
            "plannedMicrophone": "pipeWire",
            "readiness": "controlPlaneOnly",
            "notes": []
        }"#;

        let contract =
            serde_json::from_str::<AudioBackendContract>(payload).expect("deserialize contract");

        assert!(contract.statuses.is_empty());
    }

    #[test]
    fn audio_stream_session_accepts_legacy_payload_without_backend() {
        let payload = r#"{
            "id": "audio-stream-1",
            "sessionId": "session-1",
            "selectedWindowId": "window-1",
            "source": {
                "scope": "selectedApplication",
                "selectedWindowId": "window-1",
                "applicationId": "terminal",
                "title": "Terminal"
            },
            "devices": {
                "outputDeviceId": null,
                "inputDeviceId": null
            },
            "microphone": "disabled",
            "mute": {
                "systemAudioMuted": false,
                "microphoneMuted": true
            },
            "capabilities": {
                "systemAudio": { "supported": true, "reason": null },
                "microphoneCapture": { "supported": true, "reason": null },
                "microphoneInjection": { "supported": false, "reason": "not implemented" },
                "echoCancellation": { "supported": true, "reason": null },
                "deviceSelection": { "supported": true, "reason": null }
            },
            "stats": {
                "packetsSent": 0,
                "packetsReceived": 0,
                "latencyMs": 0
            },
            "health": {
                "healthy": true,
                "message": "audio stream started"
            },
            "state": "streaming"
        }"#;

        let session =
            serde_json::from_str::<AudioStreamSession>(payload).expect("deserialize session");

        assert_eq!(session.backend, None);
        assert_eq!(
            session.microphone_injection,
            MicrophoneInjectionState::default()
        );
    }
}
