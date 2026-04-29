use apprelay_core::ServerConfig;
use apprelay_protocol::{
    AppRelayError, AudioStreamState, ButtonAction, ClientPoint, ControlAuth, ControlError,
    CreateSessionRequest, Feature, ForwardInputRequest, InputDeliveryStatus, InputEvent,
    MappedInputEvent, MicrophoneMode, NegotiateVideoStreamRequest, Platform, PointerButton,
    ReconnectVideoStreamRequest, ResizeSessionRequest, ServerPoint, ServerVersion, SessionState,
    StartAudioStreamRequest, StartVideoStreamRequest, StopAudioStreamRequest,
    StopVideoStreamRequest, UpdateAudioStreamRequest, VideoEncodingPipelineState,
    VideoResolutionAdaptationReason, VideoStreamFailureKind, VideoStreamNegotiationState,
    VideoStreamRecoveryAction, VideoStreamState, ViewportSize, WebRtcIceCandidate, WebRtcSdpType,
    WebRtcSessionDescription,
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
fn control_plane_reports_desktop_audio_capabilities() {
    for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
        let control_plane = ServerControlPlane::new(
            ServerServices::new(platform, "integration-test"),
            ServerConfig::local("correct-token"),
        );
        let capabilities = control_plane
            .capabilities(&ControlAuth::new("correct-token"))
            .expect("authorized capabilities response");

        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::SystemAudioStream && capability.supported
        }));
        assert!(capabilities.iter().any(|capability| {
            capability.feature == Feature::ClientMicrophoneInput && capability.supported
        }));
    }
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

#[test]
fn control_plane_manages_audio_stream_independently_from_video() {
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
    let video = control_plane
        .start_video_stream(
            &auth,
            StartVideoStreamRequest {
                session_id: session.id.clone(),
            },
        )
        .expect("start video stream");

    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id.clone(),
                microphone: MicrophoneMode::Enabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: Some("speakers".to_string()),
                input_device_id: Some("mic".to_string()),
            },
        )
        .expect("start audio stream");

    assert_eq!(audio.session_id, session.id);
    assert_eq!(audio.selected_window_id, session.selected_window.id);
    assert_eq!(audio.microphone, MicrophoneMode::Enabled);
    assert!(!audio.mute.system_audio_muted);
    assert!(audio.mute.microphone_muted);
    assert!(audio.capabilities.system_audio.supported);
    assert!(audio.capabilities.microphone_capture.supported);
    assert_eq!(audio.state, AudioStreamState::Streaming);
    assert_eq!(
        control_plane.video_stream_status(&auth, &video.id),
        Ok(video)
    );

    let stopped = control_plane
        .stop_audio_stream(
            &auth,
            StopAudioStreamRequest {
                stream_id: audio.id,
            },
        )
        .expect("stop audio stream");

    assert_eq!(stopped.state, AudioStreamState::Stopped);
}

#[test]
fn control_plane_starts_audio_stream_on_desktop_platforms() {
    for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
        let mut control_plane = ServerControlPlane::new(
            ServerServices::new(platform, "integration-test"),
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

        let audio = control_plane
            .start_audio_stream(
                &auth,
                StartAudioStreamRequest {
                    session_id: session.id,
                    microphone: MicrophoneMode::Enabled,
                    system_audio_muted: false,
                    microphone_muted: true,
                    output_device_id: None,
                    input_device_id: None,
                },
            )
            .expect("start audio stream");

        assert_eq!(audio.state, AudioStreamState::Streaming);
        assert!(audio.capabilities.system_audio.supported);
        assert!(audio.capabilities.microphone_capture.supported);
    }
}

#[test]
fn control_plane_updates_audio_mute_and_devices() {
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
    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id.clone(),
                microphone: MicrophoneMode::Disabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: None,
                input_device_id: None,
            },
        )
        .expect("start audio stream");

    let updated = control_plane
        .update_audio_stream(
            &auth,
            UpdateAudioStreamRequest {
                stream_id: audio.id.clone(),
                system_audio_muted: true,
                microphone_muted: true,
                output_device_id: Some("headphones".to_string()),
                input_device_id: None,
            },
        )
        .expect("update audio stream");

    assert!(updated.mute.system_audio_muted);
    assert!(updated.mute.microphone_muted);
    assert_eq!(
        updated.devices.output_device_id.as_deref(),
        Some("headphones")
    );
    assert_eq!(
        control_plane.audio_stream_status(&auth, &audio.id),
        Ok(updated)
    );
}

#[test]
fn control_plane_stops_audio_stream_when_session_closes() {
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
    let audio = control_plane
        .start_audio_stream(
            &auth,
            StartAudioStreamRequest {
                session_id: session.id.clone(),
                microphone: MicrophoneMode::Disabled,
                system_audio_muted: false,
                microphone_muted: true,
                output_device_id: None,
                input_device_id: None,
            },
        )
        .expect("start audio stream");

    control_plane
        .close_session(&auth, &session.id)
        .expect("close session");

    let status = control_plane
        .audio_stream_status(&auth, &audio.id)
        .expect("audio stream status");
    assert_eq!(status.state, AudioStreamState::Stopped);
    assert_eq!(
        status.health.message.as_deref(),
        Some("application session session-1 closed")
    );
}
