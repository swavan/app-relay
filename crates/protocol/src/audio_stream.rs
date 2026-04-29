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
