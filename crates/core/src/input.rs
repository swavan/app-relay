use apprelay_protocol::{
    ActiveInputFocus, AppRelayError, ApplicationSession, ClientPoint, Feature, ForwardInputRequest,
    InputBackendKind, InputDelivery, InputDeliveryStatus, InputEvent, KeyAction, KeyModifiers,
    MappedInputEvent, Platform, ServerPoint, SessionState, ViewportSize, WindowSelectionMethod,
};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

pub trait InputForwardingService {
    fn forward_input(
        &mut self,
        request: ForwardInputRequest,
        active_sessions: &[ApplicationSession],
    ) -> Result<InputDelivery, AppRelayError>;

    fn active_input_focus(
        &self,
        active_sessions: &[ApplicationSession],
    ) -> Option<ActiveInputFocus>;
}

pub trait InputBackend {
    fn deliver(
        &self,
        delivery: InputDelivery,
        session: &ApplicationSession,
    ) -> Result<InputDelivery, AppRelayError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputBackendService {
    RecordOnly,
    MacosKeyboard {
        osascript_command: PathBuf,
    },
    Unsupported {
        platform: Platform,
        kind: InputBackendKind,
    },
}

impl InputBackend for InputBackendService {
    fn deliver(
        &self,
        delivery: InputDelivery,
        session: &ApplicationSession,
    ) -> Result<InputDelivery, AppRelayError> {
        match self {
            Self::RecordOnly => Ok(delivery),
            Self::MacosKeyboard { osascript_command } => {
                deliver_macos_keyboard_input(osascript_command, delivery, session)
            }
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

const MACOS_KEYBOARD_INPUT_TIMEOUT: Duration = Duration::from_millis(400);

fn deliver_macos_keyboard_input(
    osascript_command: &Path,
    delivery: InputDelivery,
    session: &ApplicationSession,
) -> Result<InputDelivery, AppRelayError> {
    let bundle_id = session.application_id.trim();
    if bundle_id.is_empty() {
        return Err(AppRelayError::InvalidRequest(
            "macOS keyboard input requires an application bundle id".to_string(),
        ));
    }
    let native_window_id = macos_keyboard_target_window_id(session)?;

    match &delivery.mapped_event {
        MappedInputEvent::KeyboardText { text } => {
            run_macos_keyboard_text_script(osascript_command, bundle_id, native_window_id, text)?;
            Ok(delivery)
        }
        MappedInputEvent::KeyboardKey {
            key,
            action,
            modifiers,
        } => {
            run_macos_keyboard_key_script(
                osascript_command,
                bundle_id,
                native_window_id,
                key,
                *action,
                *modifiers,
            )?;
            Ok(delivery)
        }
        MappedInputEvent::PointerMove { .. }
        | MappedInputEvent::PointerButton { .. }
        | MappedInputEvent::PointerScroll { .. }
        | MappedInputEvent::PointerDrag { .. } => Err(AppRelayError::unsupported(
            Platform::Macos,
            Feature::MouseInput,
        )),
        MappedInputEvent::Focus | MappedInputEvent::Blur => Ok(delivery),
    }
}

fn macos_keyboard_target_window_id(
    session: &ApplicationSession,
) -> Result<Option<&str>, AppRelayError> {
    if session.selected_window.selection_method != WindowSelectionMethod::NativeWindow {
        return Ok(None);
    }

    parse_macos_native_keyboard_window_id(&session.selected_window.id).map(Some)
}

fn parse_macos_native_keyboard_window_id(window_id: &str) -> Result<&str, AppRelayError> {
    let Some(encoded_id) = window_id.strip_prefix("macos-window-") else {
        return Err(AppRelayError::InvalidRequest(format!(
            "selected window id `{window_id}` is not a macOS native window id"
        )));
    };
    let Some((_, native_id)) = encoded_id.rsplit_once('-') else {
        return Err(AppRelayError::InvalidRequest(format!(
            "selected window id `{window_id}` is missing a macOS native window id"
        )));
    };
    let native_id = native_id.trim();

    if native_id.is_empty() || !native_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppRelayError::InvalidRequest(format!(
            "selected window id `{window_id}` has an unusable macOS native window id"
        )));
    }

    Ok(native_id)
}

fn run_macos_keyboard_text_script(
    osascript_command: &Path,
    bundle_id: &str,
    native_window_id: Option<&str>,
    text: &str,
) -> Result<(), AppRelayError> {
    let mut args = vec![bundle_id, text];
    if let Some(native_window_id) = native_window_id {
        args.push(native_window_id);
    }

    let output = run_macos_keyboard_osascript(
        osascript_command,
        &[
            "on run argv",
            "set targetBundleId to item 1 of argv",
            "set inputText to item 2 of argv",
            "if (count of argv) > 2 then set targetWindowId to (item 3 of argv) as integer",
            "tell application \"System Events\"",
            "set matchingProcesses to application processes whose bundle identifier is targetBundleId",
            "if (count of matchingProcesses) is 0 then error \"application process not found\"",
            "set targetProcess to missing value",
            "if (count of argv) > 2 then",
            "repeat with candidateProcess in matchingProcesses",
            "set matchingWindows to windows of candidateProcess whose id is targetWindowId",
            "if (count of matchingWindows) > 0 then",
            "set targetProcess to candidateProcess",
            "perform action \"AXRaise\" of item 1 of matchingWindows",
            "exit repeat",
            "end if",
            "end repeat",
            "if targetProcess is missing value then error \"window not found\"",
            "else",
            "set targetProcess to item 1 of matchingProcesses",
            "end if",
            "set frontmost of targetProcess to true",
            "keystroke inputText",
            "end tell",
            "end run",
        ],
        &args,
    )?;

    validate_macos_keyboard_output(output)
}

fn run_macos_keyboard_key_script(
    osascript_command: &Path,
    bundle_id: &str,
    native_window_id: Option<&str>,
    key: &str,
    action: KeyAction,
    modifiers: KeyModifiers,
) -> Result<(), AppRelayError> {
    if action == KeyAction::Release {
        return Ok(());
    }

    if let Some(key_code) = macos_key_code(key) {
        let mut args = vec![bundle_id];
        if let Some(native_window_id) = native_window_id {
            args.push(native_window_id);
        }

        let command = format!("key code {key_code}{}", macos_modifier_clause(modifiers));
        let output = run_macos_keyboard_osascript(
            osascript_command,
            &[
                "on run argv",
                "set targetBundleId to item 1 of argv",
                "if (count of argv) > 1 then set targetWindowId to (item 2 of argv) as integer",
                "tell application \"System Events\"",
                "set matchingProcesses to application processes whose bundle identifier is targetBundleId",
                "if (count of matchingProcesses) is 0 then error \"application process not found\"",
                "set targetProcess to missing value",
                "if (count of argv) > 1 then",
                "repeat with candidateProcess in matchingProcesses",
                "set matchingWindows to windows of candidateProcess whose id is targetWindowId",
                "if (count of matchingWindows) > 0 then",
                "set targetProcess to candidateProcess",
                "perform action \"AXRaise\" of item 1 of matchingWindows",
                "exit repeat",
                "end if",
                "end repeat",
                "if targetProcess is missing value then error \"window not found\"",
                "else",
                "set targetProcess to item 1 of matchingProcesses",
                "end if",
                "set frontmost of targetProcess to true",
                command.as_str(),
                "end tell",
                "end run",
            ],
            &args,
        )?;
        return validate_macos_keyboard_output(output);
    }

    if key.chars().count() == 1 {
        let mut args = vec![bundle_id, key];
        if let Some(native_window_id) = native_window_id {
            args.push(native_window_id);
        }

        let command = format!("keystroke inputKey{}", macos_modifier_clause(modifiers));
        let output = run_macos_keyboard_osascript(
            osascript_command,
            &[
                "on run argv",
                "set targetBundleId to item 1 of argv",
                "set inputKey to item 2 of argv",
                "if (count of argv) > 2 then set targetWindowId to (item 3 of argv) as integer",
                "tell application \"System Events\"",
                "set matchingProcesses to application processes whose bundle identifier is targetBundleId",
                "if (count of matchingProcesses) is 0 then error \"application process not found\"",
                "set targetProcess to missing value",
                "if (count of argv) > 2 then",
                "repeat with candidateProcess in matchingProcesses",
                "set matchingWindows to windows of candidateProcess whose id is targetWindowId",
                "if (count of matchingWindows) > 0 then",
                "set targetProcess to candidateProcess",
                "perform action \"AXRaise\" of item 1 of matchingWindows",
                "exit repeat",
                "end if",
                "end repeat",
                "if targetProcess is missing value then error \"window not found\"",
                "else",
                "set targetProcess to item 1 of matchingProcesses",
                "end if",
                "set frontmost of targetProcess to true",
                command.as_str(),
                "end tell",
                "end run",
            ],
            &args,
        )?;
        return validate_macos_keyboard_output(output);
    }

    Err(AppRelayError::InvalidRequest(format!(
        "macOS keyboard key `{}` is not supported",
        key
    )))
}

fn macos_key_code(key: &str) -> Option<u16> {
    match key.trim().to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(36),
        "tab" => Some(48),
        "space" | "spacebar" => Some(49),
        "escape" | "esc" => Some(53),
        "backspace" | "delete" => Some(51),
        "arrowleft" | "left" => Some(123),
        "arrowright" | "right" => Some(124),
        "arrowdown" | "down" => Some(125),
        "arrowup" | "up" => Some(126),
        _ => None,
    }
}

fn macos_modifier_clause(modifiers: KeyModifiers) -> String {
    let mut modifier_names = Vec::new();
    if modifiers.shift {
        modifier_names.push("shift down");
    }
    if modifiers.control {
        modifier_names.push("control down");
    }
    if modifiers.alt {
        modifier_names.push("option down");
    }
    if modifiers.meta {
        modifier_names.push("command down");
    }

    if modifier_names.is_empty() {
        String::new()
    } else {
        format!(" using {{{}}}", modifier_names.join(", "))
    }
}

fn run_macos_keyboard_osascript(
    osascript_command: &Path,
    script_lines: &[&str],
    args: &[&str],
) -> Result<Output, AppRelayError> {
    let mut command = Command::new(osascript_command);
    for line in script_lines {
        command.arg("-e").arg(line);
    }
    command.args(args);

    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppRelayError::ServiceUnavailable(format!(
                "failed to run macOS keyboard input command `{}`: {error}",
                osascript_command.display()
            ))
        })?;

    wait_for_macos_keyboard_output_with_timeout(child, MACOS_KEYBOARD_INPUT_TIMEOUT).ok_or_else(
        || AppRelayError::ServiceUnavailable("macOS keyboard input command timed out".to_string()),
    )
}

fn wait_for_macos_keyboard_output_with_timeout(
    mut child: Child,
    timeout: Duration,
) -> Option<Output> {
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait().ok()? {
            Some(_) => return child.wait_with_output().ok(),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn validate_macos_keyboard_output(output: Output) -> Result<(), AppRelayError> {
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        if message.is_empty() {
            Err(AppRelayError::ServiceUnavailable(
                "macOS keyboard input command failed".to_string(),
            ))
        } else {
            Err(AppRelayError::ServiceUnavailable(format!(
                "macOS keyboard input command failed: {message}"
            )))
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

    pub fn active_input_focus(
        &self,
        active_sessions: &[ApplicationSession],
    ) -> Option<ActiveInputFocus> {
        let focused_session_id = self.focused_session_id.as_deref()?;
        active_sessions
            .iter()
            .find(|session| {
                session.id == focused_session_id
                    && session.state != SessionState::Closed
                    && session.selected_window.application_id == session.application_id
            })
            .map(|session| ActiveInputFocus {
                session_id: session.id.clone(),
                selected_window_id: session.selected_window.id.clone(),
            })
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

                let delivered = self.backend.deliver(delivery, session)?;
                self.record_delivery(delivered.clone());
                Ok(delivered)
            }
        }
    }

    fn active_input_focus(
        &self,
        active_sessions: &[ApplicationSession],
    ) -> Option<ActiveInputFocus> {
        InMemoryInputForwardingService::active_input_focus(self, active_sessions)
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

    fn native_macos_session(window_id: &str) -> ApplicationSession {
        ApplicationSession {
            id: "session-1".to_string(),
            application_id: "dev.apprelay.fake".to_string(),
            selected_window: SelectedWindow {
                id: window_id.to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                title: "Native Fake Window".to_string(),
                selection_method: WindowSelectionMethod::NativeWindow,
            },
            viewport: ViewportSize::new(1280, 720),
            state: SessionState::Ready,
            launch_intent: Some(ApplicationLaunchIntent {
                session_id: "session-1".to_string(),
                application_id: "dev.apprelay.fake".to_string(),
                launch: None,
                status: LaunchIntentStatus::Attached,
            }),
            resize_intent: None,
        }
    }

    #[cfg(unix)]
    fn unique_test_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
    }

    #[cfg(unix)]
    fn write_executable_script(path: &std::path::Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, contents).expect("write executable script");
        let mut permissions = std::fs::metadata(path)
            .expect("read executable script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark executable script");
    }

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
    #[cfg(unix)]
    fn macos_keyboard_backend_delivers_text_with_fake_osascript() {
        let root = unique_test_dir("macos-keyboard-text");
        std::fs::create_dir_all(&root).expect("create test input directory");
        let marker = root.join("keyboard-marker");
        let osascript = root.join("fake-osascript");
        write_executable_script(
            &osascript,
            &format!(
                "#!/bin/sh\nlast=''\nfor arg in \"$@\"; do last=$arg; done\nprintf '%s\\n' \"$last\" > {}\n",
                marker.display()
            ),
        );
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: osascript,
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
        let delivery = input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardText {
                        text: "AppRelay".to_string(),
                    },
                },
                &active_sessions,
            )
            .expect("deliver keyboard text");

        assert_eq!(delivery.status, InputDeliveryStatus::Delivered);
        assert_eq!(
            std::fs::read_to_string(&marker).expect("read keyboard marker"),
            "AppRelay\n"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn macos_keyboard_backend_targets_native_selected_window() {
        let root = unique_test_dir("macos-keyboard-native-window");
        std::fs::create_dir_all(&root).expect("create test input directory");
        let marker = root.join("keyboard-marker");
        let osascript = root.join("fake-osascript");
        write_executable_script(
            &osascript,
            &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", marker.display()),
        );
        let active_sessions = vec![native_macos_session("macos-window-session-1-88")];
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: osascript,
        });

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: "session-1".to_string(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("focus session");
        input
            .forward_input(
                ForwardInputRequest {
                    session_id: "session-1".to_string(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardText {
                        text: "AppRelay".to_string(),
                    },
                },
                &active_sessions,
            )
            .expect("deliver keyboard text");

        let script_args = std::fs::read_to_string(&marker).expect("read keyboard marker");
        assert!(script_args.contains("dev.apprelay.fake"));
        assert!(script_args.contains("AppRelay"));
        assert!(script_args.contains("88"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn macos_keyboard_backend_rejects_unusable_native_window_id() {
        let active_sessions = vec![native_macos_session("macos-window-session-1-not-a-number")];
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: PathBuf::from("unused-osascript"),
        });

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: "session-1".to_string(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("focus session");
        let error = input
            .forward_input(
                ForwardInputRequest {
                    session_id: "session-1".to_string(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardText {
                        text: "AppRelay".to_string(),
                    },
                },
                &active_sessions,
            )
            .expect_err("invalid native window id should fail input");

        assert!(matches!(error, AppRelayError::InvalidRequest(_)));
    }

    #[test]
    #[cfg(unix)]
    fn macos_keyboard_backend_delivers_conservative_key_with_fake_osascript() {
        let root = unique_test_dir("macos-keyboard-key");
        std::fs::create_dir_all(&root).expect("create test input directory");
        let marker = root.join("keyboard-marker");
        let osascript = root.join("fake-osascript");
        write_executable_script(
            &osascript,
            &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", marker.display()),
        );
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: osascript,
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
        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardKey {
                        key: "Enter".to_string(),
                        action: KeyAction::Press,
                        modifiers: KeyModifiers {
                            meta: true,
                            ..KeyModifiers::default()
                        },
                    },
                },
                &active_sessions,
            )
            .expect("deliver keyboard key");

        let script_args = std::fs::read_to_string(&marker).expect("read keyboard marker");
        assert!(script_args.contains("key code 36 using {command down}"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn macos_keyboard_backend_reports_osascript_failure() {
        let root = unique_test_dir("macos-keyboard-failure");
        std::fs::create_dir_all(&root).expect("create test input directory");
        let osascript = root.join("fake-osascript");
        write_executable_script(
            &osascript,
            "#!/bin/sh\nprintf 'accessibility denied\\n' >&2\nexit 7\n",
        );
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: osascript,
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
        let error = input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardText {
                        text: "AppRelay".to_string(),
                    },
                },
                &active_sessions,
            )
            .expect_err("osascript failure should fail input");

        assert!(matches!(
            error,
            AppRelayError::ServiceUnavailable(message) if message.contains("accessibility denied")
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn macos_keyboard_backend_times_out_hung_osascript() {
        let root = unique_test_dir("macos-keyboard-timeout");
        std::fs::create_dir_all(&root).expect("create test input directory");
        let osascript = root.join("fake-osascript");
        write_executable_script(&osascript, "#!/bin/sh\nsleep 1\n");
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: osascript,
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
        let started = Instant::now();
        let error = input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id,
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::KeyboardText {
                        text: "AppRelay".to_string(),
                    },
                },
                &active_sessions,
            )
            .expect_err("hung osascript should time out");

        assert!(started.elapsed() < Duration::from_millis(800));
        assert!(matches!(error, AppRelayError::ServiceUnavailable(_)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn macos_keyboard_backend_reports_pointer_unsupported() {
        let mut sessions = InMemoryApplicationSessionService::default();
        let session = sessions
            .create_session(CreateSessionRequest {
                application_id: "terminal".to_string(),
                viewport: ViewportSize::new(1280, 720),
            })
            .expect("create session");
        let active_sessions = sessions.active_sessions();
        let mut input = InMemoryInputForwardingService::new(InputBackendService::MacosKeyboard {
            osascript_command: PathBuf::from("unused-osascript"),
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
                    event: InputEvent::PointerButton {
                        position: ClientPoint::new(10.0, 10.0),
                        button: PointerButton::Primary,
                        action: ButtonAction::Press,
                    },
                },
                &active_sessions,
            ),
            Err(AppRelayError::unsupported(
                Platform::Macos,
                Feature::MouseInput
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
    fn input_service_discovers_only_active_input_focus() {
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

        assert_eq!(
            input.active_input_focus(&active_sessions),
            Some(ActiveInputFocus {
                session_id: session.id.clone(),
                selected_window_id: session.selected_window.id.clone(),
            })
        );

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

        assert_eq!(input.active_input_focus(&active_sessions), None);

        input
            .forward_input(
                ForwardInputRequest {
                    session_id: session.id.clone(),
                    client_viewport: ViewportSize::new(1280, 720),
                    event: InputEvent::Focus,
                },
                &active_sessions,
            )
            .expect("refocus session");

        assert_eq!(input.active_input_focus(&[]), None);

        let mut closed_session = session.clone();
        closed_session.state = SessionState::Closed;
        assert_eq!(input.active_input_focus(&[closed_session]), None);

        input.close_session(&session.id);
        assert_eq!(input.active_input_focus(&active_sessions), None);
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
