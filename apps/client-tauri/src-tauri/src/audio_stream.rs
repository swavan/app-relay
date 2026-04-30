use apprelay_protocol::{
    AudioStreamSession, ControlAuth, StartAudioStreamRequest, StopAudioStreamRequest,
    UpdateAudioStreamRequest,
};

use crate::with_control_plane;

#[tauri::command]
pub fn start_audio_stream(
    auth_token: String,
    client_id: String,
    request: StartAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.start_audio_stream(&paired_auth(auth_token, client_id), request)
    })
}

#[tauri::command]
pub fn stop_audio_stream(
    auth_token: String,
    client_id: String,
    request: StopAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.stop_audio_stream(&paired_auth(auth_token, client_id), request)
    })
}

#[tauri::command]
pub fn update_audio_stream(
    auth_token: String,
    client_id: String,
    request: UpdateAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.update_audio_stream(&paired_auth(auth_token, client_id), request)
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
