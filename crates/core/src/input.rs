use apprelay_protocol::{
    AppRelayError, ApplicationSession, ClientPoint, Feature, ForwardInputRequest, InputBackendKind,
    InputDelivery, InputDeliveryStatus, InputEvent, MappedInputEvent, Platform, ServerPoint,
    SessionState, ViewportSize,
};

pub trait InputForwardingService {
    fn forward_input(
        &mut self,
        request: ForwardInputRequest,
        active_sessions: &[ApplicationSession],
    ) -> Result<InputDelivery, AppRelayError>;
}

pub trait InputBackend {
    fn deliver(&self, delivery: InputDelivery) -> Result<InputDelivery, AppRelayError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputBackendService {
    RecordOnly,
    Unsupported {
        platform: Platform,
        kind: InputBackendKind,
    },
}

impl InputBackend for InputBackendService {
    fn deliver(&self, delivery: InputDelivery) -> Result<InputDelivery, AppRelayError> {
        match self {
            Self::RecordOnly => Ok(delivery),
            Self::Unsupported { platform, kind } => {
                let feature = match kind {
                    InputBackendKind::Pointer => Feature::MouseInput,
                    InputBackendKind::Keyboard => Feature::KeyboardInput,
                };
                Err(AppRelayError::unsupported(*platform, feature))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InMemoryInputForwardingService {
    backend: InputBackendService,
    focused_session_id: Option<String>,
    deliveries: Vec<InputDelivery>,
}

impl InMemoryInputForwardingService {
    const MAX_RECORDED_DELIVERIES: usize = 1024;

    pub fn new(backend: InputBackendService) -> Self {
        Self {
            backend,
            focused_session_id: None,
            deliveries: Vec::new(),
        }
    }

    pub fn with_recording_backend() -> Self {
        Self::new(InputBackendService::RecordOnly)
    }

    pub fn deliveries(&self) -> &[InputDelivery] {
        &self.deliveries
    }

    pub fn focused_session_id(&self) -> Option<&str> {
        self.focused_session_id.as_deref()
    }

    pub fn close_session(&mut self, session_id: &str) {
        if self.focused_session_id.as_deref() == Some(session_id) {
            self.focused_session_id = None;
        }
        self.deliveries
            .retain(|delivery| delivery.session_id != session_id);
    }

    fn record_delivery(&mut self, delivery: InputDelivery) {
        if self.deliveries.len() == Self::MAX_RECORDED_DELIVERIES {
            self.deliveries.remove(0);
        }
        self.deliveries.push(delivery);
    }

    fn validate_client_viewport(viewport: &ViewportSize) -> Result<(), AppRelayError> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(AppRelayError::InvalidRequest(
                "client viewport must be non-zero".to_string(),
            ));
        }

        Ok(())
    }

    fn selected_session<'a>(
        request: &ForwardInputRequest,
        active_sessions: &'a [ApplicationSession],
    ) -> Result<&'a ApplicationSession, AppRelayError> {
        active_sessions
            .iter()
            .find(|session| {
                session.id == request.session_id
                    && session.state != SessionState::Closed
                    && session.selected_window.application_id == session.application_id
            })
            .ok_or_else(|| {
                AppRelayError::PermissionDenied(format!(
                    "input is not authorized for session {}",
                    request.session_id
                ))
            })
    }

    fn map_event(
        event: InputEvent,
        client_viewport: &ViewportSize,
        server_viewport: &ViewportSize,
    ) -> Result<MappedInputEvent, AppRelayError> {
        match event {
            InputEvent::Focus => Ok(MappedInputEvent::Focus),
            InputEvent::Blur => Ok(MappedInputEvent::Blur),
            InputEvent::PointerMove { position } => Ok(MappedInputEvent::PointerMove {
                position: map_point(position, client_viewport, server_viewport)?,
            }),
            InputEvent::PointerButton {
                position,
                button,
                action,
            } => Ok(MappedInputEvent::PointerButton {
                position: map_point(position, client_viewport, server_viewport)?,
                button,
                action,
            }),
            InputEvent::PointerScroll {
                position,
                delta_x,
                delta_y,
            } => Ok(MappedInputEvent::PointerScroll {
                position: map_point(position, client_viewport, server_viewport)?,
                delta_x,
                delta_y,
            }),
            InputEvent::PointerDrag { from, to, button } => Ok(MappedInputEvent::PointerDrag {
                from: map_point(from, client_viewport, server_viewport)?,
                to: map_point(to, client_viewport, server_viewport)?,
                button,
            }),
            InputEvent::KeyboardText { text } => {
                if text.is_empty() {
                    return Err(AppRelayError::InvalidRequest(
                        "keyboard text is required".to_string(),
                    ));
                }
                Ok(MappedInputEvent::KeyboardText { text })
            }
            InputEvent::KeyboardKey {
                key,
                action,
                modifiers,
            } => {
                if key.trim().is_empty() {
                    return Err(AppRelayError::InvalidRequest("key is required".to_string()));
                }
                Ok(MappedInputEvent::KeyboardKey {
                    key,
                    action,
                    modifiers,
                })
            }
        }
    }
}

impl Default for InMemoryInputForwardingService {
    fn default() -> Self {
        Self::with_recording_backend()
    }
}

impl InputForwardingService for InMemoryInputForwardingService {
    fn forward_input(
        &mut self,
        request: ForwardInputRequest,
        active_sessions: &[ApplicationSession],
    ) -> Result<InputDelivery, AppRelayError> {
        Self::validate_client_viewport(&request.client_viewport)?;
        let session = Self::selected_session(&request, active_sessions)?;
        let event = request.event;
        let backend_kind = event.backend_kind();
        let mapped_event =
            Self::map_event(event.clone(), &request.client_viewport, &session.viewport)?;

        let mut delivery = InputDelivery {
            session_id: session.id.clone(),
            selected_window_id: session.selected_window.id.clone(),
            mapped_event,
            status: InputDeliveryStatus::Delivered,
        };

        match event {
            InputEvent::Focus => {
                self.focused_session_id = Some(session.id.clone());
                delivery.status = InputDeliveryStatus::Focused;
                self.record_delivery(delivery.clone());
                Ok(delivery)
            }
            InputEvent::Blur => {
                if self.focused_session_id.as_deref() == Some(session.id.as_str()) {
                    self.focused_session_id = None;
                }
                delivery.status = InputDeliveryStatus::Blurred;
                self.record_delivery(delivery.clone());
                Ok(delivery)
            }
            _ if self.focused_session_id.as_deref() != Some(session.id.as_str()) => {
                delivery.status = InputDeliveryStatus::IgnoredBlurred;
                Ok(delivery)
            }
            _ => {
                if let Some(kind) = backend_kind {
                    if let InputBackendService::Unsupported {
                        kind: unsupported_kind,
                        ..
                    } = &self.backend
                    {
                        if *unsupported_kind != kind {
                            self.record_delivery(delivery.clone());
                            return Ok(delivery);
                        }
                    }
                }

                let delivered = self.backend.deliver(delivery)?;
                self.record_delivery(delivered.clone());
                Ok(delivered)
            }
        }
    }
}

pub fn map_point(
    point: ClientPoint,
    client_viewport: &ViewportSize,
    server_viewport: &ViewportSize,
) -> Result<ServerPoint, AppRelayError> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(AppRelayError::InvalidRequest(
            "input position must be finite".to_string(),
        ));
    }

    if point.x < 0.0
        || point.y < 0.0
        || point.x >= client_viewport.width as f32
        || point.y >= client_viewport.height as f32
    {
        return Err(AppRelayError::InvalidRequest(
            "input position is outside client viewport".to_string(),
        ));
    }

    let mapped_x = ((point.x / client_viewport.width as f32) * server_viewport.width as f32)
        .floor()
        .min(server_viewport.width.saturating_sub(1) as f32) as u32;
    let mapped_y = ((point.y / client_viewport.height as f32) * server_viewport.height as f32)
        .floor()
        .min(server_viewport.height.saturating_sub(1) as f32) as u32;

    Ok(ServerPoint::new(mapped_x, mapped_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use apprelay_protocol::{
        ApplicationLaunchIntent, ButtonAction, CreateSessionRequest, KeyAction, KeyModifiers,
        LaunchIntentStatus, PointerButton, SelectedWindow, WindowSelectionMethod,
    };

    use crate::{ApplicationSessionService, InMemoryApplicationSessionService};

    #[test]
    fn input_service_maps_pointer_coordinates_across_viewports() {
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1920, 1080),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::default();

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: ViewportSize::new(960, 540),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("focus session");
        let delivery = input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(960, 540),
                    event: InputEvent::PointerButton {
                        position: ClientPoint::new(480.0, 270.0),
                        button: PointerButton::Primary,
                        action: ButtonAction::Press,
                    },
                },
                &active_sessions,
            )
            .expect("forward pointer");

        assert_eq!(
            delivery.mapped_event,
            MappedInputEvent::PointerButton {
                position: ServerPoint::new(960, 540),
                button: PointerButton::Primary,
                action: ButtonAction::Press,
            }
        );
        assert_eq!(input.deliveries().len(), 2);
    }

    #[test]
    fn input_service_validates_event_payloads() {
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let mut input = InMemoryInputForwardingService::default();

        assert_eq!(
            input.forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::PointerMove {
                        position: ClientPoint::new(1280.0, 10.0),
                    },
                },
                &sessions.active_sessions(),
            ),
            Err(AppRelayError::InvalidRequest(
                "input position is outside client viewport".to_string()
            ))
        );
    }

    #[test]
    fn input_service_rejects_unauthorized_session() {
        let mut input = InMemoryInputForwardingService::default();

        assert_eq!(
            input.forward_input(
                ForwardInputRequest {
                    session_id: "session-unknown".to_string(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &[],
            ),
            Err(AppRelayError::PermissionDenied(
                "input is not authorized for session session-unknown".to_string()
            ))
        );
    }

    #[test]
    fn input_service_blur_stops_delivery() {
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::default();

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("focus session");
        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Blur,
                },
                &active_sessions,
            )
            .expect("blur session");
        let delivery = input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardText {
                        text: "ignored".to_string(),
                    },
                },
                &active_sessions,
            )
            .expect("input ignored while blurred");

        assert_eq!(delivery.status, InputDeliveryStatus::IgnoredBlurred);
        assert_eq!(input.deliveries().len(), 2);
        assert_eq!(input.focused_session_id(), None);
    }

    #[test]
    fn input_service_reports_unsupported_backend() {
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::new(InputBackendService::Unsupported {
            platform: Platform::Linux,
            kind: InputBackendKind::Keyboard,
        });

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("focus session");

        assert_eq!(
            input.forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardKey {
                        key: "Enter".to_string(),
                        action: KeyAction::Press,
                        modifiers: KeyModifiers::default(),
                    },
                },
                &active_sessions,
            ),
            Err(AppRelayError::unsupported(
                Platform::Linux,
                Feature::KeyboardInput
            ))
        );
    }

    #[test]
    fn input_service_clears_session_state_on_close() {
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::default();

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("focus session");

        input.close_session(&session.id);

        assert_eq!(input.focused_session_id(), None);
        assert_eq!(input.deliveries(), &[]);
    }

    #[test]
    fn selected_session_requires_matching_selected_window() {
        let session = ApplicationSession {
            id: "session-1".to_string(),
            application_id: "terminal".to_string(),
            selected_window: SelectedWindow {
                id: "window-session-1".to_string(),
                application_id: "browser".to_string(),
                title: "Browser".to_string(),
                selection_method: WindowSelectionMethod::Synthetic,
            },
            launch_intent: Some(ApplicationLaunchIntent {
                session_id: "session-1".to_string(),
                application_id: "terminal".to_string(),
                launch: None,
                status: LaunchIntentStatus::Attached,
            }),
            viewport: ViewportSize::new(1280, 720),
            resize_intent: None,
            state: SessionState::Ready,
        };
        let mut input = InMemoryInputForwardingService::default();

        assert_eq!(
            input.forward_input(
                ForwardInputRequest {
                    session_id: "session-1".to_string(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &[session],
            ),
            Err(AppRelayError::PermissionDenied(
                "input is not authorized for session session-1".to_string()
            ))
        );
    }
}
