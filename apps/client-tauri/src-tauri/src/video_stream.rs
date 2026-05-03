use apprelay_protocol::{
    NegotiateVideoStreamRequest, ReconnectVideoStreamRequest, StartVideoStreamRequest,
    StopVideoStreamRequest, VideoStreamSession,
};

use crate::{paired_auth, with_control_plane, with_control_plane_events};

#[tauri::command]
pub fn active_video_streams(
    auth_token: String,
    client_id: String,
) -> Result<Vec<VideoStreamSession>, String> {
    with_control_plane(|control_plane| {
        control_plane.active_video_streams(&paired_auth(auth_token, client_id))
    })
}

#[tauri::command]
pub fn start_video_stream(
    auth_token: String,
    client_id: String,
    request: StartVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.start_video_stream_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

#[tauri::command]
pub fn stop_video_stream(
    auth_token: String,
    client_id: String,
    request: StopVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.stop_video_stream_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

#[tauri::command]
pub fn reconnect_video_stream(
    auth_token: String,
    client_id: String,
    request: ReconnectVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.reconnect_video_stream_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

#[tauri::command]
pub fn negotiate_video_stream(
    auth_token: String,
    client_id: String,
    request: NegotiateVideoStreamRequest,
) -> Result<VideoStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.negotiate_video_stream(&paired_auth(auth_token, client_id), request)
    })
}

#[tauri::command]
pub fn video_stream_status(
    auth_token: String,
    client_id: String,
    stream_id: String,
) -> Result<VideoStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.video_stream_status(&paired_auth(auth_token, client_id), &stream_id)
    })
}
