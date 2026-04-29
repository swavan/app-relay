use apprelay_core::ServerConfig;
use apprelay_protocol::{
    AppRelayError, ButtonAction, ClientPoint, ControlAuth, ControlError, CreateSessionRequest,
    Feature, ForwardInputRequest, InputDeliveryStatus, InputEvent, MappedInputEvent,
    NegotiateVideoStreamRequest, Platform, PointerButton, ReconnectVideoStreamRequest,
    ResizeSessionRequest, ServerPoint, ServerVersion, SessionState, StartVideoStreamRequest,
    StopVideoStreamRequest, VideoEncodingPipelineState, VideoResolutionAdaptationReason,
    VideoStreamFailureKind, VideoStreamNegotiationState, VideoStreamRecoveryAction,
    VideoStreamState, ViewportSize, WebRtcIceCandidate, WebRtcSdpType, WebRtcSessionDescription,
};
use apprelay_server::{ServerControlPlane, ServerServices};

#[test]
fn control_plane_rejects_unauthorized_requests() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.version(&ControlAuth::new("wrong-token")),
        Err(ControlError::Unauthorized)
    );
}

#[test]
fn control_plane_exposes_version_for_authorized_requests() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Macos, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.version(&ControlAuth::new("correct-token")),
        Ok(ServerVersion::new(
            "apprelay-server",
            "integration-test",
            Platform::Macos
        ))
    );
}

#[test]
fn control_plane_reports_linux_app_discovery_capability() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let capabilities = control_plane
        .capabilities(&ControlAuth::new("correct-token"))
        .expect("authorized capabilities response");

    assert!(capabilities
        .iter()
        .any(|capability| capability.feature == Feature::AppDiscovery && capability.supported));
}

#[test]
fn control_plane_reports_macos_app_discovery_capability() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Macos, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let capabilities = control_plane
        .capabilities(&ControlAuth::new("correct-token"))
        .expect("authorized capabilities response");

    assert!(capabilities
        .iter()
        .any(|capability| capability.feature == Feature::AppDiscovery && capability.supported));
}

#[test]
fn control_plane_maps_unsupported_application_discovery_errors() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Windows, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.available_applications(&ControlAuth::new("correct-token")),
        Err(ControlError::Service(AppRelayError::unsupported(
            Platform::Windows,
            Feature::AppDiscovery
        )))
    );
}

#[test]
fn control_plane_heartbeat_supports_reconnect_checks() {
    let control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");

    let first = control_plane
        .heartbeat(&auth)
        .expect("first heartbeat response");
    let second = control_plane
        .heartbeat(&auth)
        .expect("second heartbeat response");

    assert!(first.healthy);
    assert!(second.healthy);
    assert_eq!(first.sequence + 1, second.sequence);
}

#[test]
fn control_plane_manages_application_session_lifecycle() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");

    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let resized = control_plane
        .resize_session(
            &auth,
            ResizeSessionRequest {
                session_id: session.id.clone(),
                viewport: ViewportSize::new(1440, 900),
            },
        )
        .expect("resize session");
    let closed = control_plane
        .close_session(&auth, &session.id)
        .expect("close session");

    assert_eq!(resized.viewport, ViewportSize::new(1440, 900));
    assert_eq!(closed.state, SessionState::Closed);
    assert_eq!(control_plane.active_sessions(&auth), Ok(Vec::new()));
}

#[test]
fn control_plane_authorizes_and_forwards_input_to_session() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1920, 1080),
            },
        )
        .expect("create session");

    let focused = control_plane
        .forward_input(
            &auth,
            ForwardInputRequest {
                session_id: session.id.clone(),
                client_viewport: ViewportSize::new(960, 540),
                event: InputEvent::Focus,
            },
        )
        .expect("focus input");
    let clicked = control_plane
        .forward_input(
            &auth,
            ForwardInputRequest {
                session_id: session.id,
                client_viewport: ViewportSize::new(960, 540),
                event: InputEvent::PointerButton {
                    position: ClientPoint::new(480.0, 270.0),
                    button: PointerButton::Primary,
                    action: ButtonAction::Press,
                },
            },
        )
        .expect("forward input");

    assert_eq!(focused.status, InputDeliveryStatus::Focused);
    assert_eq!(
        clicked.mapped_event,
        MappedInputEvent::PointerButton {
            position: ServerPoint::new(960, 540),
            button: PointerButton::Primary,
            action: ButtonAction::Press,
        }
    );
    assert_eq!(clicked.status, InputDeliveryStatus::Delivered);
}

#[test]
fn control_plane_rejects_unauthorized_input_requests() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.forward_input(
            &ControlAuth::new("wrong-token"),
            ForwardInputRequest {
                session_id: "session-1".to_string(),
                client_viewport: ViewportSize::new(1280, 720),
                event: InputEvent::Focus,
            },
        ),
        Err(ControlError::Unauthorized)
    );
}

#[test]
fn control_plane_rejects_input_for_unknown_session() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );

    assert_eq!(
        control_plane.forward_input(
            &ControlAuth::new("correct-token"),
            ForwardInputRequest {
                session_id: "session-unknown".to_string(),
                client_viewport: ViewportSize::new(1280, 720),
                event: InputEvent::Focus,
            },
        ),
        Err(ControlError::Service(AppRelayError::PermissionDenied(
            "input is not authorized for session session-unknown".to_string()
        )))
    );
}

#[test]
fn control_plane_manages_video_stream_lifecycle() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");

    let stream = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    assert_eq!(stream.session_id, session.id);
    assert_eq!(stream.selected_window_id, session.selected_window.id);
    assert_eq!(stream.state, VideoStreamState::Starting);
    assert_eq!(
        stream.encoding.state,
        VideoEncodingPipelineState::Configured
    );
    assert_eq!(
        stream.encoding.contract.target.resolution,
        ViewportSize::new(1280, 720)
    );
    assert_eq!(
        stream.encoding.contract.adaptation.current_target,
        ViewportSize::new(1280, 720)
    );
    assert_eq!(
        stream.encoding.contract.adaptation.reason,
        VideoResolutionAdaptationReason::MatchesViewport
    );
    assert_eq!(
        control_plane.video_stream_status(&auth, &stream.id),
        Ok(stream.clone())
    );

    let stopped = control_plane
        .stop_video_stream(
            &auth,
            StopVideoStreamRequest {
                stream_id: stream.id,
            },
        )
        .expect("stop video stream");

    assert_eq!(stopped.state, VideoStreamState::Stopped);
    assert_eq!(stopped.encoding.state, VideoEncodingPipelineState::Drained);
}

#[test]
fn control_plane_negotiates_video_stream() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let stream = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    let negotiated = control_plane
        .negotiate_video_stream(
            &auth,
            NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: vec![WebRtcIceCandidate {
                    candidate: "candidate:client stream-1 typ host".to_string(),
                    sdp_mid: Some("video".to_string()),
                    sdp_m_line_index: Some(0),
                }],
            },
        )
        .expect("negotiate video stream");

    assert_eq!(negotiated.state, VideoStreamState::Streaming);
    assert_eq!(
        negotiated.encoding.state,
        VideoEncodingPipelineState::Encoding
    );
    assert_eq!(negotiated.encoding.output.frames_encoded, 1);
    assert_eq!(negotiated.encoding.output.keyframes_encoded, 1);
    assert_eq!(negotiated.stats.frames_encoded, 1);
    assert_eq!(negotiated.stats.bitrate_kbps, 2764);
    assert_eq!(
        negotiated.signaling.negotiation_state,
        VideoStreamNegotiationState::Negotiated
    );
    assert_eq!(
        negotiated.signaling.answer,
        Some(WebRtcSessionDescription {
            sdp_type: WebRtcSdpType::Answer,
            sdp: "client-answer".to_string(),
        })
    );
    assert_eq!(negotiated.signaling.ice_candidates.len(), 2);
}

#[test]
fn control_plane_reconnects_and_resizes_video_stream() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let stream = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    let reconnected = control_plane
        .reconnect_video_stream(
            &auth,
            ReconnectVideoStreamRequest {
                stream_id: stream.id.clone(),
            },
        )
        .expect("reconnect video stream");
    assert_eq!(reconnected.stats.reconnect_attempts, 1);

    control_plane
        .resize_session(
            &auth,
            ResizeSessionRequest {
                session_id: session.id,
                viewport: ViewportSize::new(2560, 1440),
            },
        )
        .expect("resize session");

    let status = control_plane
        .video_stream_status(&auth, &stream.id)
        .expect("stream status");
    assert_eq!(status.viewport, ViewportSize::new(2560, 1440));
    assert_eq!(
        status.encoding.contract.target.resolution,
        ViewportSize::new(1920, 1080)
    );
    assert_eq!(status.encoding.contract.target.target_bitrate_kbps, 6220);
    assert_eq!(
        status.encoding.contract.adaptation.requested_viewport,
        ViewportSize::new(2560, 1440)
    );
    assert_eq!(
        status.encoding.contract.adaptation.reason,
        VideoResolutionAdaptationReason::CappedToLimits
    );
    assert_eq!(
        status.health.message.as_deref(),
        Some("stream viewport updated")
    );
}

#[test]
fn control_plane_keeps_negotiated_video_encoding_coherent_after_resize() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let stream = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    control_plane
        .negotiate_video_stream(
            &auth,
            NegotiateVideoStreamRequest {
                stream_id: stream.id.clone(),
                client_answer: WebRtcSessionDescription {
                    sdp_type: WebRtcSdpType::Answer,
                    sdp: "client-answer".to_string(),
                },
                client_ice_candidates: Vec::new(),
            },
        )
        .expect("negotiate video stream");

    control_plane
        .resize_session(
            &auth,
            ResizeSessionRequest {
                session_id: session.id,
                viewport: ViewportSize::new(2560, 1440),
            },
        )
        .expect("resize session");

    let status = control_plane
        .video_stream_status(&auth, &stream.id)
        .expect("stream status");
    assert_eq!(status.state, VideoStreamState::Streaming);
    assert_eq!(status.encoding.state, VideoEncodingPipelineState::Encoding);
    assert_eq!(status.viewport, ViewportSize::new(2560, 1440));
    assert_eq!(
        status.encoding.contract.target.resolution,
        ViewportSize::new(1920, 1080)
    );
    assert_eq!(
        status.encoding.contract.adaptation.reason,
        VideoResolutionAdaptationReason::CappedToLimits
    );
    assert_eq!(status.stats.bitrate_kbps, 6220);
}

#[test]
fn control_plane_marks_stream_failed_when_session_closes() {
    let mut control_plane = ServerControlPlane::new(
        ServerServices::new(Platform::Linux, "integration-test"),
        ServerConfig::local("correct-token"),
    );
    let auth = ControlAuth::new("correct-token");
    let session = control_plane
        .create_session(
            &auth,
            CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            },
        )
        .expect("create session");
    let stream = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    control_plane
        .close_session(&auth, &session.id)
        .expect("close session");

    let status = control_plane
        .video_stream_status(&auth, &stream.id)
        .expect("stream status");
    assert_eq!(status.state, VideoStreamState::Failed);
    assert_eq!(status.encoding.state, VideoEncodingPipelineState::Drained);
    assert_eq!(status.stats.bitrate_kbps, 0);
    assert_eq!(status.stats.latency_ms, 0);
    assert_eq!(
        status.failure.as_ref().map(|failure| &failure.kind),
        Some(&VideoStreamFailureKind::AppClosed)
    );
    assert_eq!(
        status
            .failure
            .as_ref()
            .map(|failure| &failure.recovery.action),
        Some(&VideoStreamRecoveryAction::RestartApplicationSession)
    );
    assert_eq!(
        control_plane.reconnect_video_stream(
            &auth,
            ReconnectVideoStreamRequest {
                stream_id: stream.id
            }
        ),
        Err(apprelay_protocol::ControlError::Service(
            AppRelayError::InvalidRequest(
                "stream stream-1 cannot reconnect because its application session closed"
                    .to_string()
            )
        ))
    );
}
