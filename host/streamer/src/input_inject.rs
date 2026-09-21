//! Injects Android-sourced input into Windows with the Win32 `SendInput` API.
//!
//! Input events arrive device -> host in transport `Control` packets, are
//! decoded to [`InputEvent`], and are replayed here. Pointer events drive the
//! system cursor as absolute mouse input. Normalized `0..=65535` coordinates
//! span the *tablet surface*, which shows exactly the virtual monitor, so they
//! are first mapped into that monitor's rectangle within the virtual desktop
//! (`MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` coordinates span every
//! monitor). The monitor is found by its "USBDisplay" friendly name; if it is
//! absent the coordinates are used as-is, spanning the whole desktop.
//!
//! Key events inject text as Unicode (`KEYEVENTF_UNICODE`) and editing keys via
//! their virtual-key codes.
//!
//! This is the software slice of the input return path: it drives the system
//! cursor and focused-window keyboard. Routing input specifically to the
//! USBDisplay virtual monitor as HID touch/pen is a later, driver-side step.

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
    use std::time::{Duration, Instant};
    use usbdisplay_protocol::{InputAction, KeyAction, NamedKey, PointerButton};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{BOOL, FALSE, LPARAM, RECT, TRUE};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
        MONITORINFO, MONITORINFOEXW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
        MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN,
        VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    /// Wheel delta for one notch, per Win32 `WHEEL_DELTA`.
    const WHEEL_DELTA: i32 = 120;

    /// Re-resolve the target monitor at most this often, so a Windows mode
    /// switch is picked up without enumerating displays on every event.
    const RESOLVE_INTERVAL: Duration = Duration::from_secs(1);

    /// Substring of the virtual monitor's friendly name, advertised by the
    /// driver through its EDID product-name descriptor (`driver/idd/Edid.cpp`).
    const MONITOR_NAME_HINT: &str = "usbdisplay";

    /// A rectangle in virtual-desktop coordinates.
    #[derive(Clone, Copy, Debug)]
    struct MonitorRect {
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    }

    fn wide_to_string(wide: &[u16]) -> String {
        let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        String::from_utf16_lossy(&wide[..end])
    }

    struct MonitorSearch {
        found: Option<MonitorRect>,
    }

    /// The whole virtual desktop (all monitors stitched together).
    fn virtual_screen() -> MonitorRect {
        // SAFETY: GetSystemMetrics is a pure query with no preconditions.
        unsafe {
            MonitorRect {
                left: GetSystemMetrics(SM_XVIRTUALSCREEN),
                top: GetSystemMetrics(SM_YVIRTUALSCREEN),
                width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
                height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    /// # Safety
    /// Invoked by `EnumDisplayMonitors`; `data` must point to a `MonitorSearch`.
    unsafe extern "system" fn monitor_proc(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let search = &mut *(data.0 as *mut MonitorSearch);

        let mut info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        if !GetMonitorInfoW(hmonitor, &mut info.monitorInfo).as_bool() {
            return TRUE;
        }

        // `szDevice` is the adapter name (\\.\DISPLAY2). The *monitor* friendly
        // name — which the driver sets to "USBDisplay" — comes from the first
        // display device attached to that adapter.
        let adapter = wide_to_string(&info.szDevice);
        let adapter_w: Vec<u16> = adapter.encode_utf16().chain(std::iter::once(0)).collect();
        let mut device = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if EnumDisplayDevicesW(PCWSTR(adapter_w.as_ptr()), 0, &mut device, 0).as_bool() {
            let friendly = wide_to_string(&device.DeviceString).to_ascii_lowercase();
            if friendly.contains(MONITOR_NAME_HINT) {
                let r = info.monitorInfo.rcMonitor;
                search.found = Some(MonitorRect {
                    left: r.left,
                    top: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                });
                return FALSE; // unambiguous; stop enumerating.
            }
        }
        TRUE
    }

    /// The virtual monitor's desktop rectangle, or `None` when it is absent.
    fn resolve_monitor() -> Option<MonitorRect> {
        let mut search = MonitorSearch { found: None };
        // SAFETY: the callback only dereferences the `MonitorSearch` passed in,
        // which outlives the call.
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(monitor_proc),
                LPARAM(&mut search as *mut MonitorSearch as isize),
            );
        }
        search.found
    }

    pub struct WindowsInputSink {
        target: Option<MonitorRect>,
        vscreen: MonitorRect,
        last_resolve: Instant,
    }

    impl WindowsInputSink {
        pub fn new() -> Self {
            let target = resolve_monitor();
            match target {
                Some(m) => eprintln!(
                    "input_target_monitor={}x{} at ({},{})",
                    m.width, m.height, m.left, m.top
                ),
                None => {
                    eprintln!("input_target_monitor=whole-desktop (USBDisplay monitor not found)")
                }
            }
            Self {
                target,
                vscreen: virtual_screen(),
                last_resolve: Instant::now(),
            }
        }

        /// Re-resolve the monitor geometry at most once per `RESOLVE_INTERVAL`,
        /// so a mode switch (resolution or layout change) is absorbed.
        fn refresh(&mut self) {
            if self.last_resolve.elapsed() < RESOLVE_INTERVAL {
                return;
            }
            self.last_resolve = Instant::now();
            if let Some(m) = resolve_monitor() {
                self.target = Some(m);
            }
            self.vscreen = virtual_screen();
        }

        /// Map tablet-normalized `0..=65535` coordinates — which span the
        /// virtual monitor's surface — onto `MOUSEEVENTF_ABSOLUTE |
        /// MOUSEEVENTF_VIRTUALDESK` coordinates, which span the whole virtual
        /// desktop.
        ///
        /// Without this the tablet surface is stretched across every monitor,
        /// so on a multi-monitor desktop touches land on the wrong screen.
        fn map_absolute(&self, nx: u16, ny: u16) -> (i32, i32) {
            let Some(m) = self.target else {
                // No virtual monitor: keep the historical behaviour of treating
                // the coordinates as spanning the entire desktop.
                return (nx as i32, ny as i32);
            };
            let px = m.left as i64 + (nx as i64 * m.width as i64) / 65535;
            let py = m.top as i64 + (ny as i64 * m.height as i64) / 65535;
            let ax = (px - self.vscreen.left as i64) * 65535 / self.vscreen.width.max(1) as i64;
            let ay = (py - self.vscreen.top as i64) * 65535 / self.vscreen.height.max(1) as i64;
            (ax.clamp(0, 65535) as i32, ay.clamp(0, 65535) as i32)
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
                    self.refresh();
                    let abs = MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
                    let (dx, dy) = self.map_absolute(p.x, p.y);
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
