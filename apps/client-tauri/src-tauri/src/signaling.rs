use apprelay_protocol::{
    ControlAuth, IceCandidatePayload, PollSignalingRequest, SdpRole, SignalingDirection,
    SignalingEnvelope, SignalingMessage, SignalingPoll, SignalingSubmitAck, SubmitSignalingRequest,
};

use crate::with_control_plane_events;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingSubmitAckDto {
    pub session_id: String,
    pub direction: String,
    pub sequence: u64,
    pub envelope_kind: String,
    pub payload_byte_length: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingPollDto {
    pub session_id: String,
    pub direction: String,
    pub last_sequence: u64,
    pub messages: Vec<SignalingMessageDto>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingMessageDto {
    pub sequence: u64,
    pub direction: String,
    pub envelope: SignalingEnvelopeDto,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SignalingEnvelopeDto {
    SdpOffer { sdp: String, role: String },
    SdpAnswer { sdp: String },
    IceCandidate(IceCandidatePayloadDto),
    EndOfCandidates,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IceCandidatePayloadDto {
    pub candidate: String,
    pub sdp_mid: String,
    pub sdp_mline_index: u16,
}

#[tauri::command]
pub fn submit_sdp_offer(
    auth_token: String,
    client_id: String,
    session_id: String,
    role: String,
    sdp: String,
) -> Result<SignalingSubmitAckDto, String> {
    let role = SdpRole::parse(&role).ok_or_else(|| "invalid sdp role".to_string())?;
    submit_signaling(
        auth_token,
        client_id,
        SubmitSignalingRequest {
            session_id,
            direction: SignalingDirection::OfferToAnswerer,
            envelope: SignalingEnvelope::SdpOffer { sdp, role },
        },
    )
}

#[tauri::command]
pub fn submit_sdp_answer(
    auth_token: String,
    client_id: String,
    session_id: String,
    sdp: String,
) -> Result<SignalingSubmitAckDto, String> {
    submit_signaling(
        auth_token,
        client_id,
        SubmitSignalingRequest {
            session_id,
            direction: SignalingDirection::AnswererToOfferer,
            envelope: SignalingEnvelope::SdpAnswer { sdp },
        },
    )
}

#[tauri::command]
pub fn submit_ice_candidate(
    auth_token: String,
    client_id: String,
    session_id: String,
    direction: String,
    candidate: String,
    sdp_mid: String,
    sdp_mline_index: u16,
) -> Result<SignalingSubmitAckDto, String> {
    let direction = parse_direction(&direction)?;
    submit_signaling(
        auth_token,
        client_id,
        SubmitSignalingRequest {
            session_id,
            direction,
            envelope: SignalingEnvelope::IceCandidate(IceCandidatePayload {
                candidate,
                sdp_mid,
                sdp_mline_index,
            }),
        },
    )
}

#[tauri::command]
pub fn signal_end_of_candidates(
    auth_token: String,
    client_id: String,
    session_id: String,
    direction: String,
) -> Result<SignalingSubmitAckDto, String> {
    let direction = parse_direction(&direction)?;
    submit_signaling(
        auth_token,
        client_id,
        SubmitSignalingRequest {
            session_id,
            direction,
            envelope: SignalingEnvelope::EndOfCandidates,
        },
    )
}

#[tauri::command]
pub fn poll_signaling(
    auth_token: String,
    client_id: String,
    session_id: String,
    direction: String,
    since_sequence: u64,
) -> Result<SignalingPollDto, String> {
    let direction = parse_direction(&direction)?;
    with_control_plane_events(|control_plane, events| {
        control_plane
            .poll_signaling_with_audit(
                &paired_auth(auth_token, client_id),
                PollSignalingRequest {
                    session_id,
                    direction,
                    since_sequence,
                },
                events,
            )
            .map(SignalingPollDto::from)
    })
}

fn submit_signaling(
    auth_token: String,
    client_id: String,
    request: SubmitSignalingRequest,
) -> Result<SignalingSubmitAckDto, String> {
    with_control_plane_events(|control_plane, events| {
        control_plane
            .submit_signaling_with_audit(&paired_auth(auth_token, client_id), request, events)
            .map(SignalingSubmitAckDto::from)
    })
}

fn parse_direction(value: &str) -> Result<SignalingDirection, String> {
    match value {
        "offerToAnswerer" | "offer-to-answerer" => Ok(SignalingDirection::OfferToAnswerer),
        "answererToOfferer" | "answerer-to-offerer" => Ok(SignalingDirection::AnswererToOfferer),
        _ => Err(format!("invalid signaling direction {value}")),
    }
}

fn paired_auth(auth_token: String, client_id: String) -> ControlAuth {
    ControlAuth::with_client_id(auth_token, client_id)
}

fn direction_label(direction: SignalingDirection) -> String {
    match direction {
        SignalingDirection::OfferToAnswerer => "offerToAnswerer".to_string(),
        SignalingDirection::AnswererToOfferer => "answererToOfferer".to_string(),
    }
}

impl From<SignalingSubmitAck> for SignalingSubmitAckDto {
    fn from(ack: SignalingSubmitAck) -> Self {
        Self {
            session_id: ack.session_id,
            direction: direction_label(ack.direction),
            sequence: ack.sequence,
            envelope_kind: ack.envelope_kind,
            payload_byte_length: ack.payload_byte_length,
        }
    }
}

impl From<SignalingPoll> for SignalingPollDto {
    fn from(poll: SignalingPoll) -> Self {
        Self {
            session_id: poll.session_id,
            direction: direction_label(poll.direction),
            last_sequence: poll.last_sequence,
            messages: poll.messages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SignalingMessage> for SignalingMessageDto {
    fn from(message: SignalingMessage) -> Self {
        Self {
            sequence: message.sequence,
            direction: direction_label(message.direction),
            envelope: message.envelope.into(),
        }
    }
}

impl From<SignalingEnvelope> for SignalingEnvelopeDto {
    fn from(envelope: SignalingEnvelope) -> Self {
        match envelope {
            SignalingEnvelope::SdpOffer { sdp, role } => Self::SdpOffer {
                sdp,
                role: role.label().to_string(),
            },
            SignalingEnvelope::SdpAnswer { sdp } => Self::SdpAnswer { sdp },
            SignalingEnvelope::IceCandidate(payload) => {
                Self::IceCandidate(IceCandidatePayloadDto {
                    candidate: payload.candidate,
                    sdp_mid: payload.sdp_mid,
                    sdp_mline_index: payload.sdp_mline_index,
                })
            }
            SignalingEnvelope::EndOfCandidates => Self::EndOfCandidates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_direction_accepts_camel_case_and_kebab_case() {
        assert!(matches!(
            parse_direction("offerToAnswerer"),
            Ok(SignalingDirection::OfferToAnswerer)
        ));
        assert!(matches!(
            parse_direction("offer-to-answerer"),
            Ok(SignalingDirection::OfferToAnswerer)
        ));
        assert!(matches!(
            parse_direction("answererToOfferer"),
            Ok(SignalingDirection::AnswererToOfferer)
        ));
        assert!(matches!(
            parse_direction("answerer-to-offerer"),
            Ok(SignalingDirection::AnswererToOfferer)
        ));
        assert!(parse_direction("sideways").is_err());
    }

    #[test]
    fn signaling_submit_ack_dto_uses_camel_case_direction() {
        let ack = SignalingSubmitAck {
            session_id: "session-1".to_string(),
            direction: SignalingDirection::OfferToAnswerer,
            sequence: 7,
            envelope_kind: "sdp-offer".to_string(),
            payload_byte_length: 12,
        };
        let dto = SignalingSubmitAckDto::from(ack);
        assert_eq!(dto.direction, "offerToAnswerer");
        assert_eq!(dto.sequence, 7);
        assert_eq!(dto.envelope_kind, "sdp-offer");
        assert_eq!(dto.payload_byte_length, 12);
    }

    #[test]
    fn signaling_envelope_dto_maps_each_envelope_variant() {
        let offer: SignalingEnvelopeDto = SignalingEnvelope::SdpOffer {
            sdp: "offer-sdp".to_string(),
            role: SdpRole::Offerer,
        }
        .into();
        assert!(matches!(offer, SignalingEnvelopeDto::SdpOffer { ref role, .. } if role == "offerer"));

        let answer: SignalingEnvelopeDto = SignalingEnvelope::SdpAnswer {
            sdp: "answer-sdp".to_string(),
        }
        .into();
        assert!(matches!(answer, SignalingEnvelopeDto::SdpAnswer { ref sdp } if sdp == "answer-sdp"));

        let candidate: SignalingEnvelopeDto = SignalingEnvelope::IceCandidate(IceCandidatePayload {
            candidate: "candidate:1".to_string(),
            sdp_mid: "video".to_string(),
            sdp_mline_index: 0,
        })
        .into();
        assert!(matches!(
            candidate,
            SignalingEnvelopeDto::IceCandidate(ref payload) if payload.sdp_mid == "video" && payload.sdp_mline_index == 0
        ));

        let end: SignalingEnvelopeDto = SignalingEnvelope::EndOfCandidates.into();
        assert!(matches!(end, SignalingEnvelopeDto::EndOfCandidates));
    }
}
