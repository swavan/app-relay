use crate::ViewportSize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartVideoStreamRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StopVideoStreamRequest {
    pub stream_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoStreamSession {
    pub id: String,
    pub session_id: String,
    pub selected_window_id: String,
    pub viewport: ViewportSize,
    pub signaling: VideoStreamSignaling,
    pub stats: VideoStreamStats,
    pub state: VideoStreamState,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoStreamSignaling {
    pub kind: VideoStreamSignalingKind,
    pub offer: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoStreamSignalingKind {
    WebRtcOffer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoStreamStats {
    pub frames_encoded: u64,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
            signaling: VideoStreamSignaling {
                kind: VideoStreamSignalingKind::WebRtcOffer,
                offer: Some("offer".to_string()),
            },
            stats: VideoStreamStats {
                frames_encoded: 0,
                bitrate_kbps: 0,
                latency_ms: 0,
            },
            state: VideoStreamState::Starting,
            failure_reason: None,
        };

        assert_eq!(stream.session_id, "session-1");
        assert_eq!(stream.selected_window_id, "window-session-1");
    }
}
