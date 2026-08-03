use crate::common::data_dir;
use crate::theme::{self, col};
use crate::workflow::{self, ClickFailAction, StepType, WorkflowStep};
use eframe::egui;
use image::{ImageBuffer, Rgba};
use rusqlite::{Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, VIRTUAL_KEY,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYVIRTUALSCREEN,
};

#[derive(Serialize, Deserialize)]
struct AppConfig {
    element_folder: String,
    workflow_path: String,
    #[serde(default = "default_threshold")]
    match_threshold: f32,
    #[serde(default)]
    pure_vision: bool,
    #[serde(default)]
    retries: u32,
    #[serde(default = "default_retry_ms")]
    retry_ms: u64,
    #[serde(default)]
    on_fail: String,
    #[serde(default)]
    save_match_debug: bool,
}

fn default_threshold() -> f32 {
    0.8
}

fn default_retry_ms() -> u64 {
    500
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            element_folder: String::new(),
            workflow_path: String::new(),
            match_threshold: 0.8,
            pure_vision: false,
            retries: 0,
            retry_ms: 500,
            on_fail: "skip".into(),
            save_match_debug: true,
        }
    }
}

fn get_config_path() -> String {
    data_dir()
        .join("clicker_config.json")
        .to_string_lossy()
        .to_string()
}

fn load_config() -> AppConfig {
    let path = get_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppConfig::default()
    }
}

fn save_config(config: &AppConfig) {
    let _ = fs::create_dir_all(data_dir());
    let path = get_config_path();
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = fs::write(&path, json);
    }
}

#[derive(Clone, Debug)]
struct UIElement {
    id: i64,
    #[allow(dead_code)]
    name: String,
    center_x: i32,
    center_y: i32,
    bbox_x: i32,
    bbox_y: i32,
    bbox_width: i32,
    bbox_height: i32,
    screen_width: i32,
    screen_height: i32,
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    primary_state: Option<String>,
}

#[derive(Clone, Debug)]
struct ElementState {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    element_id: i64,
    state_name: String,
    screenshot_path: String,
    is_primary: bool,
    #[allow(dead_code)]
    created_at: String,
}

struct ElementDatabase {
    db_path: String,
}

impl ElementDatabase {
    fn new(db_path: String) -> Self {
        Self { db_path }
    }

    fn load_element(
        &self,
        element_name: &str,
    ) -> SqlResult<Option<(UIElement, Vec<ElementState>)>> {
        let conn = Connection::open(&self.db_path)?;

        let (base_name, state_suffix) = if let Some(pos) = element_name.rfind("_-") {
            (&element_name[..pos], &element_name[pos + 1..])
        } else {
            (element_name, "")
        };

        let mut stmt = conn.prepare(
            "SELECT id, name, center_x, center_y, bbox_x, bbox_y, bbox_width, bbox_height,
                    screen_width, screen_height, created_at
             FROM elements WHERE name = ?1",
        )?;

        let element = stmt
            .query_row([base_name], |row| {
                Ok(UIElement {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    center_x: row.get(2)?,
                    center_y: row.get(3)?,
                    bbox_x: row.get(4)?,
                    bbox_y: row.get(5)?,
                    bbox_width: row.get(6)?,
                    bbox_height: row.get(7)?,
                    screen_width: row.get(8)?,
                    screen_height: row.get(9)?,
                    created_at: row.get(10)?,
                    primary_state: None,
                })
            })
            .ok();

        if let Some(ref elem) = element {
            let mut stmt = conn.prepare(
                "SELECT id, element_id, state_name, image_path, is_primary, created_at
                 FROM element_states WHERE element_id = ?1",
            )?;

            let mut states: Vec<ElementState> = stmt
                .query_map([elem.id], |row| {
                    Ok(ElementState {
                        id: row.get(0)?,
                        element_id: row.get(1)?,
                        state_name: row.get(2)?,
                        screenshot_path: row.get(3)?,
                        is_primary: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            if !state_suffix.is_empty() {
                if let Some(matched) = states.iter().find(|s| s.state_name == state_suffix) {
                    let mut m = matched.clone();
                    m.is_primary = true;
                    states = vec![m];
                }
            }

            Ok(Some((elem.clone(), states)))
        } else {
            Ok(None)
        }
    }
}

#[derive(Clone)]
struct ClickPoint {
    id: u32,
    x: i32,
    y: i32,
    description: String,
    template_path: Option<String>,
    original_width: Option<u32>,
    original_height: Option<u32>,
    #[allow(dead_code)]
    action_name: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum AppState {
    Idle,
    Running,
    Done,
    Paused,
}

#[derive(Clone, PartialEq)]
enum AppMode {
    CsvMode,
    WorkflowMode,
}

#[derive(Clone, Debug)]
enum DialogKind {
    Pause {
        message: String,
    },
    Manual {
        message: String,
        instruction: Option<String>,
    },
}

pub struct ClickerApp {
    mode: AppMode,
    csv_path: String,
    points: Vec<ClickPoint>,
    delay_ms: String,
    state: Arc<Mutex<AppState>>,
    current_index: Arc<Mutex<usize>>,
    log_messages: Arc<Mutex<Vec<String>>>,

    workflow_path: String,
    element_folder: String,
    workflow_steps: Vec<WorkflowStep>,
    /// Global template match threshold (0.0–1.0).
    match_threshold: f32,
    /// When true, only click if template matches; never fall back to DB coords.
    pure_vision: bool,
    retries: u32,
    retry_ms: u64,
    on_fail: ClickFailAction,
    save_match_debug: bool,

    dialog_pending: Arc<Mutex<Option<DialogKind>>>,
    dialog_confirm: Arc<(Mutex<bool>, Condvar)>,

    window_visible: Arc<Mutex<bool>>,
    dialog_brought_to_front: bool,
    /// Live loop progress `(current, total)` for HUD; `None` when not inside a loop.
    loop_progress: Arc<Mutex<Option<(u32, u32)>>>,
}

impl ClickerApp {
    pub fn new(default_element_folder: String) -> Self {
        let config = load_config();
        let element_folder = if !config.element_folder.is_empty() {
            config.element_folder
        } else {
            default_element_folder
        };
        let on_fail = ClickFailAction::parse(&config.on_fail).unwrap_or(ClickFailAction::Skip);
        Self {
            mode: AppMode::CsvMode,
            csv_path: String::new(),
            points: Vec::new(),
            delay_ms: "1000".to_string(),
            state: Arc::new(Mutex::new(AppState::Idle)),
            current_index: Arc::new(Mutex::new(0)),
            log_messages: Arc::new(Mutex::new(Vec::new())),

            workflow_path: config.workflow_path,
            element_folder,
            workflow_steps: Vec::new(),
            match_threshold: config.match_threshold.clamp(0.1, 1.0),
            pure_vision: config.pure_vision,
            retries: config.retries,
            retry_ms: config.retry_ms,
            on_fail,
            save_match_debug: config.save_match_debug,

            dialog_pending: Arc::new(Mutex::new(None)),
            dialog_confirm: Arc::new((Mutex::new(false), Condvar::new())),
            loop_progress: Arc::new(Mutex::new(None)),

            window_visible: Arc::new(Mutex::new(true)),
            dialog_brought_to_front: false,
        }
    }

    fn persist_config(&self) {
        save_config(&AppConfig {
            element_folder: self.element_folder.clone(),
            workflow_path: self.workflow_path.clone(),
            match_threshold: self.match_threshold,
            pure_vision: self.pure_vision,
            retries: self.retries,
            retry_ms: self.retry_ms,
            on_fail: self.on_fail.as_str().to_string(),
            save_match_debug: self.save_match_debug,
        });
    }

    /// Agent entrypoint: set step/click delay in milliseconds.
    pub fn set_delay_ms(&mut self, delay_ms: u64) {
        self.delay_ms = delay_ms.to_string();
    }

    pub fn set_match_threshold(&mut self, threshold: f32) {
        self.match_threshold = threshold.clamp(0.1, 1.0);
        self.persist_config();
    }

    pub fn set_pure_vision(&mut self, enabled: bool) {
        self.pure_vision = enabled;
        self.persist_config();
    }

    pub fn set_retries(&mut self, retries: u32) {
        self.retries = retries.min(20);
        self.persist_config();
    }

    pub fn set_retry_ms(&mut self, retry_ms: u64) {
        self.retry_ms = retry_ms.min(60_000);
        self.persist_config();
    }

    pub fn set_on_fail(&mut self, action: ClickFailAction) {
        self.on_fail = action;
        self.persist_config();
    }

    pub fn set_save_match_debug(&mut self, enabled: bool) {
        self.save_match_debug = enabled;
        self.persist_config();
    }

    pub fn match_threshold(&self) -> f32 {
        self.match_threshold
    }

    pub fn pure_vision(&self) -> bool {
        self.pure_vision
    }

    pub fn retries(&self) -> u32 {
        self.retries
    }

    pub fn retry_ms(&self) -> u64 {
        self.retry_ms
    }

    pub fn on_fail(&self) -> ClickFailAction {
        self.on_fail.clone()
    }

    pub fn save_match_debug(&self) -> bool {
        self.save_match_debug
    }

    /// Agent entrypoint: set element folder (contains db/images).
    pub fn set_element_folder(&mut self, folder: String) {
        self.element_folder = folder;
        self.persist_config();
    }

    /// Agent entrypoint: get current runtime status string.
    pub fn status_text(&self) -> &'static str {
        match self.state.lock().unwrap().clone() {
            AppState::Idle => "idle",
            AppState::Running => "running",
            AppState::Done => "done",
            AppState::Paused => "paused",
        }
    }

    /// Agent entrypoint: snapshot log messages.
    pub fn logs_snapshot(&self) -> Vec<String> {
        self.log_messages.lock().unwrap().clone()
    }

    /// Agent entrypoint: stop current execution.
    pub fn stop(&mut self, ctx: &egui::Context) {
        *self.state.lock().unwrap() = AppState::Idle;
        *self.window_visible.lock().unwrap() = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    }

    /// Agent entrypoint: load workflow file and keep parsed steps.
    pub fn agent_load_workflow_file(&mut self, workflow_path: &str) -> Result<usize, String> {
        self.workflow_path = workflow_path.to_string();
        match workflow::parse_workflow_file(workflow_path) {
            Ok(steps) => {
                let count = steps.len();
                self.workflow_steps = steps;
                Ok(count)
            }
            Err(e) => Err(e),
        }
    }

    /// Agent entrypoint: replace workflow steps from external planner/editor.
    pub fn agent_set_workflow_steps(&mut self, steps: Vec<WorkflowStep>) {
        self.mode = AppMode::WorkflowMode;
        self.workflow_steps = steps;
    }

    /// Agent entrypoint: start currently loaded workflow steps.
    pub fn agent_start_workflow(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.workflow_steps.is_empty() {
            return Err("workflow is empty".into());
        }
        if self.element_folder.trim().is_empty() {
            return Err("element_folder is empty".into());
        }
        self.mode = AppMode::WorkflowMode;
        self.start_workflow(ctx);
        Ok(())
    }

    /// Whether a workflow or CSV click run is active.
    pub fn is_busy(&self) -> bool {
        matches!(
            *self.state.lock().unwrap(),
            AppState::Running | AppState::Paused
        )
    }

    /// Floating always-on-top status bar while workflow/CSV is running (main window hidden).
    pub fn should_show_run_hud(&self) -> bool {
        let state = self.state.lock().unwrap().clone();
        let dialog = self.dialog_pending.lock().unwrap().is_some();
        dialog || matches!(state, AppState::Running | AppState::Paused)
    }

    /// Paint a slim top-of-screen HUD with current step + stop / continue.
    pub fn paint_run_hud(&mut self, ctx: &egui::Context) {
        if !self.should_show_run_hud() {
            return;
        }

        let state = self.state.lock().unwrap().clone();
        let idx = *self.current_index.lock().unwrap();
        let dialog = self.dialog_pending.lock().unwrap().clone();
        let loop_prog = *self.loop_progress.lock().unwrap();

        let (total, step_label) = if self.mode == AppMode::WorkflowMode {
            let total = self.workflow_steps.len().max(1);
            let label = self
                .workflow_steps
                .get(idx)
                .map(|s| step_type_hud_label(&s.step_type))
                .unwrap_or_else(|| "…".into());
            (total, label)
        } else {
            let total = self.points.len().max(1);
            let label = self
                .points
                .get(idx)
                .map(|p| {
                    if p.description.is_empty() {
                        format!("坐标  ({}, {})", p.x, p.y)
                    } else {
                        format!("{}  ({}, {})", p.description, p.x, p.y)
                    }
                })
                .unwrap_or_else(|| "…".into());
            (total, label)
        };

        let step_num = (idx + 1).min(total);
        let progress = step_num as f32 / total as f32;
        let time = ctx.input(|i| i.time);
        let pulse = ((time * 3.0).sin() * 0.5 + 0.5) as f32;

        let screen = ctx.input(|i| {
            i.viewport()
                .monitor_size
                .unwrap_or(egui::vec2(1280.0, 800.0))
        });
        let bar_w = (screen.x * 0.52).clamp(520.0, 860.0);
        let bar_h = if dialog.is_some() { 72.0 } else { 48.0 };
        let pos_x = ((screen.x - bar_w) * 0.5).max(8.0);
        let pos_y = 10.0;

        let mut stop = false;
        let mut cont = false;

        let builder = egui::ViewportBuilder::default()
            .with_title("Mouse Suite · 运行中")
            .with_decorations(false)
            .with_taskbar(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_inner_size([bar_w, bar_h])
            .with_position([pos_x, pos_y]);

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("mouse_suite_run_hud"),
            builder,
            |ctx, _class| {
                ctx.request_repaint();
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                    .show(ctx, |ui| {
                        let full = ui.max_rect();
                        let painter = ui.painter();

                        // Soft shadow + frosted dark card
                        painter.rect_filled(
                            full.shrink(1.0).translate(egui::vec2(0.0, 3.0)),
                            14.0,
                            egui::Color32::from_black_alpha(40),
                        );
                        painter.rect_filled(full.shrink(1.0), 14.0, col().HUD_BG);
                        painter.rect_stroke(
                            full.shrink(1.0),
                            14.0,
                            egui::Stroke::new(1.0, col().HUD_EDGE),
                        );
                        // Accent top rail
                        let rail = egui::Rect::from_min_max(
                            egui::pos2(full.left() + 14.0, full.top() + 4.0),
                            egui::pos2(full.right() - 14.0, full.top() + 6.0),
                        );
                        painter.rect_filled(rail, 1.0, col().ACCENT);

                        // Progress strip
                        let strip = egui::Rect::from_min_max(
                            egui::pos2(full.left() + 14.0, full.bottom() - 7.0),
                            egui::pos2(full.right() - 14.0, full.bottom() - 4.0),
                        );
                        painter.rect_filled(strip, 2.0, col().HUD_EDGE);
                        let mut filled = strip;
                        filled.set_width((strip.width() * progress).max(4.0));
                        painter.rect_filled(filled, 2.0, col().ACCENT_HOT);

                        let inner = egui::Rect::from_min_max(
                            egui::pos2(full.left() + 14.0, full.top() + 8.0),
                            egui::pos2(full.right() - 14.0, full.bottom() - 10.0),
                        );
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                            ui.horizontal(|ui| {
                                // Pulse dot — teal when running, warn when paused
                                let base = col().ACCENT;
                                let [r, g, b, _] = base.to_array();
                                let dot_c = match state {
                                    AppState::Paused => col().WARN,
                                    _ => egui::Color32::from_rgb(
                                        (r as f32 + 30.0 * pulse).min(255.0) as u8,
                                        (g as f32 + 20.0 * pulse).min(255.0) as u8,
                                        (b as f32 + 10.0 * pulse).min(255.0) as u8,
                                    ),
                                };
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(10.0, 10.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(dot_rect.center(), 4.5, dot_c);

                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new("Mouse Suite")
                                        .size(12.0)
                                        .strong()
                                        .color(col().HUD_TEXT),
                                );

                                ui.add_space(10.0);
                                let phase = match (&dialog, state) {
                                    (Some(DialogKind::Pause { .. }), _) => "暂停",
                                    (Some(DialogKind::Manual { .. }), _) => "人工",
                                    (_, AppState::Paused) => "暂停",
                                    _ => "执行中",
                                };
                                ui.label(
                                    egui::RichText::new(phase)
                                        .size(11.0)
                                        .color(col().HUD_MUTED),
                                );

                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("·")
                                        .size(11.0)
                                        .color(col().HUD_MUTED),
                                );
                                ui.add_space(8.0);

                                ui.label(
                                    egui::RichText::new(format!("{}/{}", step_num, total))
                                        .size(13.0)
                                        .strong()
                                        .color(col().ACCENT),
                                );
                                if let Some((cur, tot)) = loop_prog {
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format!("循环{}/{}", cur, tot))
                                            .size(12.0)
                                            .strong()
                                            .color(col().WARN),
                                    );
                                }
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(&step_label)
                                        .size(13.0)
                                        .color(col().HUD_TEXT),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let stop_btn = egui::Button::new(
                                            egui::RichText::new(crate::i18n::t("clicker.btn.stop"))
                                                .size(12.0)
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(col().DANGER)
                                        .rounding(6.0);
                                        if ui.add(stop_btn).clicked() {
                                            stop = true;
                                        }
                                        if dialog.is_some() {
                                            ui.add_space(6.0);
                                            let go = egui::Button::new(
                                                egui::RichText::new("继续")
                                                    .size(12.0)
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(col().ACCENT)
                                            .rounding(6.0);
                                            if ui.add(go).clicked() {
                                                cont = true;
                                            }
                                        }
                                    },
                                );
                            });

                            if let Some(ref kind) = dialog {
                                ui.add_space(2.0);
                                let msg = match kind {
                                    DialogKind::Pause { message } => message.as_str(),
                                    DialogKind::Manual { message, .. } => message.as_str(),
                                };
                                ui.label(
                                    egui::RichText::new(msg)
                                        .size(11.0)
                                        .color(col().HUD_MUTED),
                                );
                            }
                        });
                    });
            },
        );

        if stop {
            self.dialog_brought_to_front = false;
            *self.state.lock().unwrap() = AppState::Idle;
            *self.dialog_pending.lock().unwrap() = None;
            *self.window_visible.lock().unwrap() = true;
            let (lock, cvar) = &*self.dialog_confirm;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(false),
            );
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal),
            );
        } else if cont {
            self.dialog_brought_to_front = false;
            *self.dialog_pending.lock().unwrap() = None;
            *self.window_visible.lock().unwrap() = false;
            let (lock, cvar) = &*self.dialog_confirm;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal),
            );
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Minimized(true),
            );
        }
    }

    /// Flow node id for the step currently under the PC (canvas highlight).
    pub fn current_workflow_node(&self) -> Option<u32> {
        if !matches!(
            *self.state.lock().unwrap(),
            AppState::Running | AppState::Paused
        ) {
            return None;
        }
        let idx = *self.current_index.lock().unwrap();
        self.workflow_steps.get(idx).and_then(|s| s.source_node)
    }

    /// Agent entrypoint: load CSV points file.
    pub fn agent_load_csv_file(&mut self, path: &str) -> Result<usize, String> {
        self.csv_path = path.to_string();
        self.points = parse_csv(path).map_err(|e| e.to_string())?;
        Ok(self.points.len())
    }

    /// Agent entrypoint: run loaded CSV click points.
    pub fn agent_start_csv(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("csv points are empty".into());
        }
        self.mode = AppMode::CsvMode;
        self.start_clicking(ctx);
        Ok(())
    }

    fn load_csv(&mut self, path: &str) {
        self.csv_path = path.to_string();
        self.points = parse_csv(path).unwrap_or_default();
    }

    fn load_workflow(&mut self, workflow_path: &str) {
        self.workflow_path = workflow_path.to_string();
        match workflow::parse_workflow_file(workflow_path) {
            Ok(steps) => {
                self.workflow_steps = steps;
                self.log_messages.lock().unwrap().push(format!(
                    "Loaded {} workflow steps",
                    self.workflow_steps.len()
                ));
            }
            Err(e) => {
                self.log_messages
                    .lock()
                    .unwrap()
                    .push(format!("Failed to load workflow: {}", e));
            }
        }
    }

    /// Run steps produced by the visual flow editor.
    pub fn run_workflow_steps(&mut self, ctx: &egui::Context, steps: Vec<WorkflowStep>) {
        self.mode = AppMode::WorkflowMode;
        self.workflow_steps = steps;
        self.start_workflow(ctx);
    }

    fn start_workflow(&mut self, ctx: &egui::Context) {
        let delay: u64 = self.delay_ms.trim().parse().unwrap_or(1000);
        let steps = self.workflow_steps.clone();
        let element_folder = self.element_folder.clone();
        let global_threshold = self.match_threshold;
        let global_pure_vision = self.pure_vision;
        let global_retries = self.retries;
        let global_retry_ms = self.retry_ms;
        let global_on_fail = self.on_fail.clone();
        let save_match_debug = self.save_match_debug;
        let state = self.state.clone();
        let current_index = self.current_index.clone();
        let loop_progress = self.loop_progress.clone();
        let log = self.log_messages.clone();
        let dialog_pending = self.dialog_pending.clone();
        let dialog_confirm = self.dialog_confirm.clone();
        let window_visible = self.window_visible.clone();
        let ctx_clone = ctx.clone();

        *state.lock().unwrap() = AppState::Running;
        *current_index.lock().unwrap() = 0;
        *loop_progress.lock().unwrap() = None;
        log.lock().unwrap().clear();

        thread::spawn(move || {
            *window_visible.lock().unwrap() = false;
            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(true));

            thread::sleep(Duration::from_secs(3));

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let log_file = format!("{}\\workflow_{}.log", element_folder, now);

            log_write(&log, &log_file, "=== Workflow started ===".to_string());
            log_write(
                &log,
                &log_file,
                format!(
                    "DB: {}\\mouse_recorder.db | threshold={:.2} | pure_vision={} | retries={} | on_fail={} | debug={}",
                    element_folder,
                    global_threshold,
                    global_pure_vision,
                    global_retries,
                    global_on_fail.as_str(),
                    save_match_debug
                ),
            );

            let db_path = format!("{}\\mouse_recorder.db", element_folder);
            let db = ElementDatabase::new(db_path);
            // Shared across IfVision → Click: reuse coords + grow ROI from last hits.
            let vision = Arc::new(Mutex::new(VisionSession::default()));

            let mut pc: usize = 0;
            enum LoopFrame {
                Count {
                    body_start: usize,
                    remaining: u32,
                    total: u32,
                },
                While {
                    head_pc: usize,
                    max_times: u32,
                },
            }
            let mut loop_stack: Vec<LoopFrame> = Vec::new();
            // While-loop iteration counts keyed by LoopWhileStart PC
            let mut while_iters: std::collections::HashMap<usize, u32> =
                std::collections::HashMap::new();
            let sync_loop_hud =
                |stack: &[LoopFrame],
                 while_iters: &std::collections::HashMap<usize, u32>,
                 loop_progress: &Arc<Mutex<Option<(u32, u32)>>>| {
                    let prog = match stack.last() {
                        Some(LoopFrame::Count {
                            remaining, total, ..
                        }) => {
                            let current = total.saturating_sub(*remaining).saturating_add(1);
                            Some((current.min(*total), *total))
                        }
                        Some(LoopFrame::While { head_pc, max_times }) => {
                            let cur = while_iters.get(head_pc).copied().unwrap_or(0).max(1);
                            Some((cur.min(*max_times), *max_times))
                        }
                        None => None,
                    };
                    *loop_progress.lock().unwrap() = prog;
                };

            while pc < steps.len() {
                {
                    let s = state.lock().unwrap();
                    if *s != AppState::Running && *s != AppState::Paused {
                        return;
                    }
                }
                *current_index.lock().unwrap() = pc;
                let i = pc;
                match &steps[pc].step_type {
                    StepType::Goto { jump } => {
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Goto → {}", i + 1, jump + 1),
                        );
                        pc = *jump;
                    }
                    StepType::IfVision {
                        element_name,
                        or_elements,
                        threshold,
                        retries,
                        retry_ms,
                        then_jump,
                        else_jump,
                    } => {
                        let th = threshold.unwrap_or(global_threshold).clamp(0.1, 1.0);
                        let retries = retries.unwrap_or(global_retries);
                        let retry_ms = retry_ms.unwrap_or(global_retry_ms);
                        let names = workflow::merge_or_names(element_name, or_elements);
                        let hit = probe_any_element(
                            &names,
                            &db,
                            &element_folder,
                            &log,
                            &log_file,
                            i,
                            th,
                            retries,
                            retry_ms,
                            &vision,
                        );
                        let label = names.join(" or ");
                        if let Some(found) = hit {
                            {
                                let mut vs = vision.lock().unwrap();
                                vs.remember(&found.name, found.x, found.y);
                                vs.pending = Some(found.clone());
                            }
                            log_write(
                                &log,
                                &log_file,
                                format!(
                                    "Step {}: IfVision '{}' → TRUE @({},{}) (jump {})",
                                    i + 1,
                                    label,
                                    found.x,
                                    found.y,
                                    then_jump + 1
                                ),
                            );
                            pc = *then_jump;
                        } else {
                            vision.lock().unwrap().pending = None;
                            log_write(
                                &log,
                                &log_file,
                                format!(
                                    "Step {}: IfVision '{}' → FALSE (jump {})",
                                    i + 1,
                                    label,
                                    else_jump + 1
                                ),
                            );
                            pc = *else_jump;
                        }
                    }
                    StepType::LoopStart { times } => {
                        let t = (*times).max(1);
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Loop start ×{}", i + 1, t),
                        );
                        loop_stack.push(LoopFrame::Count {
                            body_start: pc + 1,
                            remaining: t,
                            total: t,
                        });
                        sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                        pc += 1;
                    }
                    StepType::LoopWhileStart {
                        element_name,
                        or_elements,
                        threshold,
                        retries,
                        retry_ms,
                        max_times,
                    } => {
                        let th = threshold.unwrap_or(global_threshold).clamp(0.1, 1.0);
                        let retries = retries.unwrap_or(global_retries);
                        let retry_ms = retry_ms.unwrap_or(global_retry_ms);
                        let max_times = (*max_times).max(1);
                        let names = workflow::merge_or_names(element_name, or_elements);
                        let label = names.join(" or ");
                        let count = while_iters.entry(pc).or_insert(0);
                        if *count >= max_times {
                            log_write(
                                &log,
                                &log_file,
                                format!(
                                    "Step {}: LoopWhile '{}' hit max_times={} — exit",
                                    i + 1,
                                    label,
                                    max_times
                                ),
                            );
                            while_iters.remove(&pc);
                            loop_stack.retain(|f| {
                                !matches!(f, LoopFrame::While { head_pc, .. } if *head_pc == pc)
                            });
                            sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                            pc = skip_after_matching_loop_end(&steps, pc);
                        } else {
                            let hit = probe_any_element(
                                &names,
                                &db,
                                &element_folder,
                                &log,
                                &log_file,
                                i,
                                th,
                                retries,
                                retry_ms,
                                &vision,
                            );
                            if let Some(found) = hit {
                                {
                                    let mut vs = vision.lock().unwrap();
                                    vs.remember(&found.name, found.x, found.y);
                                }
                                *count += 1;
                                log_write(
                                    &log,
                                    &log_file,
                                    format!(
                                        "Step {}: LoopWhile '{}' match @({},{}) — enter body ({}/{})",
                                        i + 1,
                                        label,
                                        found.x,
                                        found.y,
                                        *count,
                                        max_times
                                    ),
                                );
                                loop_stack.push(LoopFrame::While {
                                    head_pc: pc,
                                    max_times,
                                });
                                sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                                pc += 1;
                            } else {
                                log_write(
                                    &log,
                                    &log_file,
                                    format!(
                                        "Step {}: LoopWhile '{}' miss — exit loop",
                                        i + 1,
                                        label
                                    ),
                                );
                                while_iters.remove(&pc);
                                sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                                pc = skip_after_matching_loop_end(&steps, pc);
                            }
                        }
                    }
                    StepType::LoopEnd => match loop_stack.pop() {
                        Some(LoopFrame::Count {
                            body_start,
                            remaining,
                            total,
                        }) => {
                            if remaining > 1 {
                                loop_stack.push(LoopFrame::Count {
                                    body_start,
                                    remaining: remaining - 1,
                                    total,
                                });
                                sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                                log_write(
                                    &log,
                                    &log_file,
                                    format!(
                                        "Step {}: Loop end → repeat ({}/{}, {} left)",
                                        i + 1,
                                        total - remaining + 2,
                                        total,
                                        remaining - 1
                                    ),
                                );
                                pc = body_start;
                            } else {
                                sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                                log_write(
                                    &log,
                                    &log_file,
                                    format!("Step {}: Loop finished ({}/{})", i + 1, total, total),
                                );
                                pc += 1;
                            }
                        }
                        Some(LoopFrame::While { head_pc, .. }) => {
                            sync_loop_hud(&loop_stack, &while_iters, &loop_progress);
                            log_write(
                                &log,
                                &log_file,
                                format!(
                                    "Step {}: Loop end → recheck while at {}",
                                    i + 1,
                                    head_pc + 1
                                ),
                            );
                            pc = head_pc;
                        }
                        None => {
                            log_write(
                                &log,
                                &log_file,
                                format!("Step {}: Loop end without start (ignored)", i + 1),
                            );
                            pc += 1;
                        }
                    },
                    StepType::Click {
                        element_name,
                        or_elements,
                        threshold,
                        pure_vision,
                        retries,
                        retry_ms,
                        on_fail,
                    } => {
                        let th = threshold.unwrap_or(global_threshold).clamp(0.1, 1.0);
                        let pv = pure_vision.unwrap_or(global_pure_vision);
                        let retries = retries.unwrap_or(global_retries);
                        let retry_ms = retry_ms.unwrap_or(global_retry_ms);
                        let on_fail = on_fail.clone().unwrap_or(global_on_fail.clone());
                        let names = workflow::merge_or_names(element_name, or_elements);
                        let outcome = execute_click_with_retries(
                            &names,
                            &db,
                            &element_folder,
                            &log,
                            &log_file,
                            i,
                            delay,
                            th,
                            pv,
                            retries,
                            retry_ms,
                            on_fail,
                            save_match_debug,
                            &vision,
                            &state,
                        );
                        if matches!(outcome, ClickOutcome::Abort) {
                            log_write(
                                &log,
                                &log_file,
                                format!("Step {}: aborted by on_fail policy", i + 1),
                            );
                            *state.lock().unwrap() = AppState::Idle;
                            *loop_progress.lock().unwrap() = None;
                            *window_visible.lock().unwrap() = true;
                            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            return;
                        }
                        pc += 1;
                    }

                    StepType::Wait { seconds } => {
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Wait {}s", i + 1, seconds),
                        );
                        thread::sleep(Duration::from_secs(*seconds as u64));
                        pc += 1;
                    }

                    StepType::TypeText { text, interval_ms } => {
                        let ms = interval_ms.unwrap_or(30).min(2000);
                        let preview: String = text.chars().take(40).collect();
                        log_write(
                            &log,
                            &log_file,
                            format!(
                                "Step {}: Type \"{}\"{}",
                                i + 1,
                                preview,
                                if text.chars().count() > 40 { "…" } else { "" }
                            ),
                        );
                        type_text_sequence(text, ms);
                        pc += 1;
                    }

                    StepType::Pause { message, .. } => {
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Paused — {}", i + 1, message),
                        );
                        *state.lock().unwrap() = AppState::Paused;
                        *dialog_pending.lock().unwrap() = Some(DialogKind::Pause {
                            message: message.clone(),
                        });
                        // Stay minimized — confirm via top HUD

                        let (lock, cvar) = &*dialog_confirm;
                        let mut confirmed = lock.lock().unwrap();
                        *confirmed = false;
                        confirmed = cvar.wait_while(confirmed, |c| !*c).unwrap();
                        drop(confirmed);

                        *dialog_pending.lock().unwrap() = None;
                        if *state.lock().unwrap() != AppState::Paused {
                            log_write(&log, &log_file, format!("Step {}: Stopped by user", i + 1));
                            *loop_progress.lock().unwrap() = None;
                            *window_visible.lock().unwrap() = true;
                            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            return;
                        }
                        *state.lock().unwrap() = AppState::Running;
                        log_write(&log, &log_file, format!("Step {}: Resumed", i + 1));
                        pc += 1;
                    }

                    StepType::Manual {
                        message,
                        instruction,
                        ..
                    } => {
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Manual — {}", i + 1, message),
                        );
                        *state.lock().unwrap() = AppState::Paused;
                        *dialog_pending.lock().unwrap() = Some(DialogKind::Manual {
                            message: message.clone(),
                            instruction: instruction.clone(),
                        });
                        // Stay minimized — confirm via top HUD

                        let (lock, cvar) = &*dialog_confirm;
                        let mut confirmed = lock.lock().unwrap();
                        *confirmed = false;
                        confirmed = cvar.wait_while(confirmed, |c| !*c).unwrap();
                        drop(confirmed);

                        *dialog_pending.lock().unwrap() = None;
                        if *state.lock().unwrap() != AppState::Paused {
                            log_write(&log, &log_file, format!("Step {}: Stopped by user", i + 1));
                            *loop_progress.lock().unwrap() = None;
                            *window_visible.lock().unwrap() = true;
                            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            return;
                        }
                        *state.lock().unwrap() = AppState::Running;
                        log_write(&log, &log_file, format!("Step {}: Manual done", i + 1));
                        pc += 1;
                    }
                }
            }

            *state.lock().unwrap() = AppState::Done;
            *loop_progress.lock().unwrap() = None;
            *window_visible.lock().unwrap() = true;
            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            log_write(&log, &log_file, "=== Workflow completed ===".to_string());
        });
    }

    fn start_clicking(&mut self, ctx: &egui::Context) {
        let delay: u64 = self.delay_ms.trim().parse().unwrap_or(1000);
        let points = self.points.clone();
        let state = self.state.clone();
        let current_index = self.current_index.clone();
        let log = self.log_messages.clone();
        let window_visible = self.window_visible.clone();
        let ctx_clone = ctx.clone();
        let threshold = self.match_threshold;
        let pure_vision = self.pure_vision;

        *state.lock().unwrap() = AppState::Running;
        *current_index.lock().unwrap() = 0;
        log.lock().unwrap().clear();

        thread::spawn(move || {
            *window_visible.lock().unwrap() = false;
            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(true));

            thread::sleep(Duration::from_secs(3));
            log.lock().unwrap().push("Starting...".to_string());

            for (i, p) in points.iter().enumerate() {
                {
                    let s = state.lock().unwrap();
                    if *s != AppState::Running {
                        return;
                    }
                }
                *current_index.lock().unwrap() = i;

                let (current_w, current_h) = screen_size();

                let (scaled_x, scaled_y) =
                    if let (Some(orig_w), Some(orig_h)) = (p.original_width, p.original_height) {
                        let scale_x = current_w as f64 / orig_w as f64;
                        let scale_y = current_h as f64 / orig_h as f64;
                        ((p.x as f64 * scale_x) as i32, (p.y as f64 * scale_y) as i32)
                    } else {
                        (p.x, p.y)
                    };

                let click_xy = if let Some(ref template_path) = p.template_path {
                    let result = find_template(template_path, threshold);
                    match result.hit {
                        Some((x, y)) => {
                            log.lock().unwrap().push(format!(
                                "#{:03} [Template Match] ({}, {}) score={:.3} thr={:.2} - {}",
                                p.id, x, y, result.best_score, threshold, p.description
                            ));
                            Some((x, y))
                        }
                        None => {
                            if pure_vision {
                                log.lock().unwrap().push(format!(
                                    "#{:03} pure-vision miss best={:.3} — skip {}",
                                    p.id, result.best_score, p.description
                                ));
                                None
                            } else {
                                log.lock().unwrap().push(format!(
                                    "#{:03} [Fallback Scaled] ({}, {}) best={:.3} - {}",
                                    p.id, scaled_x, scaled_y, result.best_score, p.description
                                ));
                                Some((scaled_x, scaled_y))
                            }
                        }
                    }
                } else if pure_vision {
                    log.lock().unwrap().push(format!(
                        "#{:03} pure-vision — no template, skip {}",
                        p.id, p.description
                    ));
                    None
                } else {
                    let res_info = if let (Some(orig_w), Some(orig_h)) =
                        (p.original_width, p.original_height)
                    {
                        format!(" [scaled from {}x{}]", orig_w, orig_h)
                    } else {
                        String::new()
                    };
                    log.lock().unwrap().push(format!(
                        "#{:03} [Coords] ({}, {}){}  - {}",
                        p.id, scaled_x, scaled_y, res_info, p.description
                    ));
                    Some((scaled_x, scaled_y))
                };

                if let Some((click_x, click_y)) = click_xy {
                    click_at(click_x, click_y);
                    thread::sleep(Duration::from_millis(delay));
                }
            }

            *state.lock().unwrap() = AppState::Done;
            *window_visible.lock().unwrap() = true;
            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            log.lock()
                .unwrap()
                .push("All clicks completed!".to_string());
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(100));

        let dialog = self.dialog_pending.lock().unwrap().clone();
        if let Some(ref kind) = dialog {
            if !self.dialog_brought_to_front {
                self.dialog_brought_to_front = true;
                *self.window_visible.lock().unwrap() = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::AlwaysOnTop,
                ));
            }

            theme::themed_window("等待确认")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_min_width(320.0);
                    match kind {
                        DialogKind::Pause { message } => {
                            theme::modal_title(ui, "流程已暂停");
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(message.as_str()).color(theme::col().TEXT));
                        }
                        DialogKind::Manual {
                            message,
                            instruction,
                        } => {
                            theme::modal_title(ui, "需要人工操作");
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(message.as_str()).color(theme::col().TEXT));
                            if let Some(inst) = instruction {
                                ui.add_space(4.0);
                                theme::hairline(ui);
                                ui.label(
                                    egui::RichText::new(format!("操作说明: {}", inst))
                                        .color(theme::col().MUTED),
                                );
                            }
                        }
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if theme::danger_button(ui, "停止执行").clicked() {
                            self.dialog_brought_to_front = false;
                            *self.state.lock().unwrap() = AppState::Idle;
                            *self.dialog_pending.lock().unwrap() = None;
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::Normal,
                            ));
                            let (lock, cvar) = &*self.dialog_confirm;
                            *lock.lock().unwrap() = true;
                            cvar.notify_all();
                        }
                        ui.add_space(20.0);
                        let btn_label = match kind {
                            DialogKind::Pause { .. } => "继续执行",
                            DialogKind::Manual { .. } => "已完成，继续",
                        };
                        if theme::primary_button(ui, btn_label).clicked() {
                            self.dialog_brought_to_front = false;
                            *self.dialog_pending.lock().unwrap() = None;
                            *self.window_visible.lock().unwrap() = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::Normal,
                            ));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            let (lock, cvar) = &*self.dialog_confirm;
                            *lock.lock().unwrap() = true;
                            cvar.notify_all();
                        }
                    });
                });
            return;
        } else {
            self.dialog_brought_to_front = false;
        }

        let window_visible = *self.window_visible.lock().unwrap();
        if !window_visible {
            return;
        }

        // Pin run log to the bottom so it never gets clipped by the form above.
        self.paint_run_log_panel(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(col().BG))
            .show(ctx, |ui| {
                theme::paint_atmosphere(ui);
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(20.0, 18.0))
                    .show(ui, |ui| {
                        theme::section_header(
                            ui,
                            crate::i18n::t("clicker.header.title"),
                            crate::i18n::t("clicker.header.subtitle"),
                        );

                        theme::toolbar_row(ui, |ui| {
                            theme::field_label(ui, crate::i18n::t("clicker.mode"));
                            ui.add_space(8.0);
                            let labels = [
                                "CSV",
                                crate::i18n::t("clicker.mode.workflow"),
                            ];
                            let selected = if self.mode == AppMode::CsvMode { 0 } else { 1 };
                            if let Some(i) = theme::segmented_control(ui, &labels, selected) {
                                self.mode = if i == 0 {
                                    AppMode::CsvMode
                                } else {
                                    AppMode::WorkflowMode
                                };
                            }
                        });

                        ui.add_space(10.0);
                        theme::hairline(ui);

                        // Form content scrolls independently of the log panel.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .id_salt("clicker_form_scroll")
                            .show(ui, |ui| {
                                if self.mode == AppMode::CsvMode {
                                    self.render_csv_mode(ui, ctx);
                                } else {
                                    self.render_workflow_mode(ui, ctx);
                                }
                            });
                    });
            });
    }

    fn paint_run_log_panel(&mut self, ctx: &egui::Context) {
        let logs = {
            let guard = self.log_messages.lock().unwrap();
            guard.clone()
        };

        egui::TopBottomPanel::bottom("clicker_run_log")
            .resizable(true)
            .default_height(168.0)
            .min_height(88.0)
            .max_height(420.0)
            .frame(
                egui::Frame::none()
                    .fill(col().CHROME)
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, col().PANEL_EDGE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::t("clicker.log"))
                            .size(12.0)
                            .strong()
                            .color(col().MUTED),
                    );
                    if !logs.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("· {}", logs.len()))
                                .size(11.0)
                                .color(col().FAINT),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::quiet_button(ui, crate::i18n::t("clicker.log.clear")).clicked() {
                            self.log_messages.lock().unwrap().clear();
                        }
                    });
                });
                ui.add_space(4.0);

                theme::inset_frame().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("clicker_log_scroll")
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            if logs.is_empty() {
                                theme::empty_state(
                                    ui,
                                    crate::i18n::t("clicker.log.empty"),
                                    "运行任务后，步骤结果会显示在这里",
                                );
                            } else {
                                for msg in &logs {
                                    ui.label(
                                        egui::RichText::new(msg)
                                            .size(11.5)
                                            .color(col().TEXT),
                                    );
                                }
                            }
                        });
                });
            });
    }

    fn render_csv_mode(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::toolbar_row(ui, |ui| {
            theme::field_label(ui, crate::i18n::t("clicker.field.csv"));
            ui.add_space(8.0);
            let path_w = (ui.available_width() - 180.0).max(120.0);
            ui.add(egui::TextEdit::singleline(&mut self.csv_path).desired_width(path_w));
            if theme::secondary_button(ui, crate::i18n::t("clicker.btn.browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_directory("D:\\")
                    .pick_file()
                {
                    let p = path.to_string_lossy().to_string();
                    self.load_csv(&p);
                }
            }
            if theme::secondary_button(ui, crate::i18n::t("clicker.btn.load")).clicked() && !self.csv_path.is_empty() {
                let p = self.csv_path.clone();
                self.load_csv(&p);
            }
        });

        ui.add_space(8.0);

        theme::toolbar_row(ui, |ui| {
            theme::field_label(ui, crate::i18n::t("clicker.field.interval"));
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.delay_ms)
                    .desired_width(72.0)
                    .hint_text("ms"),
            );
            ui.label(egui::RichText::new("ms").size(12.0).color(col().MUTED));
            ui.add_space(16.0);
            theme::field_label(ui, crate::i18n::t("clicker.field.threshold"));
            ui.add_space(8.0);
            let mut thr = self.match_threshold;
            ui.allocate_ui_with_layout(
                egui::vec2(140.0, theme::CTRL_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui
                        .add(egui::Slider::new(&mut thr, 0.5..=0.99).fixed_decimals(2))
                        .changed()
                    {
                        self.set_match_threshold(thr);
                    }
                },
            );
            let mut pv = self.pure_vision;
            if theme::themed_checkbox(ui, &mut pv, crate::i18n::t("clicker.pure_vision")).changed() {
                self.set_pure_vision(pv);
            }
        });

        ui.add_space(10.0);
        theme::hairline(ui);

        if !self.points.is_empty() {
            theme::field_label(ui, &format!("已加载 {} 个点", self.points.len()));
            ui.add_space(4.0);

            let current = *self.current_index.lock().unwrap();
            let state = self.state.lock().unwrap().clone();

            theme::inset_frame().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for (i, p) in self.points.iter().enumerate() {
                            let prefix = if state == AppState::Running && i == current {
                                "▶ "
                            } else if state == AppState::Running && i < current {
                                "✓ "
                            } else {
                                "  "
                            };
                            let template_indicator = if p.template_path.is_some() {
                                " · 模板"
                            } else {
                                ""
                            };
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}#{:03}  ({}, {}){}  {}",
                                    prefix, p.id, p.x, p.y, template_indicator, p.description
                                ))
                                .size(12.5)
                                .color(
                                    if state == AppState::Running && i == current {
                                        col().ACCENT
                                    } else {
                                        col().TEXT
                                    },
                                ),
                            );
                        }
                    });
            });
        }

        ui.add_space(10.0);
        theme::hairline(ui);

        let state = self.state.lock().unwrap().clone();
        theme::toolbar_row(ui, |ui| match state {
            AppState::Idle => {
                let can_start = !self.points.is_empty();
                if ui
                    .add_enabled(
                        can_start,
                        egui::Button::new(
                            egui::RichText::new(crate::i18n::t("clicker.btn.start"))
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(col().ACCENT_HOT)
                        .min_size(egui::vec2(0.0, theme::CTRL_H)),
                    )
                    .clicked()
                {
                    self.start_clicking(ctx);
                }
            }
            AppState::Running => {
                if theme::danger_button(ui, crate::i18n::t("clicker.btn.stop")).clicked() {
                    *self.state.lock().unwrap() = AppState::Idle;
                    *self.window_visible.lock().unwrap() = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
                ui.label(
                    egui::RichText::new("运行中…")
                        .size(12.0)
                        .color(col().MUTED),
                );
            }
            AppState::Done => {
                if theme::secondary_button(ui, crate::i18n::t("clicker.btn.reset")).clicked() {
                    *self.state.lock().unwrap() = AppState::Idle;
                    self.log_messages.lock().unwrap().clear();
                    *self.current_index.lock().unwrap() = 0;
                }
                ui.label(
                    egui::RichText::new("已完成")
                        .size(12.0)
                        .color(col().SUCCESS),
                );
            }
            AppState::Paused => {
                ui.label(
                    egui::RichText::new("已暂停 — 等待确认…")
                        .size(12.0)
                        .color(col().WARN),
                );
            }
        });
    }

    fn render_workflow_mode(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::toolbar_row(ui, |ui| {
            theme::field_label(ui, crate::i18n::t("clicker.field.element_dir"));
            ui.add_space(8.0);
            let path_w = (ui.available_width() - 100.0).max(120.0);
            ui.add(egui::TextEdit::singleline(&mut self.element_folder).desired_width(path_w));
            if theme::secondary_button(ui, crate::i18n::t("clicker.btn.browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new().set_directory("D:\\").pick_folder() {
                    self.element_folder = path.to_string_lossy().to_string();
                    self.persist_config();
                }
            }
        });

        ui.add_space(8.0);

        theme::toolbar_row(ui, |ui| {
            theme::field_label(ui, crate::i18n::t("clicker.field.workflow"));
            ui.add_space(8.0);
            let path_w = (ui.available_width() - 180.0).max(120.0);
            ui.add(egui::TextEdit::singleline(&mut self.workflow_path).desired_width(path_w));
            if theme::secondary_button(ui, crate::i18n::t("clicker.btn.browse")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Workflow", &["txt"])
                    .set_directory("D:\\")
                    .pick_file()
                {
                    let p = path.to_string_lossy().to_string();
                    self.load_workflow(&p);
                    self.persist_config();
                }
            }
            if theme::secondary_button(ui, crate::i18n::t("clicker.btn.load")).clicked() && !self.workflow_path.is_empty() {
                let p = self.workflow_path.clone();
                self.load_workflow(&p);
                self.persist_config();
            }
        });

        ui.add_space(8.0);

        theme::toolbar_row(ui, |ui| {
            theme::field_label(ui, "步骤间隔");
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.delay_ms)
                    .desired_width(72.0)
                    .hint_text("ms"),
            );
            ui.label(egui::RichText::new("ms").size(12.0).color(col().MUTED));
            ui.add_space(16.0);
            theme::field_label(ui, crate::i18n::t("clicker.field.threshold"));
            ui.add_space(8.0);
            let mut thr = self.match_threshold;
            ui.allocate_ui_with_layout(
                egui::vec2(140.0, theme::CTRL_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui
                        .add(egui::Slider::new(&mut thr, 0.5..=0.99).fixed_decimals(2))
                        .changed()
                    {
                        self.set_match_threshold(thr);
                    }
                },
            );
        });

        ui.add_space(6.0);
        egui::CollapsingHeader::new(crate::i18n::t("clicker.advanced"))
            .default_open(false)
            .show(ui, |ui| {
                theme::toolbar_row(ui, |ui| {
                    let mut pv = self.pure_vision;
                    if theme::themed_checkbox(ui, &mut pv, "纯视觉（仅匹配成功才点击）").changed()
                    {
                        self.set_pure_vision(pv);
                    }
                    let mut dbg = self.save_match_debug;
                    if theme::themed_checkbox(ui, &mut dbg, "失败时保存调试截图").changed() {
                        self.set_save_match_debug(dbg);
                    }
                });

                ui.add_space(6.0);
                theme::toolbar_row(ui, |ui| {
                    theme::field_label(ui, "默认重试");
                    ui.add_space(8.0);
                    let mut r = self.retries;
                    if ui.add(egui::DragValue::new(&mut r).range(0..=20)).changed() {
                        self.set_retries(r);
                    }
                    ui.label(egui::RichText::new("次").size(12.0).color(col().MUTED));
                    ui.add_space(12.0);
                    theme::field_label(ui, "间隔");
                    ui.add_space(8.0);
                    let mut ms = self.retry_ms;
                    if ui
                        .add(egui::DragValue::new(&mut ms).range(0..=60000).suffix(" ms"))
                        .changed()
                    {
                        self.set_retry_ms(ms);
                    }
                    ui.add_space(12.0);
                    let mut fail = self.on_fail.clone();
                    egui::ComboBox::from_id_salt("wf_on_fail")
                        .selected_text(match fail {
                            ClickFailAction::Skip => "失败跳过",
                            ClickFailAction::Abort => "失败中止",
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut fail, ClickFailAction::Skip, "失败跳过")
                                .changed()
                            {
                                self.set_on_fail(fail.clone());
                            }
                            if ui
                                .selectable_value(&mut fail, ClickFailAction::Abort, "失败中止")
                                .changed()
                            {
                                self.set_on_fail(fail.clone());
                            }
                        });
                });
            });

        ui.add_space(10.0);
        theme::hairline(ui);

        if !self.workflow_steps.is_empty() {
            theme::field_label(
                ui,
                &format!("工作流步骤 · {} 步", self.workflow_steps.len()),
            );
            ui.add_space(4.0);

            let current = *self.current_index.lock().unwrap();
            let state = self.state.lock().unwrap().clone();

            theme::inset_frame().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for (i, step) in self.workflow_steps.iter().enumerate() {
                            let prefix = if state == AppState::Running && i == current {
                                "▶ "
                            } else if step.executed {
                                "✓ "
                            } else {
                                "  "
                            };

                            let step_desc = match &step.step_type {
                                StepType::Click {
                                    element_name,
                                    or_elements,
                                    threshold,
                                    pure_vision,
                                    retries,
                                    retry_ms: _,
                                    on_fail,
                                } => {
                                    let names = workflow::merge_or_names(element_name, or_elements);
                                    let mut desc = format!("点击: {}", names.join(" 或 "));
                                    if let Some(t) = threshold {
                                        desc.push_str(&format!(" thr={:.2}", t));
                                    }
                                    if pure_vision == &Some(true) {
                                        desc.push_str(" [纯视觉]");
                                    }
                                    if let Some(r) = retries {
                                        if *r > 0 {
                                            desc.push_str(&format!(" 重试={}", r));
                                        }
                                    }
                                    if on_fail == &Some(ClickFailAction::Abort) {
                                        desc.push_str(" [中止]");
                                    }
                                    desc
                                }
                                StepType::Pause { message, .. } => format!("暂停: {}", message),
                                StepType::Manual { message, .. } => format!("人工: {}", message),
                                StepType::Wait { seconds } => format!("等待: {}s", seconds),
                                StepType::TypeText { text, .. } => {
                                    let p: String = text.chars().take(24).collect();
                                    format!(
                                        "输入: {}{}",
                                        p,
                                        if text.chars().count() > 24 { "…" } else { "" }
                                    )
                                }
                                StepType::LoopStart { times } => format!("循环 ×{}", times),
                                StepType::LoopWhileStart {
                                    element_name,
                                    or_elements,
                                    max_times,
                                    ..
                                } => {
                                    let names = workflow::merge_or_names(element_name, or_elements);
                                    format!("条件循环 '{}' ≤{}", names.join(" 或 "), max_times)
                                }
                                StepType::LoopEnd => "循环结束".into(),
                                StepType::IfVision {
                                    element_name,
                                    or_elements,
                                    then_jump,
                                    else_jump,
                                    ..
                                } => {
                                    let names = workflow::merge_or_names(element_name, or_elements);
                                    format!(
                                        "视觉条件 '{}' →{}|{}",
                                        names.join(" 或 "),
                                        then_jump + 1,
                                        else_jump + 1
                                    )
                                }
                                StepType::Goto { jump } => format!("跳转 →{}", jump + 1),
                            };

                            ui.label(
                                egui::RichText::new(format!("{}{}. {}", prefix, i + 1, step_desc))
                                    .size(12.5)
                                    .color(if state == AppState::Running && i == current {
                                        col().ACCENT
                                    } else {
                                        col().TEXT
                                    }),
                            );
                        }
                    });
            });
        }

        ui.add_space(10.0);
        theme::hairline(ui);

        let state = self.state.lock().unwrap().clone();
        theme::toolbar_row(ui, |ui| match state {
            AppState::Idle => {
                let can_start = !self.workflow_steps.is_empty() && !self.element_folder.is_empty();
                if ui
                    .add_enabled(
                        can_start,
                        egui::Button::new(
                            egui::RichText::new(crate::i18n::t("clicker.btn.start_wf"))
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(col().ACCENT_HOT)
                        .min_size(egui::vec2(0.0, theme::CTRL_H)),
                    )
                    .clicked()
                {
                    self.start_workflow(ctx);
                }
                if !self.workflow_steps.is_empty() && self.element_folder.is_empty() {
                    ui.label(
                        egui::RichText::new("请先选择元素目录")
                            .size(12.0)
                            .color(col().WARN),
                    );
                }
            }
            AppState::Running => {
                if theme::danger_button(ui, crate::i18n::t("clicker.btn.stop")).clicked() {
                    *self.state.lock().unwrap() = AppState::Idle;
                    *self.window_visible.lock().unwrap() = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
                ui.label(
                    egui::RichText::new("运行中…")
                        .size(12.0)
                        .color(col().MUTED),
                );
            }
            AppState::Paused => {
                ui.label(
                    egui::RichText::new("已暂停 — 等待确认")
                        .size(12.0)
                        .color(col().WARN),
                );
            }
            AppState::Done => {
                if theme::secondary_button(ui, crate::i18n::t("clicker.btn.reset")).clicked() {
                    *self.state.lock().unwrap() = AppState::Idle;
                    self.log_messages.lock().unwrap().clear();
                    *self.current_index.lock().unwrap() = 0;
                    for step in &mut self.workflow_steps {
                        step.executed = false;
                    }
                }
                ui.label(
                    egui::RichText::new("工作流已完成")
                        .size(12.0)
                        .color(col().SUCCESS),
                );
            }
        });
    }
}

fn parse_csv(path: &str) -> std::io::Result<Vec<ClickPoint>> {
    let content = fs::read_to_string(path)?;
    let mut points = Vec::new();

    for line in content.lines().skip(1) {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }

        let mut fields = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;

        for ch in line.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => {
                    fields.push(current.clone());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        fields.push(current);

        if fields.len() >= 3 {
            let id = fields[0].trim().parse::<u32>().unwrap_or(0);
            let x = fields[1].trim().parse::<i32>().unwrap_or(0);
            let y = fields[2].trim().parse::<i32>().unwrap_or(0);
            let description = if fields.len() >= 4 {
                fields[3].trim().to_string()
            } else {
                String::new()
            };
            let template_path = if fields.len() >= 5 {
                let path = fields[4].trim();
                if path.is_empty() {
                    None
                } else {
                    Some(path.to_string())
                }
            } else {
                None
            };
            let original_width = if fields.len() >= 6 {
                fields[5].trim().parse::<u32>().ok()
            } else {
                None
            };
            let original_height = if fields.len() >= 7 {
                fields[6].trim().parse::<u32>().ok()
            } else {
                None
            };
            let action_name = if fields.len() >= 8 {
                let name = fields[7].trim();
                if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                }
            } else {
                None
            };
            points.push(ClickPoint {
                id,
                x,
                y,
                description,
                template_path,
                original_width,
                original_height,
                action_name,
            });
        }
    }

    Ok(points)
}

#[cfg(windows)]
fn screen_size() -> (i32, i32) {
    // Virtual desktop (all monitors), not primary-only SM_CXSCREEN.
    unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(GetSystemMetrics(SM_CXSCREEN)),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(GetSystemMetrics(SM_CYSCREEN)),
        )
    }
}

#[cfg(not(windows))]
fn screen_size() -> (i32, i32) {
    (1920, 1080)
}

#[cfg(windows)]
fn click_at(x: i32, y: i32) {
    unsafe {
        // SetCursorPos accepts absolute virtual-desktop coords — reliable on multi-mon.
        let _ = SetCursorPos(x, y);
    }
    thread::sleep(Duration::from_millis(8));

    let down_input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTDOWN,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up_input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(&[down_input], std::mem::size_of::<INPUT>() as i32);
        thread::sleep(Duration::from_millis(20));
        let _ = SendInput(&[up_input], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
fn click_at(x: i32, y: i32) {
    // Experimental non-Windows build: click injection is not implemented yet.
    let _ = (x, y);
}

#[derive(Clone, Debug)]
struct VisionHit {
    name: String,
    x: i32,
    y: i32,
}

/// Per-run vision state: reuse IfVision coords for Click + grow ROI from last hits.
#[derive(Default)]
struct VisionSession {
    pending: Option<VisionHit>,
    last_by_name: std::collections::HashMap<String, (i32, i32)>,
}

impl VisionSession {
    fn remember(&mut self, name: &str, x: i32, y: i32) {
        self.last_by_name.insert(name.to_string(), (x, y));
    }

    fn take_pending_for(&mut self, names: &[String]) -> Option<VisionHit> {
        let hit = self.pending.take()?;
        if names.iter().any(|n| n == &hit.name) {
            Some(hit)
        } else {
            // Wrong element — keep for nothing; drop.
            None
        }
    }

    fn last_for(&self, name: &str) -> Option<(i32, i32)> {
        self.last_by_name.get(name).copied()
    }
}

/// Expand recorded bbox (+ optional last hit) so small UI drift still hits ROI.
fn build_dynamic_roi(
    element: &UIElement,
    last: Option<(i32, i32)>,
    screen_w: i32,
    screen_h: i32,
) -> (i32, i32, i32, i32) {
    const PAD: i32 = 240;
    let mut x0 = element.bbox_x - PAD;
    let mut y0 = element.bbox_y - PAD;
    let mut x1 = element.bbox_x + element.bbox_width + PAD;
    let mut y1 = element.bbox_y + element.bbox_height + PAD;
    if let Some((cx, cy)) = last {
        x0 = x0.min(cx - PAD);
        y0 = y0.min(cy - PAD);
        x1 = x1.max(cx + PAD);
        y1 = y1.max(cy + PAD);
    }
    // Also cover scaled center in case bbox is tiny/wrong
    let scaled_cx = if element.screen_width > 0 {
        (element.center_x as f64 * screen_w as f64 / element.screen_width as f64) as i32
    } else {
        element.center_x
    };
    let scaled_cy = if element.screen_height > 0 {
        (element.center_y as f64 * screen_h as f64 / element.screen_height as f64) as i32
    } else {
        element.center_y
    };
    x0 = x0.min(scaled_cx - PAD);
    y0 = y0.min(scaled_cy - PAD);
    x1 = x1.max(scaled_cx + PAD);
    y1 = y1.max(scaled_cy + PAD);

    x0 = x0.clamp(0, screen_w.saturating_sub(1));
    y0 = y0.clamp(0, screen_h.saturating_sub(1));
    x1 = x1.clamp(x0 + 1, screen_w);
    y1 = y1.clamp(y0 + 1, screen_h);
    (x0, y0, x1 - x0, y1 - y0)
}

fn locate_element(
    element_name: &str,
    db: &ElementDatabase,
    element_folder: &str,
    log: &Arc<Mutex<Vec<String>>>,
    log_file: &str,
    step_index: usize,
    threshold: f32,
    vision: &Arc<Mutex<VisionSession>>,
    save_match_debug: bool,
    tag: &str,
) -> Option<VisionHit> {
    let quiet = tag == "GoneCheck";
    match db.load_element(element_name) {
        Ok(Some((element, states))) => {
            let (current_w, current_h) = screen_size();
            let Some(state) = states.iter().find(|s| s.is_primary) else {
                if !quiet {
                    log_write(
                        log,
                        log_file,
                        format!(
                            "Step {}: {} — no primary state for '{}'",
                            step_index + 1,
                            tag,
                            element_name
                        ),
                    );
                }
                return None;
            };
            let img_filename = Path::new(&state.screenshot_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&state.screenshot_path);
            let template_path = format!("{}\\{}", element_folder, img_filename);
            if !Path::new(&template_path).exists() {
                if !quiet {
                    log_write(
                        log,
                        log_file,
                        format!(
                            "Step {}: {} — template missing: {}",
                            step_index + 1,
                            tag,
                            template_path
                        ),
                    );
                }
                return None;
            }

            let last = vision.lock().unwrap().last_for(element_name);
            let (roi_x, roi_y, roi_w, roi_h) =
                build_dynamic_roi(&element, last, current_w, current_h);
            let roi_area = (roi_w as i64) * (roi_h as i64);
            let screen_area = (current_w as i64) * (current_h as i64).max(1);
            let roi_huge = roi_area * 10 > screen_area * 4; // >40% screen → skip ROI

            if !roi_huge {
                let roi =
                    find_template_with_roi(&template_path, threshold, roi_x, roi_y, roi_w, roi_h);
                if let Some((x, y)) = roi.hit {
                    if !quiet {
                        log_write(
                            log,
                            log_file,
                            format!(
                                "Step {}: [{} ROI] '{}' at ({}, {}) score={:.3} thr={:.2}",
                                step_index + 1,
                                tag,
                                element_name,
                                x,
                                y,
                                roi.best_score,
                                threshold
                            ),
                        );
                    }
                    vision.lock().unwrap().remember(element_name, x, y);
                    return Some(VisionHit {
                        name: element_name.to_string(),
                        x,
                        y,
                    });
                }
                if !quiet {
                    log_write(
                        log,
                        log_file,
                        format!(
                            "Step {}: {} ROI miss best={:.3} thr={:.2} (pad search), trying full-screen...",
                            step_index + 1,
                            tag,
                            roi.best_score,
                            threshold
                        ),
                    );
                }
            }

            let full = find_template(&template_path, threshold);
            if let Some((x, y)) = full.hit {
                if !quiet {
                    log_write(
                        log,
                        log_file,
                        format!(
                            "Step {}: [{} Full] '{}' at ({}, {}) score={:.3} thr={:.2} state={}",
                            step_index + 1,
                            tag,
                            element_name,
                            x,
                            y,
                            full.best_score,
                            threshold,
                            state.state_name
                        ),
                    );
                }
                vision.lock().unwrap().remember(element_name, x, y);
                return Some(VisionHit {
                    name: element_name.to_string(),
                    x,
                    y,
                });
            }
            if !quiet {
                log_write(
                    log,
                    log_file,
                    format!(
                        "Step {}: {} miss '{}' best={:.3} thr={:.2}",
                        step_index + 1,
                        tag,
                        element_name,
                        full.best_score,
                        threshold
                    ),
                );
            }
            if save_match_debug && !quiet {
                save_debug_screen(element_folder, element_name, full.best_score);
            }
            None
        }
        Ok(None) => {
            if !quiet {
                log_write(
                    log,
                    log_file,
                    format!(
                        "Step {}: {} — element '{}' not in DB",
                        step_index + 1,
                        tag,
                        element_name
                    ),
                );
            }
            None
        }
        Err(e) => {
            if !quiet {
                log_write(
                    log,
                    log_file,
                    format!(
                        "Step {}: {} DB error '{}': {}",
                        step_index + 1,
                        tag,
                        element_name,
                        e
                    ),
                );
            }
            None
        }
    }
}

/// After a successful click, wait until templates disappear so the next loop counts a new appearance.
fn wait_until_elements_gone(
    names: &[String],
    db: &ElementDatabase,
    element_folder: &str,
    log: &Arc<Mutex<Vec<String>>>,
    log_file: &str,
    step_index: usize,
    threshold: f32,
    vision: &Arc<Mutex<VisionSession>>,
    state: &Arc<Mutex<AppState>>,
) {
    const TIMEOUT_MS: u64 = 8_000;
    const POLL_MS: u64 = 250;
    let started = std::time::Instant::now();
    let label = names.join(" or ");
    log_write(
        log,
        log_file,
        format!(
            "Step {}: waiting for '{}' to disappear (≤{}ms)...",
            step_index + 1,
            label,
            TIMEOUT_MS
        ),
    );
    loop {
        {
            let s = state.lock().unwrap();
            if *s != AppState::Running && *s != AppState::Paused {
                return;
            }
        }
        let mut still = false;
        for name in names {
            if locate_element(
                name,
                db,
                element_folder,
                log,
                log_file,
                step_index,
                threshold,
                vision,
                false,
                "GoneCheck",
            )
            .is_some()
            {
                still = true;
                break;
            }
        }
        if !still {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: '{}' gone after {}ms",
                    step_index + 1,
                    label,
                    started.elapsed().as_millis()
                ),
            );
            return;
        }
        if started.elapsed().as_millis() as u64 >= TIMEOUT_MS {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: '{}' still visible after {}ms — continue anyway",
                    step_index + 1,
                    label,
                    TIMEOUT_MS
                ),
            );
            return;
        }
        thread::sleep(Duration::from_millis(POLL_MS));
    }
}

fn try_execute_click(
    element_name: &str,
    db: &ElementDatabase,
    element_folder: &str,
    log: &Arc<Mutex<Vec<String>>>,
    log_file: &str,
    step_index: usize,
    delay: u64,
    threshold: f32,
    pure_vision: bool,
    save_match_debug: bool,
    vision: &Arc<Mutex<VisionSession>>,
) -> bool {
    match db.load_element(element_name) {
        Ok(Some((element, _states))) => {
            let (current_w, current_h) = screen_size();
            let matched = locate_element(
                element_name,
                db,
                element_folder,
                log,
                log_file,
                step_index,
                threshold,
                vision,
                save_match_debug,
                "Click",
            );

            let (click_x, click_y) = if let Some(hit) = matched {
                (hit.x, hit.y)
            } else if pure_vision {
                log_write(
                    log,
                    log_file,
                    format!(
                        "Step {}: pure-vision — no match for '{}'",
                        step_index + 1,
                        element_name
                    ),
                );
                return false;
            } else {
                let scaled_x = if element.screen_width > 0 {
                    (element.center_x as f64 * current_w as f64 / element.screen_width as f64)
                        as i32
                } else {
                    element.center_x
                };
                let scaled_y = if element.screen_height > 0 {
                    (element.center_y as f64 * current_h as f64 / element.screen_height as f64)
                        as i32
                } else {
                    element.center_y
                };
                log_write(
                    log,
                    log_file,
                    format!(
                        "Step {}: [Fallback DB coords] '{}' at ({}, {})",
                        step_index + 1,
                        element_name,
                        scaled_x,
                        scaled_y
                    ),
                );
                (scaled_x, scaled_y)
            };

            log_write(
                log,
                log_file,
                format!(
                    "Step {}: → clicking at ({}, {})",
                    step_index + 1,
                    click_x,
                    click_y
                ),
            );
            click_at(click_x, click_y);
            vision
                .lock()
                .unwrap()
                .remember(element_name, click_x, click_y);
            thread::sleep(Duration::from_millis(delay));
            true
        }
        Ok(None) => {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: [Error] Element '{}' not found in DB",
                    step_index + 1,
                    element_name
                ),
            );
            false
        }
        Err(e) => {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: [DB Error] '{}': {}",
                    step_index + 1,
                    element_name,
                    e
                ),
            );
            false
        }
    }
}

#[derive(Debug)]
enum ClickOutcome {
    Ok,
    Failed,
    Abort,
}

fn execute_click_with_retries(
    names: &[String],
    db: &ElementDatabase,
    element_folder: &str,
    log: &Arc<Mutex<Vec<String>>>,
    log_file: &str,
    step_index: usize,
    delay: u64,
    threshold: f32,
    pure_vision: bool,
    retries: u32,
    retry_ms: u64,
    on_fail: ClickFailAction,
    save_match_debug: bool,
    vision: &Arc<Mutex<VisionSession>>,
    app_state: &Arc<Mutex<AppState>>,
) -> ClickOutcome {
    let label = names.join(" or ");

    // Reuse fresh IfVision hit — skip a second full search in the same loop turn.
    if let Some(hit) = vision.lock().unwrap().take_pending_for(names) {
        log_write(
            log,
            log_file,
            format!(
                "Step {}: reuse IfVision hit '{}' @({}, {}) — click without re-scan",
                step_index + 1,
                hit.name,
                hit.x,
                hit.y
            ),
        );
        click_at(hit.x, hit.y);
        vision.lock().unwrap().remember(&hit.name, hit.x, hit.y);
        thread::sleep(Duration::from_millis(delay));
        wait_until_elements_gone(
            names,
            db,
            element_folder,
            log,
            log_file,
            step_index,
            threshold,
            vision,
            app_state,
        );
        return ClickOutcome::Ok;
    }

    let total_tries = retries.saturating_add(1);
    for attempt in 0..total_tries {
        if attempt > 0 {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: retry {}/{} after {}ms",
                    step_index + 1,
                    attempt,
                    retries,
                    retry_ms
                ),
            );
            thread::sleep(Duration::from_millis(retry_ms));
        }
        for (ni, name) in names.iter().enumerate() {
            let last_try = attempt + 1 == total_tries && ni + 1 == names.len();
            if try_execute_click(
                name,
                db,
                element_folder,
                log,
                log_file,
                step_index,
                delay,
                threshold,
                pure_vision,
                save_match_debug && last_try,
                vision,
            ) {
                if names.len() > 1 {
                    log_write(
                        log,
                        log_file,
                        format!("Step {}: OR hit '{}' (of {})", step_index + 1, name, label),
                    );
                }
                wait_until_elements_gone(
                    names,
                    db,
                    element_folder,
                    log,
                    log_file,
                    step_index,
                    threshold,
                    vision,
                    app_state,
                );
                return ClickOutcome::Ok;
            }
        }
    }

    match on_fail {
        ClickFailAction::Abort => ClickOutcome::Abort,
        ClickFailAction::Skip => {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: click failed — skip and continue ('{}')",
                    step_index + 1,
                    label
                ),
            );
            ClickOutcome::Failed
        }
    }
}

/// Vision-only probe (no click). Tries OR candidates each attempt; retries on total miss.
fn probe_any_element(
    names: &[String],
    db: &ElementDatabase,
    element_folder: &str,
    log: &Arc<Mutex<Vec<String>>>,
    log_file: &str,
    step_index: usize,
    threshold: f32,
    retries: u32,
    retry_ms: u64,
    vision: &Arc<Mutex<VisionSession>>,
) -> Option<VisionHit> {
    let total_tries = retries.saturating_add(1);
    for attempt in 0..total_tries {
        if attempt > 0 {
            log_write(
                log,
                log_file,
                format!(
                    "Step {}: vision probe retry {}/{} after {}ms",
                    step_index + 1,
                    attempt,
                    retries,
                    retry_ms
                ),
            );
            thread::sleep(Duration::from_millis(retry_ms));
        }
        for name in names {
            if let Some(hit) = locate_element(
                name,
                db,
                element_folder,
                log,
                log_file,
                step_index,
                threshold,
                vision,
                false,
                "Probe",
            ) {
                if names.len() > 1 {
                    log_write(
                        log,
                        log_file,
                        format!("Step {}: OR vision hit '{}'", step_index + 1, name),
                    );
                }
                return Some(hit);
            }
        }
    }
    None
}

fn save_debug_screen(element_folder: &str, element_name: &str, best_score: f32) {
    let Some(img) = capture_screen_rgba() else {
        return;
    };
    let dir = Path::new(element_folder).join("debug");
    let _ = fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let safe: String = element_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!(
        "miss_{}_{}_{:.0}.png",
        safe,
        ts,
        best_score * 1000.0
    ));
    let _ = img.save(&path);
}

fn log_write(log: &Arc<Mutex<Vec<String>>>, log_file: &str, msg: String) {
    {
        let mut guard = log.lock().unwrap();
        guard.push(msg.clone());
        const MAX_UI_LOG: usize = 500;
        if guard.len() > MAX_UI_LOG {
            let drop_n = guard.len() - MAX_UI_LOG;
            guard.drain(0..drop_n);
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_file) {
        let _ = writeln!(f, "{}", msg);
    }
}

fn step_type_hud_label(st: &StepType) -> String {
    match st {
        StepType::Click {
            element_name,
            or_elements,
            pure_vision,
            ..
        } => {
            let names = workflow::merge_or_names(element_name, or_elements);
            let mut s = format!("点击  {}", names.join(" ∨ "));
            if pure_vision == &Some(true) {
                s.push_str("  ·纯视觉");
            }
            s
        }
        StepType::Wait { seconds } => format!("等待  {} 秒", seconds),
        StepType::TypeText { text, .. } => {
            let p: String = text.chars().take(20).collect();
            format!(
                "键盘  {}{}",
                p,
                if text.chars().count() > 20 { "…" } else { "" }
            )
        }
        StepType::Pause { message } => {
            let short: String = message.chars().take(24).collect();
            format!("暂停  {}", short)
        }
        StepType::Manual { message, .. } => {
            let short: String = message.chars().take(24).collect();
            format!("人工  {}", short)
        }
        StepType::LoopStart { times } => format!("循环开始  ×{}", times),
        StepType::LoopWhileStart {
            element_name,
            or_elements,
            max_times,
            ..
        } => {
            let names = workflow::merge_or_names(element_name, or_elements);
            format!("条件循环  {}  ≤{}", names.join(" ∨ "), max_times)
        }
        StepType::LoopEnd => "循环结束".into(),
        StepType::IfVision {
            element_name,
            or_elements,
            ..
        } => {
            let names = workflow::merge_or_names(element_name, or_elements);
            format!("视觉条件  {}", names.join(" ∨ "))
        }
        StepType::Goto { jump } => format!("跳转  →{}", jump + 1),
    }
}

/// Skip to the step after the LoopEnd that matches a loop head at `head_pc`.
fn skip_after_matching_loop_end(steps: &[WorkflowStep], head_pc: usize) -> usize {
    let mut depth = 0i32;
    for i in (head_pc + 1)..steps.len() {
        match &steps[i].step_type {
            StepType::LoopStart { .. } | StepType::LoopWhileStart { .. } => depth += 1,
            StepType::LoopEnd => {
                if depth == 0 {
                    return i + 1;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    steps.len()
}

fn capture_screen_rgba() -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    // Prefer primary / first for debug dumps; template match uses capture_all_monitors.
    let caps = crate::screen::capture_all_monitors();
    let cap = caps.into_iter().next()?;
    Some(cap.image)
}

struct MatchResult {
    hit: Option<(i32, i32)>,
    best_score: f32,
}

fn match_on_image(
    screen_img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    template: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    threshold: f32,
    origin_x: i32,
    origin_y: i32,
    roi: Option<(i32, i32, i32, i32)>,
) -> MatchResult {
    let (t_width, t_height) = template.dimensions();
    let (s_width, s_height) = screen_img.dimensions();

    let (search_x_start, search_y_start, search_x_end, search_y_end) =
        if let Some((rx, ry, rw, rh)) = roi {
            // ROI is absolute screen coords → local to this monitor
            let lx0 = (rx - origin_x).max(0) as u32;
            let ly0 = (ry - origin_y).max(0) as u32;
            let lx1 = (rx + rw - origin_x).clamp(0, s_width as i32) as u32;
            let ly1 = (ry + rh - origin_y).clamp(0, s_height as i32) as u32;
            (lx0, ly0, lx1, ly1)
        } else {
            (0, 0, s_width, s_height)
        };

    if search_x_end <= search_x_start || search_y_end <= search_y_start {
        return MatchResult {
            hit: None,
            best_score: 0.0,
        };
    }
    if t_width > (search_x_end - search_x_start) || t_height > (search_y_end - search_y_start) {
        return MatchResult {
            hit: None,
            best_score: 0.0,
        };
    }

    let mut best_score = f32::MIN;
    let mut best_pos = None;

    for y in search_y_start..=(search_y_end - t_height) {
        for x in search_x_start..=(search_x_end - t_width) {
            let score = calculate_match_score(screen_img, template, x, y);
            if score > best_score {
                best_score = score;
                best_pos = Some((
                    origin_x + x as i32 + (t_width / 2) as i32,
                    origin_y + y as i32 + (t_height / 2) as i32,
                ));
            }
        }
    }

    MatchResult {
        hit: if best_score >= threshold {
            best_pos
        } else {
            None
        },
        best_score: if best_score == f32::MIN {
            0.0
        } else {
            best_score
        },
    }
}

fn find_template(template_path: &str, threshold: f32) -> MatchResult {
    let template = match image::open(template_path) {
        Ok(img) => img.to_rgba8(),
        Err(_) => {
            return MatchResult {
                hit: None,
                best_score: f32::NEG_INFINITY,
            }
        }
    };

    let mut best = MatchResult {
        hit: None,
        best_score: f32::NEG_INFINITY,
    };
    for cap in crate::screen::capture_all_monitors() {
        let r = match_on_image(&cap.image, &template, threshold, cap.x, cap.y, None);
        if r.best_score > best.best_score {
            best = r;
        }
    }
    best
}

fn find_template_with_roi(
    template_path: &str,
    threshold: f32,
    roi_x: i32,
    roi_y: i32,
    roi_width: i32,
    roi_height: i32,
) -> MatchResult {
    let template = match image::open(template_path) {
        Ok(img) => img.to_rgba8(),
        Err(_) => {
            return MatchResult {
                hit: None,
                best_score: f32::NEG_INFINITY,
            }
        }
    };

    let mut best = MatchResult {
        hit: None,
        best_score: f32::NEG_INFINITY,
    };
    let roi = Some((roi_x, roi_y, roi_width, roi_height));
    for cap in crate::screen::capture_all_monitors() {
        let r = match_on_image(&cap.image, &template, threshold, cap.x, cap.y, roi);
        if r.best_score > best.best_score {
            best = r;
        }
    }
    best
}

fn calculate_match_score(
    screen: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    template: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    offset_x: u32,
    offset_y: u32,
) -> f32 {
    let (t_width, t_height) = template.dimensions();
    let mut sum_diff = 0i64;
    let mut count = 0u32;

    for ty in 0..t_height {
        for tx in 0..t_width {
            let sx = offset_x + tx;
            let sy = offset_y + ty;

            let t_pixel = template.get_pixel(tx, ty);
            let s_pixel = screen.get_pixel(sx, sy);

            let diff_r = (t_pixel[0] as i32 - s_pixel[0] as i32).abs();
            let diff_g = (t_pixel[1] as i32 - s_pixel[1] as i32).abs();
            let diff_b = (t_pixel[2] as i32 - s_pixel[2] as i32).abs();

            sum_diff += (diff_r + diff_g + diff_b) as i64;
            count += 1;
        }
    }

    let avg_diff = sum_diff as f32 / count as f32;
    let max_diff = 255.0 * 3.0;
    1.0 - (avg_diff / max_diff)
}

/// Parse `text` into keystrokes. Tokens like `{Enter}` become special keys.
fn type_text_sequence(text: &str, interval_ms: u64) {
    let units = parse_type_units(text);
    for (i, unit) in units.iter().enumerate() {
        if i > 0 && interval_ms > 0 {
            thread::sleep(Duration::from_millis(interval_ms));
        }
        match unit {
            TypeUnit::Char(ch) => send_unicode_char(*ch),
            TypeUnit::Key(vk) => send_vk_tap(*vk),
            TypeUnit::Chord { mods, key } => send_vk_chord(mods, *key),
        }
    }
}

enum TypeUnit {
    Char(char),
    Key(u16),
    Chord { mods: Vec<u16>, key: u16 },
}

fn parse_type_units(text: &str) -> Vec<TypeUnit> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                out.push(TypeUnit::Char('{'));
                i += 2;
                continue;
            }
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '}') {
                let token: String = chars[i + 1..i + 1 + end].iter().collect();
                if let Some(unit) = map_special_token(&token) {
                    out.push(unit);
                    i += end + 2;
                    continue;
                }
            }
            // Unrecognized `{...}` → emit literally
            out.push(TypeUnit::Char('{'));
            i += 1;
            continue;
        }
        if chars[i] == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
            out.push(TypeUnit::Char('}'));
            i += 2;
            continue;
        }
        out.push(TypeUnit::Char(chars[i]));
        i += 1;
    }
    out
}

fn map_special_token(token: &str) -> Option<TypeUnit> {
    // VK codes (Win32) — kept as raw u16 so parsing is OS-agnostic.
    const VK_BACK: u16 = 0x08;
    const VK_TAB: u16 = 0x09;
    const VK_RETURN: u16 = 0x0D;
    const VK_ESCAPE: u16 = 0x1B;
    const VK_SPACE: u16 = 0x20;
    const VK_END: u16 = 0x23;
    const VK_HOME: u16 = 0x24;
    const VK_LEFT: u16 = 0x25;
    const VK_UP: u16 = 0x26;
    const VK_RIGHT: u16 = 0x27;
    const VK_DOWN: u16 = 0x28;
    const VK_DELETE: u16 = 0x2E;
    const VK_CONTROL: u16 = 0x11;

    let t = token.trim();
    let lower = t.to_ascii_lowercase();
    let vk = match lower.as_str() {
        "enter" | "return" | "ret" => Some(VK_RETURN),
        "tab" => Some(VK_TAB),
        "esc" | "escape" => Some(VK_ESCAPE),
        "backspace" | "bs" | "back" => Some(VK_BACK),
        "delete" | "del" => Some(VK_DELETE),
        "space" => Some(VK_SPACE),
        "up" => Some(VK_UP),
        "down" => Some(VK_DOWN),
        "left" => Some(VK_LEFT),
        "right" => Some(VK_RIGHT),
        "home" => Some(VK_HOME),
        "end" => Some(VK_END),
        _ => None,
    };
    if let Some(k) = vk {
        return Some(TypeUnit::Key(k));
    }
    if let Some((mod_s, key_s)) = t.split_once('+') {
        let mod_l = mod_s.trim().to_ascii_lowercase();
        let key_l = key_s.trim().to_ascii_lowercase();
        if mod_l == "ctrl" || mod_l == "control" {
            let key = match key_l.as_str() {
                "a" => 0x41u16,
                "c" => 0x43,
                "v" => 0x56,
                "x" => 0x58,
                "z" => 0x5A,
                "s" => 0x53,
                "y" => 0x59,
                "f" => 0x46,
                _ => return None,
            };
            return Some(TypeUnit::Chord {
                mods: vec![VK_CONTROL],
                key,
            });
        }
    }
    None
}

#[cfg(windows)]
fn send_unicode_char(ch: char) {
    let mut buf = [0u16; 2];
    let encoded = ch.encode_utf16(&mut buf);
    for &unit in encoded.iter() {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            let _ = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        }
    }
}

#[cfg(not(windows))]
fn send_unicode_char(_ch: char) {}

#[cfg(windows)]
fn send_vk_tap(vk: u16) {
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: Default::default(),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
fn send_vk_tap(_vk: u16) {}

#[cfg(windows)]
fn send_vk_chord(mods: &[u16], key: u16) {
    let mut inputs = Vec::new();
    for &m in mods {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(m),
                    wScan: 0,
                    dwFlags: Default::default(),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                wScan: 0,
                dwFlags: Default::default(),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    for &m in mods.iter().rev() {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(m),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
fn send_vk_chord(_mods: &[u16], _key: u16) {}
