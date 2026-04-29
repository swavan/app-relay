use crate::ViewportSize;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartVideoStreamRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopVideoStreamRequest {
    pub stream_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectVideoStreamRequest {
    pub stream_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamSession {
    pub id: String,
    pub session_id: String,
    pub selected_window_id: String,
    pub viewport: ViewportSize,
    pub capture_source: VideoCaptureSource,
    pub signaling: VideoStreamSignaling,
    pub stats: VideoStreamStats,
    pub health: VideoStreamHealth,
    pub state: VideoStreamState,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamSignaling {
    pub kind: VideoStreamSignalingKind,
    pub offer: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCaptureSource {
    pub scope: VideoCaptureScope,
    pub selected_window_id: String,
    pub application_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoCaptureScope {
    SelectedWindow,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoStreamSignalingKind {
    WebRtcOffer,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamStats {
    pub frames_encoded: u64,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
    pub reconnect_attempts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamHealth {
    pub healthy: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoStreamState {
    Starting,
    Streaming,
    Stopped,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_stream_session_tracks_selected_window_state() {
        let stream = VideoStreamSession {
            id: "stream-1".to_string(),
            session_id: "session-1".to_string(),
            selected_window_id: "window-session-1".to_string(),
            viewport: ViewportSize::new(1280, 720),
            capture_source: VideoCaptureSource {
                scope: VideoCaptureScope::SelectedWindow,
                selected_window_id: "window-session-1".to_string(),
                application_id: "terminal".to_string(),
                title: "Terminal".to_string(),
            },
            signaling: VideoStreamSignaling {
                kind: VideoStreamSignalingKind::WebRtcOffer,
                offer: Some("offer".to_string()),
            },
            stats: VideoStreamStats {
                frames_encoded: 0,
                bitrate_kbps: 0,
                latency_ms: 0,
                reconnect_attempts: 0,
            },
            health: VideoStreamHealth {
                healthy: true,
                message: None,
            },
            state: VideoStreamState::Starting,
            failure_reason: None,
        };

        assert_eq!(stream.session_id, "session-1");
        assert_eq!(stream.selected_window_id, "window-session-1");
    }
}
