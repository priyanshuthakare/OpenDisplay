//! Input events returned from the Android client to the Windows host.
//!
//! Video frames flow host -> device inside [`crate::EncodedFrame`]. Input flows
//! the other direction (device -> host) and is carried in transport `Control`
//! packets. Keeping the input wire format here lets both the Rust host and the
//! Kotlin client share one definition.
//!
//! Every event is a fixed [`INPUT_EVENT_LEN`]-byte record whose first byte is a
//! [`kind`](InputKind) tag, so pointer and key events share one `Control`
//! stream. Pointer coordinates are normalized to `0..=65535` across the surface
//! so the host can map them onto the virtual monitor without knowing the
//! tablet's pixel size. Key events carry a Unicode code point for text (injected
//! with `KEYEVENTF_UNICODE`) or a [`NamedKey`] for editing keys.

use thiserror::Error;

/// Fixed on-wire size of an [`InputEvent`], in bytes.
pub const INPUT_EVENT_LEN: usize = 16;

/// Record tag stored in byte 0 of every event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InputKind {
    Pointer = 1,
    Key = 2,
}

impl TryFrom<u8> for InputKind {
    type Error = InputError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Pointer),
            2 => Ok(Self::Key),
            other => Err(InputError::UnknownKind(other)),
        }
    }
}

/// What happened to the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InputAction {
    /// Pointer contact began (touch down / button press).
    Down = 1,
    /// Pointer moved while in contact.
    Move = 2,
    /// Pointer contact ended (touch up / button release).
    Up = 3,
    /// Discrete scroll notch(es); carried in `scroll_x`/`scroll_y`.
    Scroll = 4,
}

impl TryFrom<u8> for InputAction {
    type Error = InputError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Down),
            2 => Ok(Self::Move),
            3 => Ok(Self::Up),
            4 => Ok(Self::Scroll),
            other => Err(InputError::UnknownAction(other)),
        }
    }
}

/// Which logical button the event applies to.
///
/// Touch input reports [`PointerButton::Left`]; the value exists so mouse and
/// pen buttons can share the same event without a second packet kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PointerButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    /// No button (e.g. a hover move or a scroll event).
    None = 255,
}

impl TryFrom<u8> for PointerButton {
    type Error = InputError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Left),
            1 => Ok(Self::Right),
            2 => Ok(Self::Middle),
            255 => Ok(Self::None),
            other => Err(InputError::UnknownButton(other)),
        }
    }
}

/// Whether a key was pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyAction {
    Down = 1,
    Up = 2,
}

impl TryFrom<u8> for KeyAction {
    type Error = InputError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Down),
            2 => Ok(Self::Up),
            other => Err(InputError::UnknownKeyAction(other)),
        }
    }
}

/// A non-character key that has no Unicode code point of its own.
///
/// The host maps each to a Windows virtual-key code. `Char` (value 0) is the
/// sentinel meaning "use the Unicode code point instead".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NamedKey {
    /// Not a named key; the event's `unicode` field carries the character.
    Char = 0,
    Enter = 1,
    Backspace = 2,
    Tab = 3,
    Escape = 4,
    Delete = 5,
    ArrowLeft = 6,
    ArrowRight = 7,
    ArrowUp = 8,
    ArrowDown = 9,
    Home = 10,
    End = 11,
}

impl TryFrom<u8> for NamedKey {
    type Error = InputError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Char),
            1 => Ok(Self::Enter),
            2 => Ok(Self::Backspace),
            3 => Ok(Self::Tab),
            4 => Ok(Self::Escape),
            5 => Ok(Self::Delete),
            6 => Ok(Self::ArrowLeft),
            7 => Ok(Self::ArrowRight),
            8 => Ok(Self::ArrowUp),
            9 => Ok(Self::ArrowDown),
            10 => Ok(Self::Home),
            11 => Ok(Self::End),
            other => Err(InputError::UnknownNamedKey(other)),
        }
    }
}

/// A pointer (touch/mouse) event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    pub action: InputAction,
    pub button: PointerButton,
    pub pointer_id: u8,
    /// Normalized `0..=65535` across the surface width.
    pub x: u16,
    /// Normalized `0..=65535` across the surface height.
    pub y: u16,
    pub scroll_x: i16,
    pub scroll_y: i16,
}

/// A keyboard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub action: KeyAction,
    /// The key identity: [`NamedKey::Char`] means use `unicode`.
    pub named: NamedKey,
    /// Unicode code point (BMP) when `named` is [`NamedKey::Char`]; else 0.
    pub unicode: u16,
}

/// A single input event travelling device -> host.
///
/// Wire layout (little endian, [`INPUT_EVENT_LEN`] bytes). Byte 0 is the
/// [`InputKind`] tag; the remainder is variant specific and zero padded.
///
/// Pointer (`kind = 1`):
///
/// | Offset | Size | Field                             |
/// | -----: | ---: | --------------------------------- |
/// | 1      | 1    | action ([`InputAction`])          |
/// | 2      | 1    | button ([`PointerButton`])        |
/// | 3      | 1    | pointer_id                        |
/// | 4      | 2    | x (normalized)                    |
/// | 6      | 2    | y (normalized)                    |
/// | 8      | 2    | scroll_x (signed)                 |
/// | 10     | 2    | scroll_y (signed)                 |
/// | 12     | 4    | reserved                          |
///
/// Key (`kind = 2`):
///
/// | Offset | Size | Field                             |
/// | -----: | ---: | --------------------------------- |
/// | 1      | 1    | key action ([`KeyAction`])        |
/// | 2      | 1    | named key ([`NamedKey`])          |
/// | 3      | 1    | reserved (modifiers, future)      |
/// | 4      | 2    | unicode code point                |
/// | 6      | 10   | reserved                          |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
}

/// Errors from decoding an [`InputEvent`] off the wire.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InputError {
    #[error("buffer is shorter than an input event")]
    ShortBuffer,
    #[error("unknown input kind {0}")]
    UnknownKind(u8),
    #[error("unknown input action {0}")]
    UnknownAction(u8),
    #[error("unknown pointer button {0}")]
    UnknownButton(u8),
    #[error("unknown key action {0}")]
    UnknownKeyAction(u8),
    #[error("unknown named key {0}")]
    UnknownNamedKey(u8),
}

impl InputEvent {
    /// Build a pointer event at a normalized position.
    pub fn pointer(
        action: InputAction,
        button: PointerButton,
        pointer_id: u8,
        x: u16,
        y: u16,
    ) -> Self {
        Self::Pointer(PointerEvent {
            action,
            button,
            pointer_id,
            x,
            y,
            scroll_x: 0,
            scroll_y: 0,
        })
    }

    /// Build a scroll event at a normalized position.
    pub fn scroll(x: u16, y: u16, scroll_x: i16, scroll_y: i16) -> Self {
        Self::Pointer(PointerEvent {
            action: InputAction::Scroll,
            button: PointerButton::None,
            pointer_id: 0,
            x,
            y,
            scroll_x,
            scroll_y,
        })
    }

    /// Build a character key event carrying a Unicode code point.
    pub fn key_char(action: KeyAction, unicode: u16) -> Self {
        Self::Key(KeyEvent {
            action,
            named: NamedKey::Char,
            unicode,
        })
    }

    /// Build a named (non-character) key event.
    pub fn key_named(action: KeyAction, named: NamedKey) -> Self {
        Self::Key(KeyEvent {
            action,
            named,
            unicode: 0,
        })
    }

    /// Serialize to [`INPUT_EVENT_LEN`] bytes.
    pub fn encode(&self) -> [u8; INPUT_EVENT_LEN] {
        let mut out = [0u8; INPUT_EVENT_LEN];
        match self {
            Self::Pointer(p) => {
                out[0] = InputKind::Pointer as u8;
                out[1] = p.action as u8;
                out[2] = p.button as u8;
                out[3] = p.pointer_id;
                out[4..6].copy_from_slice(&p.x.to_le_bytes());
                out[6..8].copy_from_slice(&p.y.to_le_bytes());
                out[8..10].copy_from_slice(&p.scroll_x.to_le_bytes());
                out[10..12].copy_from_slice(&p.scroll_y.to_le_bytes());
            }
            Self::Key(k) => {
                out[0] = InputKind::Key as u8;
                out[1] = k.action as u8;
                out[2] = k.named as u8;
                // out[3] reserved (modifiers)
                out[4..6].copy_from_slice(&k.unicode.to_le_bytes());
            }
        }
        out
    }

    /// Parse an event from the front of `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<Self, InputError> {
        if bytes.len() < INPUT_EVENT_LEN {
            return Err(InputError::ShortBuffer);
        }
        match InputKind::try_from(bytes[0])? {
            InputKind::Pointer => Ok(Self::Pointer(PointerEvent {
                action: InputAction::try_from(bytes[1])?,
                button: PointerButton::try_from(bytes[2])?,
                pointer_id: bytes[3],
                x: u16::from_le_bytes([bytes[4], bytes[5]]),
                y: u16::from_le_bytes([bytes[6], bytes[7]]),
                scroll_x: i16::from_le_bytes([bytes[8], bytes[9]]),
                scroll_y: i16::from_le_bytes([bytes[10], bytes[11]]),
            })),
            InputKind::Key => Ok(Self::Key(KeyEvent {
                action: KeyAction::try_from(bytes[1])?,
                named: NamedKey::try_from(bytes[2])?,
                unicode: u16::from_le_bytes([bytes[4], bytes[5]]),
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_pointer_event() {
        let event = InputEvent::pointer(InputAction::Down, PointerButton::Left, 2, 40_000, 12_345);
        let decoded = InputEvent::decode(&event.encode()).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn round_trips_scroll_event() {
        let event = InputEvent::scroll(100, 200, -3, 5);
        let decoded = InputEvent::decode(&event.encode()).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn round_trips_char_key() {
        let event = InputEvent::key_char(KeyAction::Down, 'A' as u16);
        let decoded = InputEvent::decode(&event.encode()).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn round_trips_named_key() {
        let event = InputEvent::key_named(KeyAction::Up, NamedKey::Enter);
        let decoded = InputEvent::decode(&event.encode()).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn encodes_to_fixed_width() {
        let event = InputEvent::pointer(InputAction::Move, PointerButton::None, 0, 0, 65_535);
        assert_eq!(event.encode().len(), INPUT_EVENT_LEN);
    }

    #[test]
    fn rejects_short_buffer() {
        assert_eq!(InputEvent::decode(&[1, 0, 0]), Err(InputError::ShortBuffer));
    }

    #[test]
    fn rejects_unknown_kind() {
        let mut bytes = InputEvent::pointer(InputAction::Up, PointerButton::Left, 0, 1, 1).encode();
        bytes[0] = 9;
        assert_eq!(InputEvent::decode(&bytes), Err(InputError::UnknownKind(9)));
    }

    #[test]
    fn rejects_unknown_action() {
        let mut bytes = InputEvent::pointer(InputAction::Up, PointerButton::Left, 0, 1, 1).encode();
        bytes[1] = 9;
        assert_eq!(
            InputEvent::decode(&bytes),
            Err(InputError::UnknownAction(9))
        );
    }

    #[test]
    fn matches_kotlin_pointer_reference_bytes() {
        // This exact byte sequence is asserted by the Kotlin InputEventTest
        // (`encodesPointerLayoutLittleEndian`). Keeping both sides pinned to the
        // same literal guarantees the host and client wire formats never drift.
        let event = InputEvent::pointer(InputAction::Down, PointerButton::Left, 2, 40_000, 12_345);
        let expected: [u8; INPUT_EVENT_LEN] = [
            1, // kind = Pointer
            1, 0, 2, // action=Down, button=Left, pointer=2
            0x40, 0x9C, // x = 40000
            0x39, 0x30, // y = 12345
            0, 0, // scroll_x
            0, 0, // scroll_y
            0, 0, 0, 0, // reserved
        ];
        assert_eq!(event.encode(), expected);
        assert_eq!(InputEvent::decode(&expected).unwrap(), event);
    }

    #[test]
    fn matches_kotlin_key_reference_bytes() {
        // Mirrors the Kotlin InputEventTest `encodesCharKeyLayout`.
        let event = InputEvent::key_char(KeyAction::Down, 'A' as u16);
        let expected: [u8; INPUT_EVENT_LEN] = [
            2, // kind = Key
            1, // action = Down
            0, // named = Char
            0, // reserved (modifiers)
            0x41, 0x00, // unicode = 'A' (0x41)
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // reserved
        ];
        assert_eq!(event.encode(), expected);
        assert_eq!(InputEvent::decode(&expected).unwrap(), event);
    }
}
