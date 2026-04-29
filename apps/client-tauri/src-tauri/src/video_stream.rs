use swavan_protocol::{
    ControlAuth, StartVideoStreamRequest, StopVideoStreamRequest, VideoStreamSession,
    VideoStreamSignaling, VideoStreamSignalingKind, VideoStreamState, VideoStreamStats,
};

use crate::{with_control_plane, ViewportSizeDto};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartVideoStreamRequestDto {
    pub session_id: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopVideoStreamRequestDto {
    pub stream_id: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamSessionDto {
    pub id: String,
    pub session_id: String,
    pub selected_window_id: String,
    pub viewport: ViewportSizeDto,
    pub signaling: VideoStreamSignalingDto,
    pub stats: VideoStreamStatsDto,
    pub state: String,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamSignalingDto {
    pub kind: String,
    pub offer: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStreamStatsDto {
    pub frames_encoded: u64,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
}

#[tauri::command]
pub fn start_video_stream(
    auth_token: String,
    request: StartVideoStreamRequestDto,
) -> Result<VideoStreamSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .start_video_stream(&ControlAuth::new(auth_token), request.into())
            .map(Into::into)
    })
}

#[tauri::command]
pub fn stop_video_stream(
    auth_token: String,
    request: StopVideoStreamRequestDto,
) -> Result<VideoStreamSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .stop_video_stream(&ControlAuth::new(auth_token), request.into())
            .map(Into::into)
    })
}

#[tauri::command]
pub fn video_stream_status(
    auth_token: String,
    stream_id: String,
) -> Result<VideoStreamSessionDto, String> {
    with_control_plane(|control_plane| {
        control_plane
            .video_stream_status(&ControlAuth::new(auth_token), &stream_id)
            .map(Into::into)
    })
}

impl From<StartVideoStreamRequestDto> for StartVideoStreamRequest {
    fn from(request: StartVideoStreamRequestDto) -> Self {
        Self {
            session_id: request.session_id,
        }
    }
}

impl From<StopVideoStreamRequestDto> for StopVideoStreamRequest {
    fn from(request: StopVideoStreamRequestDto) -> Self {
        Self {
            stream_id: request.stream_id,
        }
    }
}

impl From<VideoStreamSession> for VideoStreamSessionDto {
    fn from(stream: VideoStreamSession) -> Self {
        Self {
            id: stream.id,
            session_id: stream.session_id,
            selected_window_id: stream.selected_window_id,
            viewport: stream.viewport.into(),
            signaling: stream.signaling.into(),
            stats: stream.stats.into(),
            state: video_stream_state_name(&stream.state).to_string(),
            failure_reason: stream.failure_reason,
        }
    }
}

impl From<VideoStreamSignaling> for VideoStreamSignalingDto {
    fn from(signaling: VideoStreamSignaling) -> Self {
        Self {
            kind: video_stream_signaling_kind_name(&signaling.kind).to_string(),
            offer: signaling.offer,
        }
    }
}

impl From<VideoStreamStats> for VideoStreamStatsDto {
    fn from(stats: VideoStreamStats) -> Self {
        Self {
            frames_encoded: stats.frames_encoded,
            bitrate_kbps: stats.bitrate_kbps,
            latency_ms: stats.latency_ms,
        }
    }
}

fn video_stream_state_name(state: &VideoStreamState) -> &'static str {
    match state {
        VideoStreamState::Starting => "starting",
        VideoStreamState::Streaming => "streaming",
        VideoStreamState::Stopped => "stopped",
        VideoStreamState::Failed => "failed",
    }
}

fn video_stream_signaling_kind_name(kind: &VideoStreamSignalingKind) -> &'static str {
    match kind {
        VideoStreamSignalingKind::WebRtcOffer => "webRtcOffer",
    }
}
