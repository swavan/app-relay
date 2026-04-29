use apprelay_protocol::{
    ControlAuth, ReconnectVideoStreamRequest, StartVideoStreamRequest, StopVideoStreamRequest,
    VideoStreamSession,
};

use crate::with_control_plane;

#[tauri::command]
pub fn start_video_stream(
    auth_token: String,
    request: StartVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.start_video_stream(&ControlAuth::new(auth_token), request)
    })
}

#[tauri::command]
pub fn stop_video_stream(
    auth_token: String,
    request: StopVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.stop_video_stream(&ControlAuth::new(auth_token), request)
    })
}

#[tauri::command]
pub fn reconnect_video_stream(
    auth_token: String,
    request: ReconnectVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.reconnect_video_stream(&ControlAuth::new(auth_token), request)
    })
}

#[tauri::command]
pub fn video_stream_status(
    auth_token: String,
    stream_id: String,
) -> Result<VideoStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.video_stream_status(&ControlAuth::new(auth_token), &stream_id)
    })
}
