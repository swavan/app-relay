use crate::ViewportSize;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInputRequest {
    pub session_id: String,
    pub client_viewport: ViewportSize,
    pub event: InputEvent,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum InputEvent {
    Focus,
    Blur,
    PointerMove {
        position: ClientPoint,
    },
    PointerButton {
        position: ClientPoint,
        button: PointerButton,
        action: ButtonAction,
    },
    PointerScroll {
        position: ClientPoint,
        delta_x: i32,
        delta_y: i32,
    },
    PointerDrag {
        from: ClientPoint,
        to: ClientPoint,
        button: PointerButton,
    },
    KeyboardText {
        text: String,
    },
    KeyboardKey {
        key: String,
        action: KeyAction,
        modifiers: KeyModifiers,
    },
}

impl InputEvent {
    pub fn requires_focus(&self) -> bool {
        !matches!(self, Self::Focus | Self::Blur)
    }

    pub fn backend_kind(&self) -> Option<InputBackendKind> {
        match self {
            Self::Focus | Self::Blur => None,
            Self::PointerMove { .. }
            | Self::PointerButton { .. }
            | Self::PointerScroll { .. }
            | Self::PointerDrag { .. } => Some(InputBackendKind::Pointer),
            Self::KeyboardText { .. } | Self::KeyboardKey { .. } => {
                Some(InputBackendKind::Keyboard)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputBackendKind {
    Pointer,
    Keyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientPoint {
    pub x: f32,
    pub y: f32,
}

impl ClientPoint {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPoint {
    pub x: u32,
    pub y: u32,
}

impl ServerPoint {
    pub fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonAction {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyAction {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDelivery {
    pub session_id: String,
    pub selected_window_id: String,
    pub mapped_event: MappedInputEvent,
    pub status: InputDeliveryStatus,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MappedInputEvent {
    Focus,
    Blur,
    PointerMove {
        position: ServerPoint,
    },
    PointerButton {
        position: ServerPoint,
        button: PointerButton,
        action: ButtonAction,
    },
    PointerScroll {
        position: ServerPoint,
        delta_x: i32,
        delta_y: i32,
    },
    PointerDrag {
        from: ServerPoint,
        to: ServerPoint,
        button: PointerButton,
    },
    KeyboardText {
        text: String,
    },
    KeyboardKey {
        key: String,
        action: KeyAction,
        modifiers: KeyModifiers,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputDeliveryStatus {
    Focused,
    Blurred,
    Delivered,
    IgnoredBlurred,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_event_classifies_backend_kind() {
        assert_eq!(
            InputEvent::PointerMove {
                position: ClientPoint::new(1.0, 2.0),
            }
            .backend_kind(),
            Some(InputBackendKind::Pointer)
        );
        assert_eq!(
            InputEvent::KeyboardText {
                text: "hello".to_string(),
            }
            .backend_kind(),
            Some(InputBackendKind::Keyboard)
        );
        assert_eq!(InputEvent::Focus.backend_kind(), None);
    }
}
