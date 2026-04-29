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
pub struct NegotiateVideoStreamRequest {
    pub stream_id: String,
    pub client_answer: WebRtcSessionDescription,
    pub client_ice_candidates: Vec<WebRtcIceCandidate>,
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
    pub negotiation_state: VideoStreamNegotiationState,
    pub offer: Option<WebRtcSessionDescription>,
    pub answer: Option<WebRtcSessionDescription>,
    pub ice_candidates: Vec<WebRtcIceCandidate>,
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
pub enum VideoStreamNegotiationState {
    AwaitingAnswer,
    Negotiated,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcSessionDescription {
    pub sdp_type: WebRtcSdpType,
    pub sdp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WebRtcSdpType {
    Offer,
    Answer,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcIceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_m_line_index: Option<u16>,
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
                negotiation_state: VideoStreamNegotiationState::AwaitingAnswer,
                offer: Some(WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Offer,
                    sdp: "offer".to_string(),
                }),
                answer: None,
                ice_candidates: vec![WebRtcIceCandidate {
                    candidate: "candidate".to_string(),
                    sdp_mid: Some("video".to_string()),
                    sdp_m_line_index: Some(0),
                }],
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
