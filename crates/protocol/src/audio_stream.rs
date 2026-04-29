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
pub struct AudioBackendContract {
    pub control_plane: AudioBackendKind,
    pub planned_capture: AudioBackendKind,
    pub planned_playback: AudioBackendKind,
    pub planned_microphone: AudioBackendKind,
    pub readiness: AudioBackendReadiness,
    pub notes: Vec<String>,
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
    }
}
