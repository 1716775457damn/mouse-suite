use crate::common::{data_dir, Config};
use crate::mouse_hook::RecorderMsg;
use crate::theme;
use chrono::Local;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, ScrollArea, Stroke};
use rusqlite::{params, Connection};
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn log_error(msg: &str) {
    let log_path = data_dir().join("error.log");
    let _ = fs::create_dir_all(data_dir());
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(f, "[{}] {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
    }
}

pub fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => s.to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => s.clone(),
                None => format!("{:?}", info),
            },
        };
        let loc = info.location().map(|l| l.to_string()).unwrap_or_default();
        let full = if loc.is_empty() {
            msg
        } else {
            format!("{} at {}", msg, loc)
        };
        log_error(&full);
    }));
}

fn get_next_state_suffix(current: &str) -> String {
    let suffixes = ["-n", "-s", "-h", "-d", "-f", "-a", "-p"];
    for i in 0..suffixes.len() - 1 {
        if current == suffixes[i] {
            return suffixes[i + 1].to_string();
        }
    }
    "-n".to_string()
}

fn get_resolution() -> (u32, u32) {
    let (cx, cy) = crate::screen::cursor_pos();
    crate::screen::monitor_at(cx, cy)
        .ok()
        .map(|mon| (mon.width().unwrap_or(1920), mon.height().unwrap_or(1080)))
        .unwrap_or((1920, 1080))
}

fn load_element_thumb(ctx: &egui::Context, path: &str, id: i32) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = (img.width(), img.height());
    let max_side = 160u32;
    let m = w.max(h).max(1);
    let rgba = if m > max_side {
        let s = max_side as f32 / m as f32;
        let nw = ((w as f32) * s).round().max(1.0) as u32;
        let nh = ((h as f32) * s).round().max(1.0) as u32;
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(
        format!("elem_thumb_{id}"),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

fn init_db(path: &str) -> Connection {
    if let Some(parent) = std::path::Path::new(path).parent() {
        fs::create_dir_all(parent).expect("Failed to create DB directory");
    }
    let conn = Connection::open(path).expect("Failed to open DB");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS elements (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            center_x INTEGER NOT NULL, center_y INTEGER NOT NULL,
            bbox_x INTEGER NOT NULL, bbox_y INTEGER NOT NULL,
            bbox_width INTEGER NOT NULL, bbox_height INTEGER NOT NULL,
            screen_width INTEGER NOT NULL, screen_height INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS element_states (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            element_id INTEGER NOT NULL,
            state_name TEXT NOT NULL, image_path TEXT NOT NULL,
            is_primary BOOLEAN NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (element_id) REFERENCES elements(id)
        );",
    )
    .expect("Fail DB init");
    conn
}

#[derive(Clone)]
struct ElementRec {
    id: i32,
    name: String,
    cx: i32,
    cy: i32,
    states: Vec<(String, bool)>,
    /// Absolute path to primary preview image.
    preview_path: Option<String>,
}

fn load_elements(conn: &Connection, image_dir: &str) -> Vec<ElementRec> {
    let mut res = Vec::new();
    if let Ok(mut st) = conn.prepare("SELECT id,name,center_x,center_y FROM elements ORDER BY id") {
        if let Ok(rows) = st.query_map([], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i32>(2)?,
                r.get::<_, i32>(3)?,
            ))
        }) {
            for row in rows.flatten() {
                let (id, name, cx, cy) = row;
                let mut states = Vec::new();
                if let Ok(mut s2) = conn.prepare(
                    "SELECT state_name,is_primary FROM element_states WHERE element_id=?1 ORDER BY id",
                ) {
                    if let Ok(r2) = s2.query_map(params![id], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
                    }) {
                        for st in r2.flatten() {
                            states.push(st);
                        }
                    }
                }
                let preview_path = conn
                    .query_row(
                        "SELECT image_path FROM element_states WHERE element_id=?1 AND is_primary=1 LIMIT 1",
                        params![id],
                        |r| r.get::<_, String>(0),
                    )
                    .ok()
                    .map(|fname| {
                        let p = std::path::Path::new(image_dir).join(&fname);
                        if p.exists() {
                            p.to_string_lossy().to_string()
                        } else if std::path::Path::new(&fname).exists() {
                            fname
                        } else {
                            p.to_string_lossy().to_string()
                        }
                    });
                res.push(ElementRec {
                    id,
                    name,
                    cx,
                    cy,
                    states,
                    preview_path,
                });
            }
        }
    }
    res
}

fn export_csv(conn: &Connection, name: &str, cfg: &Config) -> Result<String, String> {
    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let out_dir = cfg.image_dir();
    let path = std::path::Path::new(&out_dir).join(format!("{}_{}.csv", name, ts));
    let path_s = path.to_string_lossy().to_string();
    let mut f = fs::File::create(&path).map_err(|e| e.to_string())?;
    f.write_all(b"\xEF\xBB\xBF").map_err(|e| e.to_string())?;
    writeln!(
        f,
        "id,x,y,description,template_path,original_width,original_height"
    )
    .map_err(|e| e.to_string())?;
    let mut st = conn
        .prepare(
            "SELECT e.id,e.name,e.center_x,e.center_y,es.image_path,e.screen_width,e.screen_height
         FROM elements e JOIN element_states es ON e.id=es.element_id AND es.is_primary=1 ORDER BY e.id",
        )
        .map_err(|e| e.to_string())?;
    for row in st
        .query_map([], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i32>(2)?,
                r.get::<_, i32>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i32>(5)?,
                r.get::<_, i32>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let (id, name, cx, cy, img, sw, sh) = row;
        let img_path = std::path::Path::new(&out_dir)
            .join(&img)
            .to_string_lossy()
            .to_string();
        writeln!(
            f,
            "{},{},{},\"{}\",\"{}\",{},{}",
            id,
            cx,
            cy,
            name.replace('"', "\"\""),
            img_path.replace('"', "\"\""),
            sw,
            sh
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(path_s)
}

fn create_elem(
    conn: &Connection,
    name: &str,
    state: &str,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    img: &str,
    sw: u32,
    sh: u32,
) -> Result<i32, String> {
    let cx = x1.min(x2) + (x1 - x2).abs() / 2;
    let cy = y1.min(y2) + (y1 - y2).abs() / 2;
    let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let bx = x1.min(x2);
    let by = y1.min(y2);
    let bw = (x1 - x2).abs() as u32;
    let bh = (y1 - y2).abs() as u32;
    conn.execute(
        "INSERT INTO elements(name,center_x,center_y,bbox_x,bbox_y,bbox_width,bbox_height,screen_width,screen_height,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![name, cx, cy, bx, by, bw, bh, sw, sh, ts],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid() as i32;
    let fname = std::path::Path::new(img)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(img)
        .to_string();
    conn.execute(
        "INSERT INTO element_states(element_id,state_name,image_path,is_primary,created_at) VALUES(?1,?2,?3,1,?4)",
        params![id, state, fname, ts],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

/// Create or replace primary template for a named element.
fn upsert_elem(
    conn: &Connection,
    name: &str,
    state: &str,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    img: &str,
    sw: u32,
    sh: u32,
) -> Result<i32, String> {
    let existing: Option<i32> = conn
        .query_row(
            "SELECT id FROM elements WHERE name=?1 LIMIT 1",
            params![name],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing {
        let cx = x1.min(x2) + (x1 - x2).abs() / 2;
        let cy = y1.min(y2) + (y1 - y2).abs() / 2;
        let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let bx = x1.min(x2);
        let by = y1.min(y2);
        let bw = (x1 - x2).abs() as u32;
        let bh = (y1 - y2).abs() as u32;
        conn.execute(
            "UPDATE elements SET center_x=?1,center_y=?2,bbox_x=?3,bbox_y=?4,bbox_width=?5,bbox_height=?6,screen_width=?7,screen_height=?8 WHERE id=?9",
            params![cx, cy, bx, by, bw, bh, sw, sh, id],
        )
        .map_err(|e| e.to_string())?;
        let fname = std::path::Path::new(img)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(img)
            .to_string();
        let _ = conn.execute(
            "UPDATE element_states SET is_primary=0 WHERE element_id=?1",
            params![id],
        );
        conn.execute(
            "INSERT INTO element_states(element_id,state_name,image_path,is_primary,created_at) VALUES(?1,?2,?3,1,?4)",
            params![id, state, fname, ts],
        )
        .map_err(|e| e.to_string())?;
        Ok(id)
    } else {
        create_elem(conn, name, state, x1, y1, x2, y2, img, sw, sh)
    }
}

enum AppMode {
    Normal,
    Capturing,
    InputName(i32, i32, i32, i32, String),
    InputState(i32, i32, i32, i32, String, String, String),
    InputExport,
}

pub struct RecorderApp {
    conn: Connection,
    config: Config,
    elements: Vec<ElementRec>,
    /// Thumbnail textures aligned with `elements` indices.
    previews: Vec<Option<egui::TextureHandle>>,
    last_id: Option<i32>,
    last_element_name: Option<String>,
    last_state_name: Option<String>,
    is_add_state_mode: bool,
    sw: u32,
    sh: u32,
    scale: f32,
    mode: AppMode,
    input: String,
    status: String,
    rx: mpsc::Receiver<RecorderMsg>,
    capture_flag: Arc<AtomicBool>,
    drag_start: Option<(i32, i32)>,
    drag_current: Option<(i32, i32)>,
    mouse_pos: Option<(i32, i32)>,
    capture_started_at: Option<std::time::Instant>,
    forced_state_name: Option<String>,
    /// When set, skip name dialog and upsert this element after crop.
    forced_element_name: Option<String>,
    /// Hide window first; after delay, grab desktop then show overlay.
    hide_started: Option<std::time::Instant>,
    pending_is_add_state: bool,
    image_dir: String,
    pre_capture: Option<(egui::TextureHandle, image::RgbaImage)>,
    /// Top-left of the captured monitor in screen pixels (multi-mon crop/overlay).
    capture_origin: (i32, i32),
    filter: String,
}

impl RecorderApp {
    pub fn new(
        config: Config,
        capture_flag: Arc<AtomicBool>,
        rx: mpsc::Receiver<RecorderMsg>,
    ) -> Self {
        let db_path = config.db_path();
        let image_dir = config.image_dir();
        let _ = fs::create_dir_all(&image_dir);
        let conn = init_db(&db_path);
        let (sw, sh) = get_resolution();
        let elems = load_elements(&conn, &image_dir);
        let last = elems.last().map(|e| e.id);
        Self {
            conn,
            config,
            image_dir,
            elements: elems,
            previews: Vec::new(),
            last_id: last,
            last_element_name: None,
            last_state_name: None,
            is_add_state_mode: false,
            sw,
            sh,
            scale: 1.0,
            mode: AppMode::Normal,
            input: String::new(),
            status: "Ready. Click [New Element] to record".into(),
            rx,
            capture_flag,
            drag_start: None,
            drag_current: None,
            mouse_pos: None,
            capture_started_at: None,
            forced_state_name: None,
            forced_element_name: None,
            hide_started: None,
            pending_is_add_state: false,
            pre_capture: None,
            capture_origin: (0, 0),
            filter: String::new(),
        }
    }

    pub fn is_capturing(&self) -> bool {
        matches!(self.mode, AppMode::Capturing) || self.hide_started.is_some()
    }

    /// Agent entrypoint: current human-readable recorder status.
    pub fn status_text(&self) -> &str {
        &self.status
    }

    /// Agent entrypoint: latest cached element count.
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Names of recorded elements (for flow pickers).
    pub fn element_names(&self) -> Vec<String> {
        self.elements.iter().map(|e| e.name.clone()).collect()
    }

    /// Catalog with preview paths for flow visual picker.
    pub fn element_catalog(&self) -> Vec<crate::common::ElementCatalogItem> {
        self.elements
            .iter()
            .map(|e| crate::common::ElementCatalogItem {
                name: e.name.clone(),
                preview_path: e.preview_path.clone().unwrap_or_default(),
            })
            .collect()
    }

    /// Agent entrypoint: refresh local list from DB.
    pub fn agent_refresh(&mut self) {
        self.refresh();
        self.status = "Refreshed by agent".into();
    }

    /// Agent entrypoint: start a new-element capture flow.
    pub fn agent_start_new_capture(&mut self, ctx: &egui::Context) {
        self.forced_state_name = None;
        self.forced_element_name = None;
        self.begin_capture(ctx, false);
    }

    /// Start capture bound to a fixed element name (flow click node screenshot).
    pub fn start_named_capture_after_hide(&mut self, ctx: &egui::Context, element_name: String) {
        self.forced_state_name = None;
        self.forced_element_name = Some(element_name);
        self.begin_capture(ctx, false);
    }

    /// Whether an element with this name already exists in the library.
    pub fn has_element(&self, name: &str) -> bool {
        self.elements.iter().any(|e| e.name == name)
    }

    /// Drop a leftover forced name (e.g. cancelled precise capture during flow record).
    pub fn clear_forced_element_name(&mut self) {
        self.forced_element_name = None;
    }

    /// Auto-crop a template around a screen click and upsert as `name` (state `-n`).
    /// Same ROI + screen metadata path as marquee capture (`upsert_elem`).
    /// Used by Flow page click-recording (auto names like `click_YYYYMMDD_HHMMSS_001`).
    pub fn save_click_crop(&mut self, name: &str, screen_x: i32, screen_y: i32) -> Result<(), String> {
        // ~160×96 around click — large enough for typical buttons, small enough for NCC.
        const HALF_W: i32 = 80;
        const HALF_H: i32 = 48;
        // Let the pressed UI settle before grabbing pixels.
        thread::sleep(Duration::from_millis(70));
        let cap = crate::screen::capture_at_point(screen_x, screen_y)?;
        let lx = (screen_x - cap.x).clamp(0, cap.width.saturating_sub(1) as i32);
        let ly = (screen_y - cap.y).clamp(0, cap.height.saturating_sub(1) as i32);
        let x1 = (lx - HALF_W).max(0);
        let y1 = (ly - HALF_H).max(0);
        let x2 = (lx + HALF_W).min(cap.width as i32);
        let y2 = (ly + HALF_H).min(cap.height as i32);
        if x2 - x1 < 8 || y2 - y1 < 8 {
            return Err("crop too small".into());
        }
        let cw = (x2 - x1) as u32;
        let ch = (y2 - y1) as u32;
        let mut cropped = image::RgbaImage::new(cw, ch);
        for py in 0..ch {
            for px in 0..cw {
                let p = *cap.image.get_pixel((x1 as u32) + px, (y1 as u32) + py);
                cropped.put_pixel(px, py, p);
            }
        }
        let state_name = "-n";
        let dest = std::path::Path::new(&self.image_dir).join(format!("{}_{}.png", name, state_name));
        cropped
            .save(&dest)
            .map_err(|e| format!("save template: {e}"))?;
        // Absolute screen coords for ROI matching.
        let abs_x1 = cap.x + x1;
        let abs_y1 = cap.y + y1;
        let abs_x2 = cap.x + x2;
        let abs_y2 = cap.y + y2;
        upsert_elem(
            &self.conn,
            name,
            state_name,
            abs_x1,
            abs_y1,
            abs_x2,
            abs_y2,
            &dest.to_string_lossy(),
            cap.width,
            cap.height,
        )?;
        self.refresh();
        self.status = format!("已保存点击模板「{}」", name);
        Ok(())
    }

    /// Agent entrypoint: start add-state capture for current element.
    pub fn agent_start_add_state_capture(
        &mut self,
        ctx: &egui::Context,
        forced_state: Option<String>,
    ) {
        self.forced_state_name = forced_state;
        self.forced_element_name = None;
        self.begin_capture(ctx, true);
    }

    /// Agent entrypoint: export current primary templates to CSV.
    pub fn agent_export_csv(&mut self, name: &str) -> Result<String, String> {
        let path = export_csv(&self.conn, name, &self.config)?;
        self.status = format!("Exported by agent: {}", path);
        Ok(path)
    }

    fn refresh(&mut self) {
        self.elements = load_elements(&self.conn, &self.image_dir);
        self.last_id = self.elements.last().map(|e| e.id);
        self.previews.clear();
    }

    fn ensure_previews(&mut self, ctx: &egui::Context) {
        if self.previews.len() == self.elements.len() {
            return;
        }
        self.previews.clear();
        for e in &self.elements {
            let tex = e
                .preview_path
                .as_ref()
                .and_then(|p| load_element_thumb(ctx, p, e.id));
            self.previews.push(tex);
        }
    }

    /// Hide UI first; actual desktop grab happens after window is fully gone.
    fn begin_capture(&mut self, ctx: &egui::Context, is_add_state: bool) {
        self.pending_is_add_state = is_add_state;
        self.drag_start = None;
        self.drag_current = None;
        self.mouse_pos = None;
        self.pre_capture = None;
        self.capture_flag.store(false, Ordering::Relaxed);
        let wait_ms = self.config.hide_wait_ms();
        self.hide_started = Some(std::time::Instant::now());
        self.status = format!("正在隐藏窗口（{}ms），完全消失后再截屏…", wait_ms);
        log_error(&format!("begin_capture: hide wait {}ms", wait_ms));

        // Minimize only — do NOT set Visible(false).
        // Visible(false) can stall the egui event loop so tick_hide_then_capture never runs.
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        // Park the window off-screen so it doesn't flash into the desktop grab.
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            -20000.0, -20000.0,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1.0, 1.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));

        // Wake the UI after the hide delay even if the window is minimized.
        let ctx_wake = ctx.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(wait_ms));
            ctx_wake.request_repaint();
        });
        ctx.request_repaint_after(Duration::from_millis(wait_ms.min(200).max(50)));
    }

    /// Call every frame: after hide delay, capture desktop then open overlay.
    pub fn tick_hide_then_capture(&mut self, ctx: &egui::Context) {
        let Some(started) = self.hide_started else {
            return;
        };
        let wait = Duration::from_millis(self.config.hide_wait_ms());
        let elapsed = started.elapsed();
        if elapsed < wait {
            // Keep requesting repaints while waiting (minimized windows may throttle).
            let remain = wait.saturating_sub(elapsed);
            ctx.request_repaint_after(remain.min(Duration::from_millis(100)));
            return;
        }
        self.hide_started = None;
        log_error("tick_hide_then_capture: starting overlay grab");
        self.start_overlay_after_grab(ctx, self.pending_is_add_state);
    }

    pub fn hide_wait_ms(&self) -> u64 {
        self.config.hide_wait_ms()
    }

    pub fn set_hide_wait_ms(&mut self, ms: u64) {
        self.config.hide_wait_ms = ms.clamp(500, 3000);
        self.config.save();
    }

    fn start_overlay_after_grab(&mut self, ctx: &egui::Context, is_add_state: bool) {
        self.capture_flag.store(true, Ordering::Relaxed);
        self.drag_start = None;
        self.drag_current = None;
        self.capture_started_at = Some(std::time::Instant::now());
        self.is_add_state_mode = is_add_state;
        self.status = "Capturing desktop...".into();
        self.scale = ctx.pixels_per_point();

        self.pre_capture = None;
        match crate::screen::capture_under_cursor() {
            Ok(cap) => {
                self.capture_origin = (cap.x, cap.y);
                self.sw = cap.width;
                self.sh = cap.height;
                let size = [cap.image.width() as usize, cap.image.height() as usize];
                let pixels: Vec<egui::Color32> = cap
                    .image
                    .pixels()
                    .map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], 255))
                    .collect();
                let color_img = egui::ColorImage { size, pixels };
                let tex = ctx.load_texture("desktop-bg", color_img, egui::TextureOptions::LINEAR);
                self.pre_capture = Some((tex, cap.image));
                self.status = format!(
                    "在当前屏框选（原点 {},{} · {}×{}）…",
                    cap.x, cap.y, cap.width, cap.height
                );
                log_error(&format!(
                    "start_overlay_after_grab: mon ({},{}) {}x{}",
                    cap.x, cap.y, cap.width, cap.height
                ));
            }
            Err(e) => {
                log_error(&format!("start_overlay_after_grab: {e}"));
            }
        }
        if self.pre_capture.is_none() {
            self.status = "Failed to capture desktop".into();
            log_error("start_overlay_after_grab: capture_image failed");
            self.capture_flag.store(false, Ordering::Relaxed);
            self.forced_element_name = None;
            self.mode = AppMode::Normal;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(700.0, 550.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                100.0, 100.0,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            return;
        }

        // Show fullscreen selection overlay only after grab succeeded.
        log_error("start_overlay_after_grab: overlay ready");
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.mode = AppMode::Capturing;
        let (ox, oy) = self.capture_origin;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            ox as f32 / self.scale,
            oy as f32 / self.scale,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            self.sw as f32 / self.scale,
            self.sh as f32 / self.scale,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn end_capture(&mut self, ctx: &egui::Context) {
        self.capture_flag.store(false, Ordering::Relaxed);
        self.capture_started_at = None;
        self.drag_start = None;
        self.drag_current = None;
        self.mouse_pos = None;
        self.pre_capture = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::Normal,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(700.0, 550.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            100.0, 100.0,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Critical,
        ));
        log_error("end_capture done: restored window");
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(16));
        self.scale = ctx.pixels_per_point();

        match &self.mode {
            AppMode::Capturing => {
                if let Some(started) = self.capture_started_at {
                    if started.elapsed().as_secs() >= 30 {
                        self.end_capture(ctx);
                        self.mode = AppMode::Normal;
                        self.status = "Capture timed out (30s)".into();
                    }
                }
                while let Ok(msg) = self.rx.try_recv() {
                    match msg {
                        RecorderMsg::Down(x, y) => {
                            self.drag_start = Some((x as i32, y as i32));
                            self.drag_current = Some((x as i32, y as i32));
                            self.status = format!(
                                "Drag started ({},{}), release to finish",
                                x as i32, y as i32
                            );
                        }
                        RecorderMsg::Move(x, y) => {
                            self.mouse_pos = Some((x as i32, y as i32));
                            if self.drag_start.is_some() {
                                self.drag_current = Some((x as i32, y as i32));
                            }
                        }
                        RecorderMsg::Up(x, y) => {
                            log_error("capture up start");
                            let saved_start = self.drag_start.take();
                            let pre_img = self.pre_capture.take();
                            self.capture_flag.store(false, Ordering::Relaxed);
                            self.capture_started_at = None;
                            self.drag_start = None;
                            self.drag_current = None;
                            self.mouse_pos = None;

                            let result = match (saved_start, pre_img) {
                                (Some((sx, sy)), Some(full)) => {
                                    let (ox, oy) = self.capture_origin;
                                    // Absolute screen coords → image-local pixels
                                    let lx0 = sx.min(x as i32) - ox;
                                    let ly0 = sy.min(y as i32) - oy;
                                    let lx1 = sx.max(x as i32) - ox;
                                    let ly1 = sy.max(y as i32) - oy;
                                    let px = lx0.max(0);
                                    let py = ly0.max(0);
                                    let pw = (lx1 - lx0).max(0) as u32;
                                    let ph = (ly1 - ly0).max(0) as u32;
                                    // Absolute bbox for clicker / element DB
                                    let abs_x1 = sx.min(x as i32);
                                    let abs_y1 = sy.min(y as i32);
                                    let abs_x2 = sx.max(x as i32);
                                    let abs_y2 = sy.max(y as i32);
                                    if pw == 0 || ph == 0 {
                                        Err("Zero region".into())
                                    } else {
                                        let lpx = (px as u32).min(full.1.width().saturating_sub(1));
                                        let lpy =
                                            (py as u32).min(full.1.height().saturating_sub(1));
                                        let lpw = pw.min(full.1.width().saturating_sub(lpx));
                                        let lph = ph.min(full.1.height().saturating_sub(lpy));
                                        if lpw == 0 || lph == 0 {
                                            Err("Region out of bounds".into())
                                        } else {
                                            let ts = Local::now().format("%Y%m%d_%H%M%S_%3f");
                                            let img_dir = &self.image_dir;
                                            let _ = fs::create_dir_all(img_dir);
                                            let path = std::path::Path::new(img_dir)
                                                .join(format!("capture_{}.png", ts));
                                            let path_s = path.to_string_lossy().to_string();
                                            let mut cropped = image::ImageBuffer::new(lpw, lph);
                                            for dy in 0..lph {
                                                for dx in 0..lpw {
                                                    cropped.put_pixel(
                                                        dx,
                                                        dy,
                                                        *full.1.get_pixel(lpx + dx, lpy + dy),
                                                    );
                                                }
                                            }
                                            match cropped.save(&path) {
                                                Ok(_) => {
                                                    Ok((abs_x1, abs_y1, abs_x2, abs_y2, path_s))
                                                }
                                                Err(e) => Err(format!("Save failed: {}", e)),
                                            }
                                        }
                                    }
                                }
                                (None, _) => Err("Cancelled".into()),
                                (_, None) => Err("No pre-captured image".into()),
                            };

                            self.end_capture(ctx);

                            match result {
                                Ok((x1, y1, x2, y2, img_path)) => {
                                    log_error(&format!("capture success: {}", img_path));
                                    if self.is_add_state_mode {
                                        if let (Some(elem_name), Some(last_state)) = (
                                            self.last_element_name.clone(),
                                            self.last_state_name.clone(),
                                        ) {
                                            let next_state =
                                                self.forced_state_name.take().unwrap_or_else(
                                                    || get_next_state_suffix(&last_state),
                                                );
                                            let new_path = std::path::Path::new(&self.image_dir)
                                                .join(format!("{}_{}.png", elem_name, next_state))
                                                .to_string_lossy()
                                                .to_string();
                                            let img_path =
                                                if std::fs::rename(&img_path, &new_path).is_ok() {
                                                    new_path
                                                } else {
                                                    img_path
                                                };
                                            let fname = std::path::Path::new(&img_path)
                                                .file_name()
                                                .and_then(|s| s.to_str())
                                                .unwrap_or(&img_path)
                                                .to_string();
                                            if let Some(id) = self.last_id {
                                                let ts = Local::now()
                                                    .format("%Y%m%d_%H%M%S")
                                                    .to_string();
                                                match self.conn.execute(
                                                    "INSERT INTO element_states(element_id,state_name,image_path,is_primary,created_at) VALUES(?1,?2,?3,0,?4)",
                                                    params![id, next_state, fname, ts],
                                                ) {
                                                    Ok(_) => {
                                                        self.last_state_name =
                                                            Some(next_state.clone());
                                                        self.status =
                                                            format!("State '{}' added!", next_state);
                                                        self.refresh();
                                                    }
                                                    Err(e) => {
                                                        self.status = format!("Error: {}", e)
                                                    }
                                                }
                                            }
                                        } else {
                                            self.status = "No element selected".into();
                                        }
                                        self.mode = AppMode::Normal;
                                    } else if let Some(name) = self.forced_element_name.take() {
                                        let state_name = "-n".to_string();
                                        let new_path = std::path::Path::new(&self.image_dir)
                                            .join(format!("{}_{}.png", name, state_name))
                                            .to_string_lossy()
                                            .to_string();
                                        let img_path =
                                            if std::fs::rename(&img_path, &new_path).is_ok() {
                                                new_path
                                            } else {
                                                img_path
                                            };
                                        match upsert_elem(
                                            &self.conn,
                                            &name,
                                            &state_name,
                                            x1,
                                            y1,
                                            x2,
                                            y2,
                                            &img_path,
                                            self.sw,
                                            self.sh,
                                        ) {
                                            Ok(id) => {
                                                self.last_id = Some(id);
                                                self.last_element_name = Some(name.clone());
                                                self.last_state_name = Some(state_name);
                                                self.status =
                                                    format!("已绑定模板「{}」(#{})", name, id);
                                                self.refresh();
                                            }
                                            Err(e) => self.status = format!("Error: {}", e),
                                        }
                                        self.mode = AppMode::Normal;
                                    } else {
                                        self.mode = AppMode::InputName(x1, y1, x2, y2, img_path);
                                        self.input.clear();
                                        self.status = "Enter element name:".into();
                                    }
                                }
                                Err(e) => {
                                    self.forced_element_name = None;
                                    self.mode = AppMode::Normal;
                                    self.status = e;
                                }
                            }
                        }
                    }
                }

                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(Color32::BLACK))
                    .show(ctx, |ui| {
                        let painter = ui.painter();
                        let sw = self.sw as f32 / self.scale;
                        let sh = self.sh as f32 / self.scale;
                        let (ox, oy) = self.capture_origin;
                        let scale = self.scale.max(0.01);
                        // Absolute screen pixels → overlay-local egui points
                        let to_local = |sx: i32, sy: i32| -> (f32, f32) {
                            ((sx - ox) as f32 / scale, (sy - oy) as f32 / scale)
                        };

                        if let Some((tex, _)) = &self.pre_capture {
                            let bg_rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(sw, sh));
                            painter.image(
                                tex.id(),
                                bg_rect,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }

                        let dim = Color32::from_black_alpha(160);
                        let cross_color = Color32::from_rgba_premultiplied(255, 255, 255, 180);

                        if let (Some((sx, sy)), Some((cx, cy))) =
                            (self.drag_start, self.drag_current)
                        {
                            let (x1a, y1a) = to_local(sx.min(cx), sy.min(cy));
                            let (x2a, y2a) = to_local(sx.max(cx), sy.max(cy));
                            let x1 = x1a.clamp(0.0, sw);
                            let y1 = y1a.clamp(0.0, sh);
                            let x2 = x2a.clamp(0.0, sw);
                            let y2 = y2a.clamp(0.0, sh);

                            painter.rect_filled(
                                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(sw, y1)),
                                0.0,
                                dim,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(Pos2::new(0.0, y2), Pos2::new(sw, sh)),
                                0.0,
                                dim,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(Pos2::new(0.0, y1), Pos2::new(x1, y2)),
                                0.0,
                                dim,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(Pos2::new(x2, y1), Pos2::new(sw, y2)),
                                0.0,
                                dim,
                            );

                            let sel = Rect::from_min_max(Pos2::new(x1, y1), Pos2::new(x2, y2));
                            painter.rect_stroke(sel, 0.0, Stroke::new(2.0, Color32::WHITE));
                            painter.rect_stroke(
                                sel,
                                0.0,
                                Stroke::new(1.0, theme::col().ACCENT),
                            );

                            for (px, py) in [(x1, y1), (x2, y1), (x1, y2), (x2, y2)] {
                                painter.rect_filled(
                                    Rect::from_center_size(Pos2::new(px, py), egui::vec2(8.0, 8.0)),
                                    0.0,
                                    Color32::WHITE,
                                );
                            }
                            for (px, py) in [
                                (x1, (y1 + y2) / 2.0),
                                (x2, (y1 + y2) / 2.0),
                                ((x1 + x2) / 2.0, y1),
                                ((x1 + x2) / 2.0, y2),
                            ] {
                                painter.rect_filled(
                                    Rect::from_center_size(Pos2::new(px, py), egui::vec2(6.0, 6.0)),
                                    0.0,
                                    Color32::WHITE,
                                );
                            }

                            let w = (sx.max(cx) - sx.min(cx)).abs();
                            let h = (sy.max(cy) - sy.min(cy)).abs();
                            let dims = format!(" {}x{} ", w, h);
                            let lx = if x2 + 4.0 + dims.len() as f32 * 8.5 < sw {
                                x2 + 4.0
                            } else {
                                x1
                            };
                            let ly = if y2 + 24.0 < sh { y2 + 4.0 } else { y1 - 24.0 };
                            let label_bg = Rect::from_min_size(
                                Pos2::new(lx, ly),
                                egui::vec2(dims.len() as f32 * 8.5, 20.0),
                            );
                            painter.rect_filled(label_bg, 3.0, theme::col().ACCENT);
                            painter.text(
                                label_bg.center(),
                                egui::Align2::CENTER_CENTER,
                                &dims,
                                FontId::proportional(14.0),
                                Color32::WHITE,
                            );
                        } else {
                            painter.rect_filled(
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(sw, sh)),
                                0.0,
                                dim,
                            );

                            if let Some((mx, my)) = self.mouse_pos {
                                let (mx, my) = to_local(mx, my);
                                painter.line_segment(
                                    [Pos2::new(0.0, my), Pos2::new(sw, my)],
                                    Stroke::new(1.0, cross_color),
                                );
                                painter.line_segment(
                                    [Pos2::new(mx, 0.0), Pos2::new(mx, sh)],
                                    Stroke::new(1.0, cross_color),
                                );

                                let coord = format!(" ({}, {}) ", mx as i32, my as i32);
                                let tip_x = if mx + 80.0 < sw {
                                    mx + 12.0
                                } else {
                                    mx - 12.0 - coord.len() as f32 * 7.5
                                };
                                let tip_y = if my + 28.0 < sh { my + 12.0 } else { my - 28.0 };
                                let tip_bg = Rect::from_min_size(
                                    Pos2::new(tip_x, tip_y),
                                    egui::vec2(coord.len() as f32 * 7.5, 20.0),
                                );
                                painter.rect_filled(tip_bg, 3.0, Color32::from_black_alpha(200));
                                painter.text(
                                    tip_bg.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &coord,
                                    FontId::proportional(13.0),
                                    Color32::WHITE,
                                );
                            }

                            let hint = "Click and drag to select   ESC to cancel";
                            painter.text(
                                Pos2::new(sw / 2.0, sh - 40.0),
                                egui::Align2::CENTER_CENTER,
                                hint,
                                FontId::proportional(16.0),
                                Color32::from_white_alpha(200),
                            );
                        }

                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.end_capture(ctx);
                            self.mode = AppMode::Normal;
                            self.status = "Cancelled".into();
                        }
                    });
            }
            _ => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(theme::col().BG))
                    .show(ctx, |ui| {
                        theme::paint_atmosphere(ui);
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(216.0, ui.available_height()),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::none()
                                        .fill(theme::col().CHROME)
                                        .stroke(egui::Stroke::new(1.0, theme::col().PANEL_EDGE))
                                        .rounding(egui::Rounding::same(8.0))
                                        .inner_margin(egui::Margin::same(16.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new("元素资源")
                                                    .size(16.0)
                                                    .strong()
                                                    .color(theme::col().TEXT),
                                            );
                                            ui.label(
                                                egui::RichText::new("视觉自动化素材库")
                                                    .size(11.0)
                                                    .color(theme::col().MUTED),
                                            );
                                            ui.add_space(8.0);
                                            theme::status_pill(
                                                ui,
                                                &format!("{} 个已录制元素", self.elements.len()),
                                                theme::StatusTone::Idle,
                                            );
                                            ui.add_space(14.0);
                                            egui::Frame::none()
                                                .fill(theme::col().ACCENT_HOT)
                                                .rounding(egui::Rounding::same(8.0))
                                                .inner_margin(egui::Margin::same(12.0))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new("捕捉新元素")
                                                            .size(14.0)
                                                            .strong()
                                                            .color(egui::Color32::WHITE),
                                                    );
                                                    ui.add_space(2.0);
                                                    ui.label(
                                                        egui::RichText::new(crate::i18n::t("recorder.header.subtitle"))
                                                            .size(11.0)
                                                            .color(egui::Color32::from_white_alpha(215)),
                                                    );
                                                    ui.add_space(10.0);
                                                    let button = egui::Button::new(
                                                        egui::RichText::new(crate::i18n::t("recorder.capture.start"))
                                                            .strong()
                                                            .color(theme::col().ACCENT_HOT),
                                                    )
                                                    .fill(egui::Color32::WHITE)
                                                    .stroke(egui::Stroke::NONE)
                                                    .min_size(egui::vec2(ui.available_width(), theme::CTRL_H));
                                                    if ui.add(button).clicked() {
                                                        self.forced_state_name = None;
                                                        self.begin_capture(ctx, false);
                                                    }
                                                });
                                            ui.add_space(14.0);
                                            theme::hairline(ui);
                                            theme::field_label(ui, "工作区");
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{} x {}",
                                                    self.sw, self.sh
                                                ))
                                                .size(13.0)
                                                .strong()
                                                .color(theme::col().TEXT),
                                            );
                                            ui.add_space(14.0);
                                            theme::hairline(ui);
                                            theme::field_label(ui, "当前选择");
                                            if let Some(name) = &self.last_element_name {
                                                ui.label(
                                                    egui::RichText::new(name)
                                                        .size(13.0)
                                                        .strong()
                                                        .color(theme::col().ACCENT_DIM),
                                                );
                                            } else {
                                                ui.label(
                                                    egui::RichText::new("从右侧选择一个元素")
                                                        .size(11.0)
                                                        .color(theme::col().MUTED),
                                                );
                                            }
                                        });
                                },
                            );
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                egui::Frame::none()
                                    .inner_margin(egui::Margin::symmetric(20.0, 18.0))
                                    .show(ui, |ui| {
                                        ui.add_space(2.0);
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                theme::section_header(
                                                    ui,
                                                    crate::i18n::t("recorder.header.title"),
                                                    &format!(
                                                        "屏幕 {}×{} · {} 个元素 · 框选区域保存为模板",
                                                        self.sw,
                                                        self.sh,
                                                        self.elements.len()
                                                    ),
                                                );
                                            });
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if theme::primary_button(ui, crate::i18n::t("recorder.btn.new")).clicked()
                                                        || ui.input(|i| i.key_pressed(egui::Key::F5))
                                                    {
                                                        self.forced_state_name = None;
                                                        self.begin_capture(ctx, false);
                                                    }
                                                    ui.add_space(6.0);
                                                    if theme::secondary_button(ui, crate::i18n::t("recorder.btn.refresh")).clicked() {
                                                        self.refresh();
                                                        self.status = "已刷新".into();
                                                    }
                                                },
                                            );
                                        });

                                        self.ensure_previews(ctx);
                                        let matching_count = {
                                            let filter = self.filter.to_lowercase();
                                            self.elements
                                                .iter()
                                                .filter(|element| {
                                                    filter.is_empty()
                                                        || element
                                                            .name
                                                            .to_lowercase()
                                                            .contains(&filter)
                                                })
                                                .count()
                                        };

                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(crate::i18n::t("recorder.filter"))
                                                    .size(12.0)
                                                    .color(theme::col().MUTED),
                                            );
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.filter)
                                                    .desired_width(220.0)
                                                    .hint_text(crate::i18n::t("recorder.filter.hint")),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    theme::status_pill(
                                                        ui,
                                                        &format!("{} 个匹配", matching_count),
                                                        theme::StatusTone::Idle,
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(8.0);

                                        theme::inset_frame().show(ui, |ui| {
                                            let filter = self.filter.to_lowercase();
                                            let indices: Vec<usize> = self
                                                .elements
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, e)| {
                                                    filter.is_empty()
                                                        || e.name.to_lowercase().contains(&filter)
                                                })
                                                .map(|(i, _)| i)
                                                .collect();

                                            // Leave room for the action rows so they stay visible at the
                                            // minimum window height, while larger windows reveal more items.
                                            // Use most of the remaining column; expand on taller windows.
                                            let gallery_height =
                                                (ui.available_height() - 140.0).clamp(140.0, 900.0);
                                            ScrollArea::vertical().max_height(gallery_height).show(
                                                ui,
                                                |ui| {
                                                    if indices.is_empty() {
                                                        theme::empty_state(
                                                            ui,
                                                            if self.elements.is_empty() {
                                                                crate::i18n::t("recorder.empty")
                                                            } else {
                                                                crate::i18n::t("recorder.no_match")
                                                            },
                                                            if self.elements.is_empty() {
                                                                "使用左侧「捕捉新元素」框选屏幕区域"
                                                            } else {
                                                                "试试清空搜索，或换个关键词"
                                                            },
                                                        );
                                                        return;
                                                    }

                                                    let avail = ui.available_width();
                                                    let min_card_w = 164.0_f32;
                                                    let gap = 10.0_f32;
                                                    let cols = ((avail + gap) / (min_card_w + gap))
                                                        .floor()
                                                        .max(1.0)
                                                        as usize;
                                                    let card_w = (avail
                                                        - gap * (cols.saturating_sub(1) as f32))
                                                        / cols as f32;

                                                    egui::Grid::new("element_gallery")
                                                        .num_columns(cols)
                                                        .min_col_width(card_w)
                                                        .max_col_width(card_w)
                                                        .spacing([gap, gap])
                                                        .show(ui, |ui| {
                                                            for (n, &i) in
                                                                indices.iter().enumerate()
                                                            {
                                                                let selected =
                                                                    Some(self.elements[i].id)
                                                                        == self.last_id;
                                                                let name =
                                                                    self.elements[i].name.clone();
                                                                let id = self.elements[i].id;
                                                                let cx = self.elements[i].cx;
                                                                let cy = self.elements[i].cy;
                                                                let n_states =
                                                                    self.elements[i].states.len();
                                                                let tex = self
                                                                    .previews
                                                                    .get(i)
                                                                    .and_then(|t| t.as_ref());

                                                                let resp = theme::panel_frame()
                                                                    .show(ui, |ui| {
                                                                        let content_w = (card_w
                                                                            - 28.0)
                                                                            .max(112.0);
                                                                        ui.set_min_width(content_w);
                                                                        ui.set_max_width(content_w);
                                                                        ui.set_min_height(172.0);
                                                                        ui.with_layout(
                                                        egui::Layout::top_down(egui::Align::Min),
                                                        |ui| {

                                                    let thumb = egui::vec2(content_w, 92.0);
                                                    let (rect, _) = ui.allocate_exact_size(
                                                        thumb,
                                                        egui::Sense::hover(),
                                                    );
                                                    ui.painter().rect_filled(
                                                        rect,
                                                        6.0,
                                                        theme::col().INSET,
                                                    );
                                                    if let Some(tex) = tex {
                                                        let size = tex.size_vec2();
                                                        let s = (thumb.x / size.x)
                                                            .min(thumb.y / size.y)
                                                            .min(1.0);
                                                        let disp = size * s;
                                                        let img_rect = egui::Rect::from_center_size(
                                                            rect.center(),
                                                            disp,
                                                        );
                                                        ui.painter().image(
                                                            tex.id(),
                                                            img_rect,
                                                            egui::Rect::from_min_max(
                                                                egui::pos2(0.0, 0.0),
                                                                egui::pos2(1.0, 1.0),
                                                            ),
                                                            egui::Color32::WHITE,
                                                        );
                                                    } else {
                                                        ui.painter().text(
                                                            rect.center(),
                                                            egui::Align2::CENTER_CENTER,
                                                            "无图",
                                                            egui::FontId::proportional(12.0),
                                                            theme::col().FAINT,
                                                        );
                                                    }

                                                    ui.add_space(8.0);
                                                    ui.label(
                                                        egui::RichText::new(&name)
                                                            .size(13.0)
                                                            .strong()
                                                            .color(if selected {
                                                                theme::col().ACCENT
                                                            } else {
                                                                theme::col().TEXT
                                                            }),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "#{id} · ({cx},{cy}) · {n_states}态"
                                                        ))
                                                        .size(11.0)
                                                        .color(theme::col().MUTED),
                                                    );
                                                        },
                                                    );
                                                                    });

                                                                let click = ui.interact(
                                                                    resp.response.rect,
                                                                    ui.id().with(("el_card", id)),
                                                                    egui::Sense::click(),
                                                                );
                                                                if selected {
                                                                    ui.painter().rect_stroke(
                                                                        resp.response.rect,
                                                                        8.0,
                                                                        egui::Stroke::new(
                                                                            2.0,
                                                                            theme::col().ACCENT,
                                                                        ),
                                                                    );
                                                                } else if click.hovered() {
                                                                    ui.painter().rect_stroke(
                                                                resp.response.rect,
                                                                8.0,
                                                                egui::Stroke::new(
                                                                    1.0,
                                                                    theme::col().PANEL_EDGE,
                                                                ),
                                                            );
                                                                }
                                                                if click.clicked() {
                                                                    self.last_id = Some(id);
                                                                    self.last_element_name =
                                                                        Some(name);
                                                                    self.status =
                                                                        format!("已选中 #{id}");
                                                                }

                                                                if (n + 1) % cols == 0 {
                                                                    ui.end_row();
                                                                }
                                                            }
                                                        });
                                                },
                                            );
                                        });

                                        ui.add_space(8.0);
                                        ui.separator();
                                        ui.colored_label(theme::col().SUCCESS, &self.status);

                                        match &self.mode {
                                            AppMode::Normal => {
                                                theme::toolbar_row(ui, |ui| {
                                                    if self.last_id.is_some() {
                                                        if theme::secondary_button(
                                                            ui,
                                                            crate::i18n::t("recorder.btn.add_state"),
                                                        )
                                                        .clicked()
                                                            || ui.input(|i| {
                                                                i.key_pressed(egui::Key::F6)
                                                            })
                                                        {
                                                            self.forced_state_name = None;
                                                            self.begin_capture(ctx, true);
                                                        }
                                                    }
                                                    if theme::secondary_button(ui, crate::i18n::t("recorder.btn.export"))
                                                        .clicked()
                                                    {
                                                        self.mode = AppMode::InputExport;
                                                        self.input.clear();
                                                        self.status = "输入导出文件名:".into();
                                                    }
                                                });
                                                ui.add_space(6.0);
                                                theme::toolbar_row(ui, |ui| {
                                                    theme::field_label(ui, crate::i18n::t("recorder.hide_wait"));
                                                    ui.add_space(8.0);
                                                    let mut ms = self.hide_wait_ms();
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(220.0, theme::CTRL_H),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if ui
                                                                .add(
                                                                    egui::Slider::new(
                                                                        &mut ms,
                                                                        500..=3000,
                                                                    )
                                                                    .step_by(100.0)
                                                                    .suffix(" ms"),
                                                                )
                                                                .changed()
                                                            {
                                                                self.set_hide_wait_ms(ms);
                                                            }
                                                        },
                                                    );
                                                });
                                                if self.last_id.is_some() {
                                                    ui.add_space(8.0);
                                                    theme::field_label(ui, "快捷状态");
                                                    ui.add_space(4.0);
                                                    theme::toolbar_row(ui, |ui| {
                                                        for (key, label, suffix) in [
                                                            (egui::Key::F1, "F1  常态", "-n"),
                                                            (egui::Key::F2, "F2  选中", "-s"),
                                                            (egui::Key::F3, "F3  点击", "-c"),
                                                            (egui::Key::F4, "F4  点+选", "-cs"),
                                                        ] {
                                                            let pressed =
                                                                ui.input(|i| i.key_pressed(key));
                                                            if theme::secondary_button(ui, label)
                                                                .clicked()
                                                                || pressed
                                                            {
                                                                self.forced_state_name =
                                                                    Some(suffix.to_string());
                                                                self.begin_capture(ctx, true);
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                            AppMode::Capturing => {
                                                ui.colored_label(
                                                    Color32::RED,
                                                    "Click and drag on screen to select region",
                                                );
                                                if theme::secondary_button(ui, "Cancel").clicked() {
                                                    self.end_capture(ctx);
                                                    self.mode = AppMode::Normal;
                                                    self.status = "Cancelled".into();
                                                }
                                            }
                                            _ => {
                                                let prompt = match &self.mode {
                                                    AppMode::InputName(..) => "Element name:",
                                                    AppMode::InputState(..) => {
                                                        "State name (default: -n):"
                                                    }
                                                    AppMode::InputExport => {
                                                        "Export filename (no extension):"
                                                    }
                                                    _ => "",
                                                };
                                                ui.label(prompt);
                                                let resp = ui.text_edit_singleline(&mut self.input);
                                                ui.horizontal(|ui| {
                                                    if theme::primary_button(ui, "OK").clicked()
                                                        || (resp.has_focus()
                                                            && ui.input_mut(|i| {
                                                                i.key_pressed(egui::Key::Enter)
                                                            }))
                                                    {
                                                        let val = self.input.trim().to_string();
                                                        let old = std::mem::replace(
                                                            &mut self.mode,
                                                            AppMode::Normal,
                                                        );
                                                        match old {
                                                            AppMode::InputName(
                                                                x1,
                                                                y1,
                                                                x2,
                                                                y2,
                                                                img,
                                                            ) if !val.is_empty() => {
                                                                self.mode = AppMode::InputState(
                                                                    x1,
                                                                    y1,
                                                                    x2,
                                                                    y2,
                                                                    img,
                                                                    val,
                                                                    "-n".to_string(),
                                                                );
                                                                self.input.clear();
                                                                self.status =
                                                            "Enter state name (default: -n):"
                                                                .into();
                                                            }
                                                            AppMode::InputState(
                                                                x1,
                                                                y1,
                                                                x2,
                                                                y2,
                                                                img,
                                                                name,
                                                                default_state,
                                                            ) => {
                                                                let state_name = if val.is_empty() {
                                                                    default_state
                                                                } else {
                                                                    val
                                                                };
                                                                let new_path =
                                                                    std::path::Path::new(
                                                                        &self.image_dir,
                                                                    )
                                                                    .join(format!(
                                                                        "{}_{}.png",
                                                                        name, state_name
                                                                    ))
                                                                    .to_string_lossy()
                                                                    .to_string();
                                                                let img_path = if std::fs::rename(
                                                                    &img, &new_path,
                                                                )
                                                                .is_ok()
                                                                {
                                                                    new_path
                                                                } else {
                                                                    img
                                                                };
                                                                match create_elem(
                                                                    &self.conn,
                                                                    &name,
                                                                    &state_name,
                                                                    x1,
                                                                    y1,
                                                                    x2,
                                                                    y2,
                                                                    &img_path,
                                                                    self.sw,
                                                                    self.sh,
                                                                ) {
                                                                    Ok(id) => {
                                                                        self.last_id = Some(id);
                                                                        self.last_element_name =
                                                                            Some(name.clone());
                                                                        self.last_state_name = Some(
                                                                            state_name.clone(),
                                                                        );
                                                                        self.status = format!(
                                                                            "Element #{} created!",
                                                                            id
                                                                        );
                                                                        self.refresh();
                                                                    }
                                                                    Err(e) => {
                                                                        self.status =
                                                                            format!("Error: {}", e)
                                                                    }
                                                                }
                                                                self.input.clear();
                                                            }
                                                            AppMode::InputExport
                                                                if !val.is_empty() =>
                                                            {
                                                                match export_csv(
                                                                    &self.conn,
                                                                    &val,
                                                                    &self.config,
                                                                ) {
                                                                    Ok(p) => {
                                                                        self.status = format!(
                                                                            "Exported: {}",
                                                                            p
                                                                        )
                                                                    }
                                                                    Err(e) => {
                                                                        self.status = format!(
                                                                            "Export error: {}",
                                                                            e
                                                                        )
                                                                    }
                                                                }
                                                                self.input.clear();
                                                            }
                                                            _ => {
                                                                self.status = "Cancelled".into();
                                                                self.input.clear();
                                                            }
                                                        }
                                                    }
                                                    if theme::secondary_button(ui, "Cancel").clicked() {
                                                        self.mode = AppMode::Normal;
                                                        self.input.clear();
                                                        self.status = "Cancelled".into();
                                                    }
                                                });
                                            }
                                        }
                                    }); // element content
                            }); // main column
                        }); // page columns
                    });
            }
        }
    }
}
