use crate::common::data_dir;
use crate::theme::{self, colors};
use crate::workflow::{self, StepType, WorkflowStep};
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
use xcap::Monitor;

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

#[derive(Serialize, Deserialize)]
struct AppConfig {
    element_folder: String,
    workflow_path: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            element_folder: String::new(),
            workflow_path: String::new(),
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

    fn load_element(&self, element_name: &str) -> SqlResult<Option<(UIElement, Vec<ElementState>)>> {
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

#[derive(Clone, PartialEq)]
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
    Pause { message: String },
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

    dialog_pending: Arc<Mutex<Option<DialogKind>>>,
    dialog_confirm: Arc<(Mutex<bool>, Condvar)>,

    window_visible: Arc<Mutex<bool>>,
    dialog_brought_to_front: bool,
}

impl ClickerApp {
    pub fn new(default_element_folder: String) -> Self {
        let config = load_config();
        let element_folder = if !config.element_folder.is_empty() {
            config.element_folder
        } else {
            default_element_folder
        };
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

            dialog_pending: Arc::new(Mutex::new(None)),
            dialog_confirm: Arc::new((Mutex::new(false), Condvar::new())),

            window_visible: Arc::new(Mutex::new(true)),
            dialog_brought_to_front: false,
        }
    }

    /// Agent entrypoint: set step/click delay in milliseconds.
    pub fn set_delay_ms(&mut self, delay_ms: u64) {
        self.delay_ms = delay_ms.to_string();
    }

    /// Agent entrypoint: set element folder (contains db/images).
    pub fn set_element_folder(&mut self, folder: String) {
        self.element_folder = folder;
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
        let state = self.state.clone();
        let current_index = self.current_index.clone();
        let log = self.log_messages.clone();
        let dialog_pending = self.dialog_pending.clone();
        let dialog_confirm = self.dialog_confirm.clone();
        let window_visible = self.window_visible.clone();
        let ctx_clone = ctx.clone();

        *state.lock().unwrap() = AppState::Running;
        *current_index.lock().unwrap() = 0;
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
                format!("DB: {}\\mouse_recorder.db", element_folder),
            );

            let db_path = format!("{}\\mouse_recorder.db", element_folder);
            let db = ElementDatabase::new(db_path);

            for (i, step) in steps.iter().enumerate() {
                {
                    let s = state.lock().unwrap();
                    if *s != AppState::Running && *s != AppState::Paused {
                        return;
                    }
                }
                *current_index.lock().unwrap() = i;
                match &step.step_type {
                    StepType::Click {
                        element_name,
                        fallback_element,
                    } => {
                        if !try_execute_click(
                            element_name,
                            &db,
                            &element_folder,
                            &log,
                            &log_file,
                            i,
                            delay,
                        ) {
                            if let Some(fb) = fallback_element {
                                log_write(
                                    &log,
                                    &log_file,
                                    format!("Step {}: → Fallback to '{}'", i + 1, fb),
                                );
                                try_execute_click(
                                    fb,
                                    &db,
                                    &element_folder,
                                    &log,
                                    &log_file,
                                    i,
                                    delay,
                                );
                            }
                        }
                    }

                    StepType::Wait { seconds } => {
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Wait {}s", i + 1, seconds),
                        );
                        thread::sleep(Duration::from_secs(*seconds as u64));
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

                        *window_visible.lock().unwrap() = true;
                        ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));

                        let (lock, cvar) = &*dialog_confirm;
                        let mut confirmed = lock.lock().unwrap();
                        *confirmed = false;
                        confirmed = cvar.wait_while(confirmed, |c| !*c).unwrap();
                        drop(confirmed);

                        *dialog_pending.lock().unwrap() = None;
                        if *state.lock().unwrap() != AppState::Paused {
                            log_write(
                                &log,
                                &log_file,
                                format!("Step {}: Stopped by user", i + 1),
                            );
                            *window_visible.lock().unwrap() = true;
                            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            return;
                        }
                        *state.lock().unwrap() = AppState::Running;
                        log_write(&log, &log_file, format!("Step {}: Resumed", i + 1));
                        *window_visible.lock().unwrap() = false;
                        ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
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

                        *window_visible.lock().unwrap() = true;
                        ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));

                        let (lock, cvar) = &*dialog_confirm;
                        let mut confirmed = lock.lock().unwrap();
                        *confirmed = false;
                        confirmed = cvar.wait_while(confirmed, |c| !*c).unwrap();
                        drop(confirmed);

                        *dialog_pending.lock().unwrap() = None;
                        if *state.lock().unwrap() != AppState::Paused {
                            log_write(
                                &log,
                                &log_file,
                                format!("Step {}: Stopped by user", i + 1),
                            );
                            *window_visible.lock().unwrap() = true;
                            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            return;
                        }
                        *state.lock().unwrap() = AppState::Running;
                        log_write(
                            &log,
                            &log_file,
                            format!("Step {}: Manual done", i + 1),
                        );
                        *window_visible.lock().unwrap() = false;
                        ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                }
            }

            *state.lock().unwrap() = AppState::Done;
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
                        (
                            (p.x as f64 * scale_x) as i32,
                            (p.y as f64 * scale_y) as i32,
                        )
                    } else {
                        (p.x, p.y)
                    };

                let (click_x, click_y, _method) = if let Some(ref template_path) = p.template_path {
                    match find_template(template_path, 0.8) {
                        Some((x, y)) => {
                            log.lock().unwrap().push(format!(
                                "#{:03} [Template Match] ({}, {}) - {}",
                                p.id, x, y, p.description
                            ));
                            (x, y, "template")
                        }
                        None => {
                            log.lock().unwrap().push(format!(
                                "#{:03} [Fallback to Scaled] ({}, {}) - {} (template not found)",
                                p.id, scaled_x, scaled_y, p.description
                            ));
                            (scaled_x, scaled_y, "fallback")
                        }
                    }
                } else {
                    let res_info =
                        if let (Some(orig_w), Some(orig_h)) = (p.original_width, p.original_height)
                        {
                            format!(" [scaled from {}x{}]", orig_w, orig_h)
                        } else {
                            String::new()
                        };
                    log.lock().unwrap().push(format!(
                        "#{:03} [Coords] ({}, {}){}  - {}",
                        p.id, scaled_x, scaled_y, res_info, p.description
                    ));
                    (scaled_x, scaled_y, "coords")
                };

                click_at(click_x, click_y);
                thread::sleep(Duration::from_millis(delay));
            }

            *state.lock().unwrap() = AppState::Done;
            *window_visible.lock().unwrap() = true;
            ctx_clone.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            log.lock().unwrap().push("All clicks completed!".to_string());
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

            egui::Window::new("等待确认")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_min_width(320.0);
                    match kind {
                        DialogKind::Pause { message } => {
                            ui.heading("流程已暂停");
                            ui.add_space(6.0);
                            ui.label(message.as_str());
                        }
                        DialogKind::Manual {
                            message,
                            instruction,
                        } => {
                            ui.heading("需要人工操作");
                            ui.add_space(6.0);
                            ui.label(message.as_str());
                            if let Some(inst) = instruction {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.label(format!("操作说明: {}", inst));
                            }
                        }
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("停止执行").clicked() {
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
                        if ui.button(btn_label).clicked() {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            theme::section_header(ui, "自动点击", "CSV 坐标序列或工作流文件回放");

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("模式").color(colors::MUTED));
                ui.selectable_value(&mut self.mode, AppMode::CsvMode, "CSV");
                ui.selectable_value(&mut self.mode, AppMode::WorkflowMode, "工作流文件");
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            if self.mode == AppMode::CsvMode {
                self.render_csv_mode(ui, ctx);
            } else {
                self.render_workflow_mode(ui, ctx);
            }

            ui.add_space(8.0);

            let logs = self.log_messages.lock().unwrap();
            if !logs.is_empty() {
                ui.label(egui::RichText::new("日志").color(colors::MUTED));
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for msg in logs.iter() {
                            ui.label(msg);
                        }
                    });
            }
        });
    }

    fn render_csv_mode(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label("CSV File:");
            ui.text_edit_singleline(&mut self.csv_path);
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_directory("D:\\")
                    .pick_file()
                {
                    let p = path.to_string_lossy().to_string();
                    self.load_csv(&p);
                }
            }
            if ui.button("Load").clicked() && !self.csv_path.is_empty() {
                let p = self.csv_path.clone();
                self.load_csv(&p);
            }
        });

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Delay (ms):");
            ui.add(egui::TextEdit::singleline(&mut self.delay_ms).desired_width(80.0));
            ui.label("between each click");
        });

        ui.add_space(8.0);
        ui.separator();

        if !self.points.is_empty() {
            ui.label(format!("Loaded {} points:", self.points.len()));
            ui.add_space(4.0);

            let current = *self.current_index.lock().unwrap();
            let state = self.state.lock().unwrap().clone();

            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for (i, p) in self.points.iter().enumerate() {
                        let prefix = if state == AppState::Running && i == current {
                            ">> "
                        } else if state == AppState::Running && i < current {
                            "[done] "
                        } else {
                            "   "
                        };
                        let template_indicator = if p.template_path.is_some() {
                            " [T]"
                        } else {
                            ""
                        };
                        ui.label(format!(
                            "{}#{:03}  ({}, {}){}  {}",
                            prefix, p.id, p.x, p.y, template_indicator, p.description
                        ));
                    }
                });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        let state = self.state.lock().unwrap().clone();
        ui.horizontal(|ui| {
            match state {
                AppState::Idle => {
                    let can_start = !self.points.is_empty();
                    if ui
                        .add_enabled(can_start, egui::Button::new("Start (3s countdown)"))
                        .clicked()
                    {
                        self.start_clicking(ctx);
                    }
                }
                AppState::Running => {
                    if ui.button("Stop").clicked() {
                        *self.state.lock().unwrap() = AppState::Idle;
                        *self.window_visible.lock().unwrap() = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    }
                    ui.label("Running...");
                }
                AppState::Done => {
                    if ui.button("Reset").clicked() {
                        *self.state.lock().unwrap() = AppState::Idle;
                        self.log_messages.lock().unwrap().clear();
                        *self.current_index.lock().unwrap() = 0;
                    }
                    ui.label("Completed!");
                }
                AppState::Paused => {
                    ui.label("Paused - waiting for confirmation...");
                }
            }
        });
    }

    fn render_workflow_mode(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label("Element Folder:");
            ui.text_edit_singleline(&mut self.element_folder);
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory("D:\\")
                    .pick_folder()
                {
                    self.element_folder = path.to_string_lossy().to_string();
                    save_config(&AppConfig {
                        element_folder: self.element_folder.clone(),
                        workflow_path: self.workflow_path.clone(),
                    });
                }
            }
        });

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Workflow File:");
            ui.text_edit_singleline(&mut self.workflow_path);
            if ui.button("Browse...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Workflow", &["txt"])
                    .set_directory("D:\\")
                    .pick_file()
                {
                    let p = path.to_string_lossy().to_string();
                    self.load_workflow(&p);
                    save_config(&AppConfig {
                        element_folder: self.element_folder.clone(),
                        workflow_path: self.workflow_path.clone(),
                    });
                }
            }
            if ui.button("Load").clicked() && !self.workflow_path.is_empty() {
                let p = self.workflow_path.clone();
                self.load_workflow(&p);
                save_config(&AppConfig {
                    element_folder: self.element_folder.clone(),
                    workflow_path: self.workflow_path.clone(),
                });
            }
        });

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Delay (ms):");
            ui.add(egui::TextEdit::singleline(&mut self.delay_ms).desired_width(80.0));
            ui.label("between each step");
        });

        ui.add_space(8.0);
        ui.separator();

        if !self.workflow_steps.is_empty() {
            ui.label(format!(
                "Workflow Steps: ({} steps)",
                self.workflow_steps.len()
            ));
            ui.add_space(4.0);

            let current = *self.current_index.lock().unwrap();
            let state = self.state.lock().unwrap().clone();

            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for (i, step) in self.workflow_steps.iter().enumerate() {
                        let prefix = if state == AppState::Running && i == current {
                            "> "
                        } else if step.executed {
                            "v "
                        } else {
                            "  "
                        };

                        let step_desc = match &step.step_type {
                            StepType::Click {
                                element_name,
                                fallback_element,
                            } => {
                                let mut desc = format!("Click: {}", element_name);
                                if let Some(fb) = fallback_element {
                                    desc.push_str(&format!(" or {}", fb));
                                }
                                desc
                            }
                            StepType::Pause { message, .. } => format!("Pause: {}", message),
                            StepType::Manual { message, .. } => format!("Manual: {}", message),
                            StepType::Wait { seconds } => format!("Wait: {}s", seconds),
                        };

                        ui.label(format!("{}{}. {}", prefix, i + 1, step_desc));
                    }
                });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        let state = self.state.lock().unwrap().clone();
        ui.horizontal(|ui| {
            match state {
                AppState::Idle => {
                    let can_start =
                        !self.workflow_steps.is_empty() && !self.element_folder.is_empty();
                    if ui
                        .add_enabled(can_start, egui::Button::new("Start Workflow (3s)"))
                        .clicked()
                    {
                        self.start_workflow(ctx);
                    }
                    if !self.workflow_steps.is_empty() && self.element_folder.is_empty() {
                        ui.colored_label(egui::Color32::YELLOW, "Select Element Folder first");
                    }
                }
                AppState::Running => {
                    if ui.button("Stop").clicked() {
                        *self.state.lock().unwrap() = AppState::Idle;
                        *self.window_visible.lock().unwrap() = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    }
                    ui.label("Running...");
                }
                AppState::Paused => {
                    ui.label("Paused - waiting for confirmation");
                }
                AppState::Done => {
                    if ui.button("Reset").clicked() {
                        *self.state.lock().unwrap() = AppState::Idle;
                        self.log_messages.lock().unwrap().clear();
                        *self.current_index.lock().unwrap() = 0;
                        for step in &mut self.workflow_steps {
                            step.executed = false;
                        }
                    }
                    ui.label("Workflow Completed!");
                }
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
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

#[cfg(not(windows))]
fn screen_size() -> (i32, i32) {
    (1920, 1080)
}

#[cfg(windows)]
fn click_at(x: i32, y: i32) {
    let (screen_w, screen_h) = screen_size();

    let abs_x = (x as f64 / screen_w as f64 * 65535.0) as i32;
    let abs_y = (y as f64 / screen_h as f64 * 65535.0) as i32;

    let move_input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: abs_x,
                dy: abs_y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let down_input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: abs_x,
                dy: abs_y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let up_input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: abs_x,
                dy: abs_y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_LEFTUP | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        let inputs = [move_input, down_input, up_input];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(not(windows))]
fn click_at(x: i32, y: i32) {
    // Experimental non-Windows build: click injection is not implemented yet.
    let _ = (x, y);
}

fn try_execute_click(
    element_name: &str,
    db: &ElementDatabase,
    element_folder: &str,
    log: &Arc<Mutex<Vec<String>>>,
    log_file: &str,
    step_index: usize,
    delay: u64,
) -> bool {
    match db.load_element(element_name) {
        Ok(Some((element, states))) => {
            let (current_w, current_h) = screen_size();

            let primary_state = states.iter().find(|s| s.is_primary);

            let (click_x, click_y, _method) = if let Some(state) = primary_state {
                let img_filename = Path::new(&state.screenshot_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&state.screenshot_path);
                let template_path = format!("{}\\{}", element_folder, img_filename);

                if Path::new(&template_path).exists() {
                    let roi_x = element.bbox_x.max(0);
                    let roi_y = element.bbox_y.max(0);
                    let roi_width = element.bbox_width.min(current_w - roi_x);
                    let roi_height = element.bbox_height.min(current_h - roi_y);

                    match find_template_with_roi(
                        &template_path,
                        0.8,
                        roi_x,
                        roi_y,
                        roi_width,
                        roi_height,
                    ) {
                        Some((x, y)) => {
                            log_write(
                                log,
                                log_file,
                                format!(
                                    "Step {}: [ROI Match] '{}' at ({}, {}) - state: {}",
                                    step_index + 1,
                                    element_name,
                                    x,
                                    y,
                                    state.state_name
                                ),
                            );
                            (x, y, "roi_match")
                        }
                        None => {
                            log_write(
                                log,
                                log_file,
                                format!(
                                    "Step {}: ROI match failed, trying full-screen...",
                                    step_index + 1
                                ),
                            );
                            match find_template(&template_path, 0.8) {
                                Some((x, y)) => {
                                    log_write(
                                        log,
                                        log_file,
                                        format!(
                                            "Step {}: [Full-Screen Match] '{}' at ({}, {}) - state: {}",
                                            step_index + 1,
                                            element_name,
                                            x,
                                            y,
                                            state.state_name
                                        ),
                                    );
                                    (x, y, "fullscreen_match")
                                }
                                None => {
                                    let scaled_x = if element.screen_width > 0 {
                                        (element.center_x as f64 * current_w as f64
                                            / element.screen_width as f64)
                                            as i32
                                    } else {
                                        element.center_x
                                    };
                                    let scaled_y = if element.screen_height > 0 {
                                        (element.center_y as f64 * current_h as f64
                                            / element.screen_height as f64)
                                            as i32
                                    } else {
                                        element.center_y
                                    };
                                    log_write(
                                        log,
                                        log_file,
                                        format!(
                                            "Step {}: [Fallback DB coords] '{}' at ({}, {}) - all match failed",
                                            step_index + 1,
                                            element_name,
                                            scaled_x,
                                            scaled_y
                                        ),
                                    );
                                    (scaled_x, scaled_y, "db_fallback")
                                }
                            }
                        }
                    }
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
                            "Step {}: [DB coords] '{}' at ({}, {}) - template not found: {}",
                            step_index + 1,
                            element_name,
                            scaled_x,
                            scaled_y,
                            template_path
                        ),
                    );
                    (scaled_x, scaled_y, "db_coords_no_template")
                }
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
                        "Step {}: [DB coords] '{}' at ({}, {}) - no primary state",
                        step_index + 1,
                        element_name,
                        scaled_x,
                        scaled_y
                    ),
                );
                (scaled_x, scaled_y, "db_coords_no_state")
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

fn log_write(log: &Arc<Mutex<Vec<String>>>, log_file: &str, msg: String) {
    log.lock().unwrap().push(msg.clone());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_file) {
        let _ = writeln!(f, "{}", msg);
    }
}

fn capture_screen_rgba() -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let monitors = Monitor::all().ok()?;
    let mon = monitors.first()?;
    let img = mon.capture_image().ok()?;
    let w = img.width();
    let h = img.height();
    let raw: Vec<u8> = img
        .pixels()
        .flat_map(|p| [p[0], p[1], p[2], 255u8])
        .collect();
    ImageBuffer::from_raw(w, h, raw)
}

fn find_template(template_path: &str, threshold: f32) -> Option<(i32, i32)> {
    let template = match image::open(template_path) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return None,
    };

    let screen_img = capture_screen_rgba()?;

    let (t_width, t_height) = template.dimensions();
    let (s_width, s_height) = screen_img.dimensions();

    if t_width > s_width || t_height > s_height {
        return None;
    }

    let mut best_score = f32::MIN;
    let mut best_pos = None;

    for y in 0..=(s_height - t_height) {
        for x in 0..=(s_width - t_width) {
            let score = calculate_match_score(&screen_img, &template, x, y);
            if score > best_score {
                best_score = score;
                best_pos = Some((
                    x as i32 + (t_width / 2) as i32,
                    y as i32 + (t_height / 2) as i32,
                ));
            }
        }
    }

    if best_score >= threshold {
        best_pos
    } else {
        None
    }
}

fn find_template_with_roi(
    template_path: &str,
    threshold: f32,
    roi_x: i32,
    roi_y: i32,
    roi_width: i32,
    roi_height: i32,
) -> Option<(i32, i32)> {
    let template = match image::open(template_path) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return None,
    };

    let screen_img = capture_screen_rgba()?;

    let (t_width, t_height) = template.dimensions();

    let search_x_start = roi_x.max(0) as u32;
    let search_y_start = roi_y.max(0) as u32;
    let search_x_end = (roi_x + roi_width).min(screen_img.width() as i32) as u32;
    let search_y_end = (roi_y + roi_height).min(screen_img.height() as i32) as u32;

    if search_x_end <= search_x_start || search_y_end <= search_y_start {
        return None;
    }

    if t_width > (search_x_end - search_x_start) || t_height > (search_y_end - search_y_start) {
        return None;
    }

    let mut best_score = f32::MIN;
    let mut best_pos = None;

    for y in search_y_start..=(search_y_end - t_height) {
        for x in search_x_start..=(search_x_end - t_width) {
            let score = calculate_match_score(&screen_img, &template, x, y);
            if score > best_score {
                best_score = score;
                best_pos = Some((
                    x as i32 + (t_width / 2) as i32,
                    y as i32 + (t_height / 2) as i32,
                ));
            }
        }
    }

    if best_score >= threshold {
        best_pos
    } else {
        None
    }
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
