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
    pub encoding: VideoEncodingPipeline,
    pub signaling: VideoStreamSignaling,
    pub stats: VideoStreamStats,
    pub health: VideoStreamHealth,
    pub state: VideoStreamState,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncodingPipeline {
    pub contract: VideoEncodingContract,
    pub state: VideoEncodingPipelineState,
    pub output: VideoEncodingOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncodingContract {
    pub codec: VideoCodec,
    pub pixel_format: VideoPixelFormat,
    pub hardware_acceleration: VideoHardwareAcceleration,
    pub target: VideoEncodingTarget,
    pub adaptation: VideoResolutionAdaptation,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncodingTarget {
    pub resolution: ViewportSize,
    pub max_fps: u32,
    pub target_bitrate_kbps: u32,
    pub keyframe_interval_frames: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResolutionAdaptation {
    pub requested_viewport: ViewportSize,
    pub current_target: ViewportSize,
    pub limits: VideoResolutionLimits,
    pub reason: VideoResolutionAdaptationReason,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResolutionLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoResolutionAdaptationReason {
    MatchesViewport,
    CappedToLimits,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncodingOutput {
    pub frames_submitted: u64,
    pub frames_encoded: u64,
    pub keyframes_encoded: u64,
    pub bytes_produced: u64,
    pub last_frame: Option<EncodedVideoFrame>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedVideoFrame {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub byte_length: u32,
    pub keyframe: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodec {
    H264,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoPixelFormat {
    Rgba,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoHardwareAcceleration {
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoEncodingPipelineState {
    Configured,
    Encoding,
    Drained,
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
            encoding: VideoEncodingPipeline {
                contract: VideoEncodingContract {
                    codec: VideoCodec::H264,
                    pixel_format: VideoPixelFormat::Rgba,
                    hardware_acceleration: VideoHardwareAcceleration::None,
                    target: VideoEncodingTarget {
                        resolution: ViewportSize::new(1280, 720),
                        max_fps: 30,
                        target_bitrate_kbps: 2_764,
                        keyframe_interval_frames: 60,
                    },
                    adaptation: VideoResolutionAdaptation {
                        requested_viewport: ViewportSize::new(1280, 720),
                        current_target: ViewportSize::new(1280, 720),
                        limits: VideoResolutionLimits {
                            max_width: 1920,
                            max_height: 1080,
                            max_pixels: 2_073_600,
                        },
                        reason: VideoResolutionAdaptationReason::MatchesViewport,
                    },
                },
                state: VideoEncodingPipelineState::Configured,
                output: VideoEncodingOutput {
                    frames_submitted: 0,
                    frames_encoded: 0,
                    keyframes_encoded: 0,
                    bytes_produced: 0,
                    last_frame: None,
                },
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
        assert_eq!(stream.encoding.contract.codec, VideoCodec::H264);
        assert_eq!(
            stream.encoding.contract.target.resolution,
            ViewportSize::new(1280, 720)
        );
    }
}
