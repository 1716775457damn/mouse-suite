//! Global hotkeys (Windows): workflow start/stop + scribe record toggle.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Ctrl+Alt+F9 — compile flow and start
    Start,
    /// Ctrl+Alt+F10 — stop clicker / workflow / scribe recording
    Stop,
    /// F8 — toggle 文档录制
    ScribeToggle,
}

pub struct HotkeyBus {
    rx: Receiver<HotkeyEvent>,
}

impl HotkeyBus {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        spawn_listener(tx);
        Self { rx }
    }

    pub fn try_recv(&self) -> Option<HotkeyEvent> {
        match self.rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn spawn_listener(tx: Sender<HotkeyEvent>) {
    #[cfg(windows)]
    {
        thread::spawn(move || windows_hotkey_loop(tx));
    }
    #[cfg(not(windows))]
    {
        let _ = tx;
    }
}

#[cfg(windows)]
fn windows_hotkey_loop(tx: Sender<HotkeyEvent>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };

    const ID_START: i32 = 0x4D53;
    const ID_STOP: i32 = 0x4D54;
    const ID_SCRIBE: i32 = 0x4D55;
    const VK_F8: u32 = 0x77;
    const VK_F9: u32 = 0x78;
    const VK_F10: u32 = 0x79;

    unsafe {
        let mods = HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0);
        let _ = RegisterHotKey(HWND(0), ID_START, mods, VK_F9);
        let _ = RegisterHotKey(HWND(0), ID_STOP, mods, VK_F10);
        // F8 alone for scribe toggle
        let _ = RegisterHotKey(HWND(0), ID_SCRIBE, HOT_KEY_MODIFIERS(MOD_NOREPEAT.0), VK_F8);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;
                let ev = match id {
                    ID_START => Some(HotkeyEvent::Start),
                    ID_STOP => Some(HotkeyEvent::Stop),
                    ID_SCRIBE => Some(HotkeyEvent::ScribeToggle),
                    _ => None,
                };
                if let Some(ev) = ev {
                    if tx.send(ev).is_err() {
                        break;
                    }
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnregisterHotKey(HWND(0), ID_START);
        let _ = UnregisterHotKey(HWND(0), ID_STOP);
        let _ = UnregisterHotKey(HWND(0), ID_SCRIBE);
    }
}
