use apprelay_protocol::{
    AudioStreamSession, ControlAuth, StartAudioStreamRequest, StopAudioStreamRequest,
    UpdateAudioStreamRequest,
};

use crate::with_control_plane;

#[tauri::command]
pub fn start_audio_stream(
    auth_token: String,
    request: StartAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.start_audio_stream(&ControlAuth::new(auth_token), request)
    })
}

#[tauri::command]
pub fn stop_audio_stream(
    auth_token: String,
    request: StopAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.stop_audio_stream(&ControlAuth::new(auth_token), request)
    })
}

#[tauri::command]
pub fn update_audio_stream(
    auth_token: String,
    request: UpdateAudioStreamRequest,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.update_audio_stream(&ControlAuth::new(auth_token), request)
    })
}

#[tauri::command]
pub fn audio_stream_status(
    auth_token: String,
    stream_id: String,
) -> Result<AudioStreamSession, String> {
    with_control_plane(|control_plane| {
        control_plane.audio_stream_status(&ControlAuth::new(auth_token), &stream_id)
    })
}
