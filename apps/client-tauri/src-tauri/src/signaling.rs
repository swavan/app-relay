use apprelay_protocol::{
    PollSignalingRequest, SignalingPoll, SignalingSubmitAck, SubmitSignalingRequest,
};

use crate::{paired_auth, with_control_plane_events};

#[tauri::command]
pub fn submit_sdp_offer(
    auth_token: String,
    client_id: String,
    request: SubmitSignalingRequest,
) -> Result<SignalingSubmitAck, String> {
    submit_signaling(auth_token, client_id, request)
}

#[tauri::command]
pub fn submit_sdp_answer(
    auth_token: String,
    client_id: String,
    request: SubmitSignalingRequest,
) -> Result<SignalingSubmitAck, String> {
    submit_signaling(auth_token, client_id, request)
}

#[tauri::command]
pub fn submit_ice_candidate(
    auth_token: String,
    client_id: String,
    request: SubmitSignalingRequest,
) -> Result<SignalingSubmitAck, String> {
    submit_signaling(auth_token, client_id, request)
}

#[tauri::command]
pub fn signal_end_of_candidates(
    auth_token: String,
    client_id: String,
    request: SubmitSignalingRequest,
) -> Result<SignalingSubmitAck, String> {
    submit_signaling(auth_token, client_id, request)
}

#[tauri::command]
pub fn poll_signaling(
    auth_token: String,
    client_id: String,
    request: PollSignalingRequest,
) -> Result<SignalingPoll, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.poll_signaling_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}

fn submit_signaling(
    auth_token: String,
    client_id: String,
    request: SubmitSignalingRequest,
) -> Result<SignalingSubmitAck, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane.submit_signaling_with_audit(
            &paired_auth(auth_token, client_id),
            request,
            events,
        )
    })
}
