//! Single global low-level mouse hook shared by recorder (marquee) and scribe (click docs).
//!
//! Windows only allows one reliable `rdev::grab` / `listen` pipeline per process.
//! Running both at once makes document recording receive zero clicks.

use rdev::{grab, Button, Event, EventType};
use std::sync::atomic::{AtomicBool, Ordering};
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

pub type IgnoreRect = Arc<Mutex<Option<(i32, i32, i32, i32)>>>;

#[derive(Clone)]
pub struct MouseHook {
    pub recorder_flag: Arc<AtomicBool>,
    pub scribe_flag: Arc<AtomicBool>,
    pub scribe_ignore: IgnoreRect,
}

impl MouseHook {
    /// Start the unique grab thread. Call once at app startup.
    pub fn start(
        recorder_tx: Sender<RecorderMsg>,
        scribe_click_tx: Sender<(i32, i32)>,
    ) -> Self {
        let recorder_flag = Arc::new(AtomicBool::new(false));
        let scribe_flag = Arc::new(AtomicBool::new(false));
        let scribe_ignore: IgnoreRect = Arc::new(Mutex::new(None));

        let rf = recorder_flag.clone();
        let sf = scribe_flag.clone();
        let ignore = scribe_ignore.clone();

        thread::spawn(move || {
            let last_pos = Arc::new(Mutex::new((0.0_f64, 0.0_f64)));
            let last_cb = Arc::clone(&last_pos);
            let cb = move |event: Event| -> Option<Event> {
                let capturing = rf.load(Ordering::Relaxed);
                let scribing = sf.load(Ordering::Relaxed);
                match event.event_type {
                    EventType::MouseMove { x, y } => {
                        if let Ok(mut pos) = last_cb.lock() {
                            *pos = (x, y);
                        }
                        if capturing {
                            let _ = recorder_tx.send(RecorderMsg::Move(x, y));
                        }
                        Some(event)
                    }
                    EventType::ButtonPress(Button::Left) => {
                        let (x, y) = last_pos
                            .lock()
                            .map(|p| (p.0, p.1))
                            .unwrap_or((0.0, 0.0));
                        if capturing {
                            let _ = recorder_tx.send(RecorderMsg::Down(x, y));
                            // Swallow so the drag does not click through the overlay.
                            return None;
                        }
                        if scribing {
                            let xi = x as i32;
                            let yi = y as i32;
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
                        Some(event)
                    }
                    EventType::ButtonRelease(Button::Left) => {
                        if capturing {
                            let (x, y) = last_pos
                                .lock()
                                .map(|p| (p.0, p.1))
                                .unwrap_or((0.0, 0.0));
                            let _ = recorder_tx.send(RecorderMsg::Up(x, y));
                            return None;
                        }
                        Some(event)
                    }
                    _ => Some(event),
                }
            };
            if let Err(e) = grab(cb) {
                eprintln!("[mouse_hook] rdev grab error: {e:?}");
            }
        });

        Self {
            recorder_flag,
            scribe_flag,
            scribe_ignore,
        }
    }
}
