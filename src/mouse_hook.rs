//! Single global low-level mouse hook shared by recorder (marquee),
//! scribe (doc clicks), and flow click-recording.
//!
//! Prefer `rdev::grab` (can swallow events). Fall back to `listen` if grab fails
//! (macOS Accessibility / Linux evdev permissions).

use rdev::{grab, listen, Button, Event, EventType, Key};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

/// Absolute screen coords for recorder marquee.
#[derive(Debug, Clone, Copy)]
pub enum RecorderMsg {
    Down(f64, f64),
    Up(f64, f64),
    Move(f64, f64),
}

/// Flow click-record event: screen coords + whether Alt was held (precise marquee).
pub type FlowClick = (i32, i32, bool);

pub type IgnoreRect = Arc<Mutex<Option<(i32, i32, i32, i32)>>>;

static LAST_X: AtomicI32 = AtomicI32::new(0);
static LAST_Y: AtomicI32 = AtomicI32::new(0);
static ALT_HELD: AtomicBool = AtomicBool::new(false);

/// Last known cursor position from the hook thread (all platforms).
pub fn last_cursor_pos() -> (i32, i32) {
    (
        LAST_X.load(Ordering::Relaxed),
        LAST_Y.load(Ordering::Relaxed),
    )
}

fn alt_held() -> bool {
    if ALT_HELD.load(Ordering::Relaxed) {
        return true;
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        // VK_MENU = 0x12 (either Alt key)
        return unsafe { GetAsyncKeyState(0x12) as u16 & 0x8000 != 0 };
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn track_keys(event_type: &EventType) {
    match event_type {
        EventType::KeyPress(Key::Alt) | EventType::KeyPress(Key::AltGr) => {
            ALT_HELD.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::Alt) | EventType::KeyRelease(Key::AltGr) => {
            ALT_HELD.store(false, Ordering::Relaxed);
        }
        _ => {}
    }
}

#[derive(Clone)]
pub struct MouseHook {
    pub recorder_flag: Arc<AtomicBool>,
    pub scribe_flag: Arc<AtomicBool>,
    pub flow_flag: Arc<AtomicBool>,
    pub scribe_ignore: IgnoreRect,
}

impl MouseHook {
    /// Start the unique grab/listen thread. Call once at app startup.
    pub fn start(
        recorder_tx: Sender<RecorderMsg>,
        scribe_click_tx: Sender<(i32, i32)>,
        flow_click_tx: Sender<FlowClick>,
    ) -> Self {
        let recorder_flag = Arc::new(AtomicBool::new(false));
        let scribe_flag = Arc::new(AtomicBool::new(false));
        let flow_flag = Arc::new(AtomicBool::new(false));
        let scribe_ignore: IgnoreRect = Arc::new(Mutex::new(None));

        let rf = recorder_flag.clone();
        let sf = scribe_flag.clone();
        let ff = flow_flag.clone();
        let ignore = scribe_ignore.clone();

        thread::spawn(move || {
            let last_pos = Arc::new(Mutex::new((0.0_f64, 0.0_f64)));

            // Grab callback can swallow events (return None).
            let last_for_grab = Arc::clone(&last_pos);
            let rf_g = rf.clone();
            let sf_g = sf.clone();
            let ff_g = ff.clone();
            let ignore_g = ignore.clone();
            let rec_tx = recorder_tx.clone();
            let scrib_tx = scribe_click_tx.clone();
            let flow_tx = flow_click_tx.clone();
            let grab_cb = move |event: Event| -> Option<Event> {
                track_keys(&event.event_type);
                let capturing = rf_g.load(Ordering::Relaxed);
                let scribing = sf_g.load(Ordering::Relaxed);
                let flow_rec = ff_g.load(Ordering::Relaxed);
                match event.event_type {
                    EventType::MouseMove { x, y } => {
                        LAST_X.store(x as i32, Ordering::Relaxed);
                        LAST_Y.store(y as i32, Ordering::Relaxed);
                        if let Ok(mut pos) = last_for_grab.lock() {
                            *pos = (x, y);
                        }
                        if capturing {
                            let _ = rec_tx.send(RecorderMsg::Move(x, y));
                        }
                        Some(event)
                    }
                    EventType::ButtonPress(Button::Left) => {
                        let (x, y) = last_for_grab
                            .lock()
                            .map(|p| (p.0, p.1))
                            .unwrap_or((0.0, 0.0));
                        if capturing {
                            let _ = rec_tx.send(RecorderMsg::Down(x, y));
                            return None;
                        }
                        let xi = x as i32;
                        let yi = y as i32;
                        if scribing {
                            let blocked = ignore_g
                                .lock()
                                .ok()
                                .and_then(|g| *g)
                                .map(|(l, t, r, b)| xi >= l && xi < r && yi >= t && yi < b)
                                .unwrap_or(false);
                            if !blocked {
                                let _ = scrib_tx.send((xi, yi));
                            }
                        }
                        if flow_rec {
                            let precise = alt_held();
                            let _ = flow_tx.send((xi, yi, precise));
                        }
                        Some(event)
                    }
                    EventType::ButtonRelease(Button::Left) => {
                        if capturing {
                            let (x, y) = last_for_grab
                                .lock()
                                .map(|p| (p.0, p.1))
                                .unwrap_or((0.0, 0.0));
                            let _ = rec_tx.send(RecorderMsg::Up(x, y));
                            return None;
                        }
                        Some(event)
                    }
                    _ => Some(event),
                }
            };

            if let Err(e) = grab(grab_cb) {
                eprintln!("[mouse_hook] grab failed ({e:?}); falling back to listen");
                // Listen cannot swallow events.
                let listen_cb = move |event: Event| {
                    track_keys(&event.event_type);
                    let capturing = rf.load(Ordering::Relaxed);
                    let scribing = sf.load(Ordering::Relaxed);
                    let flow_rec = ff.load(Ordering::Relaxed);
                    match event.event_type {
                        EventType::MouseMove { x, y } => {
                            LAST_X.store(x as i32, Ordering::Relaxed);
                            LAST_Y.store(y as i32, Ordering::Relaxed);
                            if let Ok(mut pos) = last_pos.lock() {
                                *pos = (x, y);
                            }
                            if capturing {
                                let _ = recorder_tx.send(RecorderMsg::Move(x, y));
                            }
                        }
                        EventType::ButtonPress(Button::Left) => {
                            let (x, y) = last_pos
                                .lock()
                                .map(|p| (p.0, p.1))
                                .unwrap_or((0.0, 0.0));
                            if capturing {
                                let _ = recorder_tx.send(RecorderMsg::Down(x, y));
                            }
                            let xi = x as i32;
                            let yi = y as i32;
                            if scribing {
                                let blocked = ignore
                                    .lock()
                                    .ok()
                                    .and_then(|g| *g)
                                    .map(|(l, t, r, b)| xi >= l && xi < r && yi >= t && yi < b)
                                    .unwrap_or(false);
                                if !blocked {
                                    let _ = scribe_click_tx.send((xi, yi));
                                }
                            }
                            if flow_rec {
                                let precise = alt_held();
                                let _ = flow_click_tx.send((xi, yi, precise));
                            }
                        }
                        EventType::ButtonRelease(Button::Left) => {
                            if capturing {
                                let (x, y) = last_pos
                                    .lock()
                                    .map(|p| (p.0, p.1))
                                    .unwrap_or((0.0, 0.0));
                                let _ = recorder_tx.send(RecorderMsg::Up(x, y));
                            }
                        }
                        _ => {}
                    }
                };
                if let Err(e2) = listen(listen_cb) {
                    eprintln!("[mouse_hook] listen also failed: {e2:?}");
                }
            }
        });

        Self {
            recorder_flag,
            scribe_flag,
            flow_flag,
            scribe_ignore,
        }
    }
}
