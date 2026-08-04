//! Multi-monitor helpers: pick the display under a point and capture it.

use image::RgbaImage;
use xcap::Monitor;

#[derive(Clone)]
pub struct CapturedMonitor {
    pub image: RgbaImage,
    /// Monitor top-left in screen (physical) pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Enable Per-Monitor DPI awareness early so mouse + capture share physical pixels.
#[cfg(windows)]
pub fn enable_dpi_awareness() {
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
        fn SetProcessDPIAware() -> i32;
    }
    unsafe {
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
        if SetProcessDpiAwarenessContext(-4) != 0 {
            return;
        }
        let _ = SetProcessDPIAware();
    }
}

#[cfg(not(windows))]
pub fn enable_dpi_awareness() {}

pub fn cursor_pos() -> (i32, i32) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // Trust GetCursorPos whenever the API succeeds — (0,0) is a valid
        // screen coordinate (top-left). The old `!= 0` check treated origin as
        // failure and returned a stale hook cache position.
        if unsafe { GetCursorPos(&mut pt) }.is_ok() {
            crate::mouse_hook::seed_cursor(pt.x, pt.y);
            return (pt.x, pt.y);
        }
    }
    // Shared with mouse_hook on all platforms (and Windows fallback).
    crate::mouse_hook::last_cursor_pos()
}

fn monitor_contains(mon: &Monitor, x: i32, y: i32) -> bool {
    let mx = mon.x().unwrap_or(0);
    let my = mon.y().unwrap_or(0);
    let mw = mon.width().unwrap_or(0) as i32;
    let mh = mon.height().unwrap_or(0) as i32;
    x >= mx && x < mx + mw && y >= my && y < my + mh
}

/// Prefer the monitor that contains `(x, y)`; fall back to primary, then first.
pub fn monitor_at(x: i32, y: i32) -> Result<Monitor, String> {
    if let Ok(m) = Monitor::from_point(x, y) {
        return Ok(m);
    }
    let monitors = Monitor::all().map_err(|e| format!("monitors: {e}"))?;
    if let Some(m) = monitors.iter().find(|m| monitor_contains(m, x, y)) {
        return Ok(m.clone());
    }
    if let Some(m) = monitors.iter().find(|m| m.is_primary().unwrap_or(false)) {
        return Ok(m.clone());
    }
    monitors
        .into_iter()
        .next()
        .ok_or_else(|| "no monitor".into())
}

pub fn capture_at_point(x: i32, y: i32) -> Result<CapturedMonitor, String> {
    let mon = monitor_at(x, y)?;
    let mx = mon.x().unwrap_or(0);
    let my = mon.y().unwrap_or(0);
    let img = mon.capture_image().map_err(|e| format!("capture: {e}"))?;
    let width = img.width();
    let height = img.height();
    Ok(CapturedMonitor {
        image: img,
        x: mx,
        y: my,
        width,
        height,
    })
}

pub fn capture_under_cursor() -> Result<CapturedMonitor, String> {
    let (x, y) = cursor_pos();
    capture_at_point(x, y)
}

/// Capture every connected monitor (for template search across screens).
pub fn capture_all_monitors() -> Vec<CapturedMonitor> {
    let Ok(monitors) = Monitor::all() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for mon in monitors {
        let Ok(img) = mon.capture_image() else {
            continue;
        };
        out.push(CapturedMonitor {
            width: img.width(),
            height: img.height(),
            image: img,
            x: mon.x().unwrap_or(0),
            y: mon.y().unwrap_or(0),
        });
    }
    out
}
