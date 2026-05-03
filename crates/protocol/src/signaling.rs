//! Transport-neutral SDP/ICE signaling envelope types.
//!
//! Phase C only carries signaling metadata between the offerer (server)
//! and answerer (client) over the line-based control plane. SDP and ICE
//! candidate strings are stored as base64-decoded plain text. No real
//! WebRTC peer is wired here — Phase D will consume these envelopes.

/// Maximum allowed length, in bytes, of a base64-encoded payload accepted
/// by the control-plane signaling operations. Enforced before decoding.
pub const MAX_SIGNALING_PAYLOAD_BASE64_BYTES: usize = 16 * 1024;

/// Maximum allowed length, in bytes, of the decoded SDP or ICE candidate
/// payload. Enforced after base64 decoding.
pub const MAX_SIGNALING_PAYLOAD_DECODED_BYTES: usize = 12 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SdpRole {
    Offerer,
    Answerer,
}

impl SdpRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Offerer => "offerer",
            Self::Answerer => "answerer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "offerer" => Some(Self::Offerer),
            "answerer" => Some(Self::Answerer),
            _ => None,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Offerer => Self::Answerer,
            Self::Answerer => Self::Offerer,
        }
    }
}

/// Direction of a signaling envelope, relative to a polling client.
///
/// `OfferToAnswerer` carries envelopes the offerer side produced; the
/// answerer side polls for them. `AnswererToOfferer` is the reverse.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignalingDirection {
    OfferToAnswerer,
    AnswererToOfferer,
}

impl SignalingDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::OfferToAnswerer => "offer-to-answerer",
            Self::AnswererToOfferer => "answerer-to-offerer",
        }
    }
}

/// A single signaling message produced by either side of the SDP/ICE
/// negotiation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SignalingEnvelope {
    SdpOffer { sdp: String, role: SdpRole },
    SdpAnswer { sdp: String },
    IceCandidate(IceCandidatePayload),
    EndOfCandidates,
}

impl SignalingEnvelope {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::SdpOffer { .. } => "sdp-offer",
            Self::SdpAnswer { .. } => "sdp-answer",
            Self::IceCandidate(_) => "ice-candidate",
            Self::EndOfCandidates => "end-of-candidates",
        }
    }

    /// Decoded payload size, in bytes, for redacted audit logging.
    /// Returns 0 for envelopes that carry no payload (end-of-candidates).
    pub fn payload_byte_length(&self) -> usize {
        match self {
            Self::SdpOffer { sdp, .. } | Self::SdpAnswer { sdp } => sdp.len(),
            Self::IceCandidate(payload) => payload.candidate.len(),
            Self::EndOfCandidates => 0,
        }
    }

    /// Optional `sdpMid` for redacted audit logging. Returns `None` for
    /// envelopes that do not carry an `sdpMid` (SDP and end-of-candidates).
    pub fn sdp_mid_for_audit(&self) -> Option<&str> {
        match self {
            Self::IceCandidate(payload) => Some(payload.sdp_mid.as_str()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IceCandidatePayload {
    pub candidate: String,
    pub sdp_mid: String,
    pub sdp_mline_index: u16,
}

/// One signaling envelope plus its server-assigned monotonic sequence
/// number and the direction the message travels.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingMessage {
    pub sequence: u64,
    pub direction: SignalingDirection,
    pub envelope: SignalingEnvelope,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitSignalingRequest {
    pub session_id: String,
    pub direction: SignalingDirection,
    pub envelope: SignalingEnvelope,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollSignalingRequest {
    pub session_id: String,
    pub direction: SignalingDirection,
    pub since_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingPoll {
    pub session_id: String,
    pub direction: SignalingDirection,
    pub last_sequence: u64,
    pub messages: Vec<SignalingMessage>,
}

impl SignalingPoll {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingSubmitAck {
    pub session_id: String,
    pub direction: SignalingDirection,
    pub sequence: u64,
    pub envelope_kind: String,
    pub payload_byte_length: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdp_role_parses_known_values() {
        assert_eq!(SdpRole::parse("offerer"), Some(SdpRole::Offerer));
        assert_eq!(SdpRole::parse("answerer"), Some(SdpRole::Answerer));
        assert_eq!(SdpRole::parse("observer"), None);
    }

    #[test]
    fn sdp_role_opposite_round_trips() {
        assert_eq!(SdpRole::Offerer.opposite(), SdpRole::Answerer);
        assert_eq!(SdpRole::Answerer.opposite().opposite(), SdpRole::Answerer);
    }

    #[test]
    fn signaling_envelope_payload_metrics_do_not_expose_raw_bytes() {
        let offer = SignalingEnvelope::SdpOffer {
            sdp: "v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\ns=-\r\n".to_string(),
            role: SdpRole::Offerer,
        };
        let candidate = SignalingEnvelope::IceCandidate(IceCandidatePayload {
            candidate: "candidate:1 1 udp 2113937151 192.0.2.1 51234 typ host".to_string(),
            sdp_mid: "video".to_string(),
            sdp_mline_index: 0,
        });

        let offer_sdp_len = if let SignalingEnvelope::SdpOffer { sdp, .. } = &offer {
            sdp.len()
        } else {
            unreachable!()
        };
        let candidate_len = if let SignalingEnvelope::IceCandidate(payload) = &candidate {
            payload.candidate.len()
        } else {
            unreachable!()
        };

        assert_eq!(offer.kind_label(), "sdp-offer");
        assert_eq!(offer.payload_byte_length(), offer_sdp_len);
        assert_eq!(offer.sdp_mid_for_audit(), None);

        assert_eq!(candidate.kind_label(), "ice-candidate");
        assert_eq!(candidate.payload_byte_length(), candidate_len);
        assert_eq!(candidate.sdp_mid_for_audit(), Some("video"));

        assert_eq!(SignalingEnvelope::EndOfCandidates.payload_byte_length(), 0);
    }
}
