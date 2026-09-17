//! Injects Android-sourced input into Windows with the Win32 `SendInput` API.
//!
//! Input events arrive device -> host in transport `Control` packets, are
//! decoded to [`InputEvent`], and are replayed here. Pointer events drive the
//! system cursor as absolute mouse input; normalized `0..=65535` coordinates
//! map directly onto the `MOUSEEVENTF_ABSOLUTE` coordinate space, so no
//! per-monitor scaling is needed. Key events inject text as Unicode
//! (`KEYEVENTF_UNICODE`) and editing keys via their virtual-key codes.
//!
//! This is the software slice of the input return path: it drives the system
//! cursor and focused-window keyboard over the whole desktop. Routing input
//! specifically to the USBDisplay virtual monitor as HID touch/pen is a later,
//! driver-side step.

use usbdisplay_protocol::InputEvent;

/// Injects input events into the host OS.
pub trait InputSink {
    fn inject(&mut self, event: &InputEvent);
}

/// Returns the platform input sink (real on Windows, a no-op elsewhere).
pub fn default_sink() -> Box<dyn InputSink + Send> {
    #[cfg(windows)]
    {
        Box::new(windows_impl::WindowsInputSink::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(noop::NoopInputSink)
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use usbdisplay_protocol::{InputAction, KeyAction, NamedKey, PointerButton};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
        MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN,
        VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
    };

    /// Wheel delta for one notch, per Win32 `WHEEL_DELTA`.
    const WHEEL_DELTA: i32 = 120;

    pub struct WindowsInputSink;

    impl WindowsInputSink {
        pub fn new() -> Self {
            Self
        }

        fn send_mouse(&self, flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, mouse_data: i32) {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx,
                        dy,
                        mouseData: mouse_data as u32,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            // SAFETY: `input` is a single, fully initialized INPUT record and the
            // count matches the slice length.
            unsafe {
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
        }

        fn send_key(&self, vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) {
            let input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            // SAFETY: single, fully initialized keyboard INPUT record.
            unsafe {
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
        }

        fn button_flags(button: PointerButton, down: bool) -> Option<MOUSE_EVENT_FLAGS> {
            match (button, down) {
                (PointerButton::Left, true) => Some(MOUSEEVENTF_LEFTDOWN),
                (PointerButton::Left, false) => Some(MOUSEEVENTF_LEFTUP),
                (PointerButton::Right, true) => Some(MOUSEEVENTF_RIGHTDOWN),
                (PointerButton::Right, false) => Some(MOUSEEVENTF_RIGHTUP),
                (PointerButton::Middle, true) => Some(MOUSEEVENTF_MIDDLEDOWN),
                (PointerButton::Middle, false) => Some(MOUSEEVENTF_MIDDLEUP),
                (PointerButton::None, _) => None,
            }
        }

        fn named_vk(named: NamedKey) -> Option<VIRTUAL_KEY> {
            match named {
                NamedKey::Char => None,
                NamedKey::Enter => Some(VK_RETURN),
                NamedKey::Backspace => Some(VK_BACK),
                NamedKey::Tab => Some(VK_TAB),
                NamedKey::Escape => Some(VK_ESCAPE),
                NamedKey::Delete => Some(VK_DELETE),
                NamedKey::ArrowLeft => Some(VK_LEFT),
                NamedKey::ArrowRight => Some(VK_RIGHT),
                NamedKey::ArrowUp => Some(VK_UP),
                NamedKey::ArrowDown => Some(VK_DOWN),
                NamedKey::Home => Some(VK_HOME),
                NamedKey::End => Some(VK_END),
            }
        }
    }

    impl InputSink for WindowsInputSink {
        fn inject(&mut self, event: &InputEvent) {
            match event {
                InputEvent::Pointer(p) => {
                    let abs = MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
                    let dx = p.x as i32;
                    let dy = p.y as i32;
                    match p.action {
                        InputAction::Move => {
                            self.send_mouse(abs | MOUSEEVENTF_MOVE, dx, dy, 0);
                        }
                        InputAction::Down => {
                            // Position first so the click lands where the finger is.
                            self.send_mouse(abs | MOUSEEVENTF_MOVE, dx, dy, 0);
                            if let Some(flags) = Self::button_flags(p.button, true) {
                                self.send_mouse(abs | flags, dx, dy, 0);
                            }
                        }
                        InputAction::Up => {
                            if let Some(flags) = Self::button_flags(p.button, false) {
                                self.send_mouse(abs | flags, dx, dy, 0);
                            }
                        }
                        InputAction::Scroll => {
                            self.send_mouse(abs | MOUSEEVENTF_MOVE, dx, dy, 0);
                            if p.scroll_y != 0 {
                                self.send_mouse(
                                    MOUSEEVENTF_WHEEL,
                                    0,
                                    0,
                                    p.scroll_y as i32 * WHEEL_DELTA,
                                );
                            }
                            if p.scroll_x != 0 {
                                self.send_mouse(
                                    MOUSEEVENTF_HWHEEL,
                                    0,
                                    0,
                                    p.scroll_x as i32 * WHEEL_DELTA,
                                );
                            }
                        }
                    }
                }
                InputEvent::Key(k) => {
                    let up = k.action == KeyAction::Up;
                    match Self::named_vk(k.named) {
                        // Named editing key: inject by virtual-key code.
                        Some(vk) => {
                            let flags = if up {
                                KEYEVENTF_KEYUP
                            } else {
                                KEYBD_EVENT_FLAGS(0)
                            };
                            self.send_key(vk, 0, flags);
                        }
                        // Character key: inject the Unicode code point directly.
                        None => {
                            if k.unicode != 0 {
                                let mut flags = KEYEVENTF_UNICODE;
                                if up {
                                    flags |= KEYEVENTF_KEYUP;
                                }
                                self.send_key(VIRTUAL_KEY(0), k.unicode, flags);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod noop {
    use super::*;

    /// Non-Windows builds (CI, cross checks) accept and drop input.
    pub struct NoopInputSink;

    impl InputSink for NoopInputSink {
        fn inject(&mut self, _event: &InputEvent) {}
    }
}
