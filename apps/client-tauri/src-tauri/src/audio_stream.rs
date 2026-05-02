use apprelay_protocol::{
    AudioStreamSession, ControlAuth, StartAudioStreamRequest, StopAudioStreamRequest,
    UpdateAudioStreamRequest,
};

use crate::{with_control_plane, with_control_plane_events};

#[tauri::command]
pub fn active_audio_streams(
    auth_token: String,
    client_id: String,
) -> Result<Vec<AudioStreamSession>, String> {
    with_control_plane(|control_plane| {
        control_plane.active_audio_streams(&paired_auth(auth_token, client_id))
    })
}

#[tauri::command]
pub fn start_audio_stream(
    auth_token: String,
    client_id: String,
    request: StartAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.start_audio_stream_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

#[tauri::command]
pub fn stop_audio_stream(
    auth_token: String,
    client_id: String,
    request: StopAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.stop_audio_stream_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

#[tauri::command]
pub fn update_audio_stream(
    auth_token: String,
    client_id: String,
    request: UpdateAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.update_audio_stream_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

#[tauri::command]
pub fn audio_stream_status(
    auth_token: String,
    client_id: String,
    stream_id: String,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.audio_stream_status(&paired_auth(auth_token, client_id), &stream_id)
    })
}

fn paired_auth(auth_token: String, client_id: String) -> ControlAuth {
    ControlAuth::with_client_id(auth_token, client_id)
}
