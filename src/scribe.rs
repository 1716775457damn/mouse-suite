//! 操作文档录制（clickscribe 风格）：点击 → 截所在屏 → 编辑说明 → 导出。

use crate::common::data_dir;
use crate::scribe_ai::{self, AiConfig, AiProvider};
use crate::theme;
use crate::workflow::{StepType, WorkflowStep};
use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle};
use image::{Rgba, RgbaImage};
use rdev::{listen, Button, EventType};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSIONS_SUBDIR: &str = "scribe_sessions";
const RECORDING_SUBDIR: &str = "_recording";
const THUMB_MAX: u32 = 960;

#[derive(Clone, Serialize, Deserialize)]
pub struct ScribeStep {
    pub x: i32,
    pub y: i32,
    pub screenshot: String,
    pub ts: f64,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub img_w: u32,
    #[serde(default)]
    pub img_h: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ScribeSession {
    pub id: String,
    pub title: String,
    pub created: f64,
    pub steps: Vec<ScribeStep>,
}

/// Shared outer window rect (physical px) for ignoring clicks on this app.
type IgnoreRect = Arc<Mutex<Option<(i32, i32, i32, i32)>>>;

pub struct ScribeApp {
    sessions: Vec<String>,
    active_id: Option<String>,
    session: Option<ScribeSession>,
    selected_step: usize,
    recording: bool,
    record_flag: Arc<AtomicBool>,
    step_counter: Arc<AtomicUsize>,
    step_rx: Receiver<Result<ScribeStep, String>>,
    buffer: Arc<Mutex<Vec<ScribeStep>>>,
    ignore_rect: IgnoreRect,
    status: String,
    textures: Vec<Option<TextureHandle>>,
    minimize_on_record: bool,
    ai_cfg: AiConfig,
    show_ai_settings: bool,
    ai_busy: Arc<AtomicBool>,
    ai_job: Option<Receiver<Result<Vec<String>, String>>>,
    pending_flow: Option<Vec<WorkflowStep>>,
    rename_buf: String,
}

impl ScribeApp {
    /// Reload AI config from disk (after Agent `ai_set_config`).
    pub fn reload_ai_config(&mut self) {
        self.ai_cfg = AiConfig::load();
    }

    pub fn new() -> Self {
        let flag = Arc::new(AtomicBool::new(false));
        let counter = Arc::new(AtomicUsize::new(0));
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let ignore = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel();
        start_click_listener(
            flag.clone(),
            counter.clone(),
            buffer.clone(),
            ignore.clone(),
            tx,
        );

        let mut app = Self {
            sessions: Vec::new(),
            active_id: None,
            session: None,
            selected_step: 0,
            recording: false,
            record_flag: flag,
            step_counter: counter,
            step_rx: rx,
            buffer,
            ignore_rect: ignore,
            status: "F8 开始/停止录制 · 每次左键自动截点击所在屏。".into(),
            textures: Vec::new(),
            minimize_on_record: true,
            ai_cfg: AiConfig::load(),
            show_ai_settings: false,
            ai_busy: Arc::new(AtomicBool::new(false)),
            ai_job: None,
            pending_flow: None,
            rename_buf: String::new(),
        };
        app.refresh_sessions();
        app
    }

    fn root_dir() -> PathBuf {
        data_dir().join(SESSIONS_SUBDIR)
    }

    fn recording_dir() -> PathBuf {
        Self::root_dir().join(RECORDING_SUBDIR)
    }

    fn session_json(id: &str) -> PathBuf {
        Self::root_dir().join(format!("{id}.json"))
    }

    fn session_images(id: &str) -> PathBuf {
        Self::root_dir().join(id).join("images")
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn take_pending_flow(&mut self) -> Option<Vec<WorkflowStep>> {
        self.pending_flow.take()
    }

    /// Update ignore region; cleared while minimized recording so clicks are free.
    pub fn sync_ignore_rect(&self, ctx: &egui::Context) {
        let clear = self.recording && self.minimize_on_record;
        if clear {
            if let Ok(mut g) = self.ignore_rect.lock() {
                *g = None;
            }
            return;
        }
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            // egui points ≈ physical pixels when process is DPI-aware
            let pad = 2;
            let r = (
                rect.left() as i32 - pad,
                rect.top() as i32 - pad,
                rect.right() as i32 + pad,
                rect.bottom() as i32 + pad,
            );
            if let Ok(mut g) = self.ignore_rect.lock() {
                *g = Some(r);
            }
        }
    }

    pub fn toggle_recording(&mut self, ctx: &egui::Context) {
        if self.recording {
            self.stop_recording(ctx);
        } else {
            self.start_recording(ctx);
        }
    }

    pub fn refresh_sessions(&mut self) {
        let root = Self::root_dir();
        let _ = fs::create_dir_all(&root);
        let mut ids: Vec<String> = fs::read_dir(&root)
            .ok()
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let p = e.path();
                        if p.extension().and_then(|s| s.to_str()) == Some("json") {
                            p.file_stem()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids.reverse();
        self.sessions = ids;
    }

    fn load_session(&mut self, id: &str, ctx: &egui::Context) {
        let path = Self::session_json(id);
        match fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<ScribeSession>(&s).ok())
        {
            Some(sess) => {
                self.active_id = Some(id.to_string());
                self.rename_buf = sess.title.clone();
                self.session = Some(sess);
                self.selected_step = 0;
                self.reload_textures(ctx);
                self.status = format!("已打开会话 {id}");
            }
            None => {
                self.status = format!("无法加载会话 {id}");
            }
        }
    }

    fn save_active(&mut self) {
        let Some(sess) = self.session.as_ref() else {
            return;
        };
        let path = Self::session_json(&sess.id);
        if let Ok(json) = serde_json::to_string_pretty(sess) {
            let _ = fs::write(path, json);
            self.status = "已保存".into();
        }
    }

    fn delete_session(&mut self, id: &str) {
        let _ = fs::remove_file(Self::session_json(id));
        let _ = fs::remove_dir_all(Self::root_dir().join(id));
        if self.active_id.as_deref() == Some(id) {
            self.active_id = None;
            self.session = None;
            self.textures.clear();
        }
        self.refresh_sessions();
        self.status = format!("已删除 {id}");
    }

    fn duplicate_session(&mut self, id: &str, ctx: &egui::Context) {
        let Ok(raw) = fs::read_to_string(Self::session_json(id)) else {
            self.status = "复制失败：无法读取".into();
            return;
        };
        let Ok(mut sess) = serde_json::from_str::<ScribeSession>(&raw) else {
            self.status = "复制失败：JSON 损坏".into();
            return;
        };
        let new_id = format!("{}_copy_{}", id, chrono::Local::now().format("%H%M%S"));
        let src_img = Self::session_images(id);
        let dst_img = Self::session_images(&new_id);
        let _ = fs::create_dir_all(&dst_img);
        if src_img.is_dir() {
            if let Ok(rd) = fs::read_dir(&src_img) {
                for e in rd.flatten() {
                    let to = dst_img.join(e.file_name());
                    let _ = fs::copy(e.path(), to);
                }
            }
        }
        sess.id = new_id.clone();
        sess.title = format!("{}（副本）", sess.title);
        sess.created = now_ts();
        if let Ok(json) = serde_json::to_string_pretty(&sess) {
            let _ = fs::write(Self::session_json(&new_id), json);
        }
        self.refresh_sessions();
        self.load_session(&new_id, ctx);
        self.status = format!("已复制为 {new_id}");
    }

    fn apply_rename_title(&mut self) {
        let title = self.rename_buf.trim().to_string();
        if title.is_empty() {
            return;
        }
        if let Some(sess) = self.session.as_mut() {
            sess.title = title;
            self.save_active();
        }
    }

    fn reload_textures(&mut self, ctx: &egui::Context) {
        self.textures.clear();
        let Some(sess) = self.session.as_ref() else {
            return;
        };
        let img_dir = Self::session_images(&sess.id);
        for step in &sess.steps {
            let path = img_dir.join(&step.screenshot);
            self.textures
                .push(load_texture_thumb(ctx, &path, &step.screenshot));
        }
    }

    fn request_ai_descriptions(&mut self) {
        if self.ai_busy.load(Ordering::Relaxed) {
            return;
        }
        let Some(sess) = self.session.as_ref() else {
            self.status = "请先打开会话".into();
            return;
        };
        if sess.steps.is_empty() {
            self.status = "没有步骤可生成".into();
            return;
        }
        let img_dir = Self::session_images(&sess.id);
        let paths: Vec<PathBuf> = sess
            .steps
            .iter()
            .map(|s| img_dir.join(&s.screenshot))
            .collect();
        let cfg = self.ai_cfg.clone();
        let (tx, rx) = mpsc::channel();
        self.ai_job = Some(rx);
        self.ai_busy.store(true, Ordering::Relaxed);
        self.status = "AI 生成中…".into();
        let busy = self.ai_busy.clone();
        thread::spawn(move || {
            let r = scribe_ai::describe_all(&paths, &cfg);
            let _ = tx.send(r);
            busy.store(false, Ordering::Relaxed);
        });
    }

    fn drain_ai(&mut self) {
        let Some(rx) = self.ai_job.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(descs)) => {
                if let Some(sess) = self.session.as_mut() {
                    for (i, d) in descs.into_iter().enumerate() {
                        if i >= sess.steps.len() {
                            break;
                        }
                        let t = d.trim();
                        if !t.is_empty() {
                            sess.steps[i].description = t.to_string();
                            if sess.steps[i].title.is_empty() {
                                let short: String = t.chars().take(16).collect();
                                sess.steps[i].title = short;
                            }
                        }
                    }
                }
                self.ai_job = None;
                self.save_active();
                self.status = "AI 说明已填入并保存".into();
            }
            Ok(Err(e)) => {
                self.ai_job = None;
                self.status = format!("AI 失败: {e}");
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.ai_job = None;
                self.ai_busy.store(false, Ordering::Relaxed);
            }
        }
    }

    pub fn build_flow_draft(&mut self) {
        let Some(sess) = self.session.as_ref() else {
            self.status = "请先打开会话".into();
            return;
        };
        let mut steps = Vec::new();
        for (i, st) in sess.steps.iter().enumerate() {
            let msg = if st.title.is_empty() {
                format!("第 {} 步", i + 1)
            } else {
                st.title.clone()
            };
            let instruction = if st.description.trim().is_empty() {
                None
            } else {
                Some(st.description.clone())
            };
            steps.push(WorkflowStep::new(StepType::Manual {
                message: msg,
                instruction,
            }));
        }
        if steps.is_empty() {
            self.status = "没有步骤可生成流程图".into();
            return;
        }
        self.pending_flow = Some(steps);
        self.status = "已生成流程图草稿".into();
    }

    pub fn agent_start(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.recording {
            return Err("already recording".into());
        }
        self.start_recording(ctx);
        Ok(())
    }

    pub fn agent_stop(&mut self, ctx: &egui::Context) -> Result<usize, String> {
        if !self.recording {
            return Err("not recording".into());
        }
        self.stop_recording(ctx);
        Ok(self.session.as_ref().map(|s| s.steps.len()).unwrap_or(0))
    }

    pub fn agent_export_html(&self, path: &str) -> Result<(), String> {
        let sess = self
            .session
            .as_ref()
            .ok_or_else(|| "no active session".to_string())?;
        export_html(sess, Path::new(path))
    }

    pub fn agent_session_id(&self) -> Option<String> {
        self.active_id.clone()
    }

    pub fn start_recording(&mut self, ctx: &egui::Context) {
        if self.recording {
            return;
        }
        let dir = Self::recording_dir();
        let _ = fs::create_dir_all(&dir);
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = fs::remove_file(e.path());
            }
        }
        if let Ok(mut b) = self.buffer.lock() {
            b.clear();
        }
        self.step_counter.store(0, Ordering::Relaxed);
        while self.step_rx.try_recv().is_ok() {}

        self.recording = true;
        self.record_flag.store(true, Ordering::Relaxed);
        self.active_id = None;
        self.session = None;
        self.textures.clear();
        self.status = "录制中… 在其他窗口点击即可。再点「停止录制」结束。".into();

        if self.minimize_on_record {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        ctx.request_repaint();
    }

    pub fn stop_recording(&mut self, ctx: &egui::Context) {
        if !self.recording {
            return;
        }
        self.record_flag.store(false, Ordering::Relaxed);
        self.recording = false;
        thread::sleep(std::time::Duration::from_millis(150));
        self.drain_captures(ctx);

        let steps = self
            .buffer
            .lock()
            .map(|b| b.clone())
            .unwrap_or_default();

        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

        if steps.is_empty() {
            self.status = "未捕获到点击，会话未创建。".into();
            return;
        }

        match archive_session(steps) {
            Ok(id) => {
                self.refresh_sessions();
                self.load_session(&id, ctx);
                let n = self
                    .session
                    .as_ref()
                    .map(|s| s.steps.len())
                    .unwrap_or(0);
                self.status = format!("录制完成，共 {n} 步");
            }
            Err(e) => {
                self.status = format!("归档失败: {e}");
            }
        }
    }

    fn drain_captures(&mut self, ctx: &egui::Context) {
        loop {
            match self.step_rx.try_recv() {
                Ok(Ok(step)) => {
                    let n = self.buffer.lock().map(|b| b.len()).unwrap_or(0);
                    self.status = format!("已记录第 {n} 步 · {}", step.screenshot);
                    ctx.request_repaint();
                }
                Ok(Err(e)) => {
                    self.status = format!("截图失败: {e}");
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn export_markdown(&self) -> Result<PathBuf, String> {
        let sess = self
            .session
            .as_ref()
            .ok_or_else(|| "无活动会话".to_string())?;
        let out = rfd::FileDialog::new()
            .set_title("选择 Markdown 导出目录")
            .pick_folder()
            .ok_or_else(|| "已取消".to_string())?;
        export_markdown(sess, &out)
    }

    fn export_html(&self) -> Result<PathBuf, String> {
        let sess = self
            .session
            .as_ref()
            .ok_or_else(|| "无活动会话".to_string())?;
        let path = rfd::FileDialog::new()
            .set_title("保存 HTML")
            .set_file_name(format!("{}.html", sess.id))
            .add_filter("HTML", &["html"])
            .save_file()
            .ok_or_else(|| "已取消".to_string())?;
        export_html(sess, &path)?;
        Ok(path)
    }

    fn export_json(&self) -> Result<PathBuf, String> {
        let sess = self
            .session
            .as_ref()
            .ok_or_else(|| "无活动会话".to_string())?;
        let path = rfd::FileDialog::new()
            .set_title("保存 JSON")
            .set_file_name(format!("{}.json", sess.id))
            .add_filter("JSON", &["json"])
            .save_file()
            .ok_or_else(|| "已取消".to_string())?;
        let json = serde_json::to_string_pretty(sess).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.sync_ignore_rect(ctx);
        self.drain_captures(ctx);
        self.drain_ai();
        if self.recording || self.ai_busy.load(Ordering::Relaxed) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::colors::BG))
            .show(ctx, |ui| {
                theme::paint_atmosphere(ui);

                let pad = 16.0;
                let gap = 12.0;
                let full = ui.available_size();
                ui.allocate_ui_with_layout(
                    full,
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.add_space(pad);
                        ui.horizontal(|ui| {
                            ui.add_space(pad);
                            ui.vertical(|ui| {
                                ui.set_width((full.x - pad * 2.0).max(200.0));

                                // —— Header ——
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new("操作文档")
                                                .size(24.0)
                                                .strong()
                                                .color(theme::colors::TEXT),
                                        );
                                        ui.label(
                                            RichText::new("F8 录制 · 编辑说明 · 导出指南")
                                                .size(13.0)
                                                .color(theme::colors::MUTED),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if self.recording {
                                                theme::status_pill(
                                                    ui,
                                                    &format!(
                                                        "录制中 · {} 步",
                                                        self.buffer
                                                            .lock()
                                                            .map(|b| b.len())
                                                            .unwrap_or(0)
                                                    ),
                                                    theme::StatusTone::Danger,
                                                );
                                            } else if self.session.is_some() {
                                                theme::status_pill(
                                                    ui,
                                                    "已打开会话",
                                                    theme::StatusTone::Idle,
                                                );
                                            }
                                        },
                                    );
                                });

                                ui.add_space(12.0);

                                // —— Toolbar ——
                                theme::panel_frame().show(ui, |ui| {
                                    theme::toolbar_row(ui, |ui| {
                                        if self.recording {
                                            if theme::danger_button(ui, "停止录制").clicked() {
                                                self.stop_recording(ctx);
                                            }
                                        } else if theme::primary_button(ui, "开始录制").clicked()
                                        {
                                            self.start_recording(ctx);
                                        }

                                        ui.add_space(10.0);
                                        ui.checkbox(&mut self.minimize_on_record, "录制时最小化");
                                        ui.label(
                                            RichText::new("F8")
                                                .size(12.0)
                                                .color(theme::colors::FAINT),
                                        );

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let can = self.session.is_some() && !self.recording;
                                                let ai_busy = self.ai_busy.load(Ordering::Relaxed);
                                                if theme::quiet_button(ui, "AI 设置").clicked() {
                                                    self.show_ai_settings = !self.show_ai_settings;
                                                }
                                                if ui
                                                    .add_enabled(
                                                        can && !ai_busy,
                                                        egui::Button::new(if ai_busy {
                                                            "AI 生成中…"
                                                        } else {
                                                            "AI 写说明"
                                                        })
                                                        .min_size(egui::vec2(0.0, theme::CTRL_H)),
                                                    )
                                                    .clicked()
                                                {
                                                    self.request_ai_descriptions();
                                                }
                                                if ui
                                                    .add_enabled(
                                                        can,
                                                        egui::Button::new("生成流程图")
                                                            .min_size(egui::vec2(0.0, theme::CTRL_H)),
                                                    )
                                                    .clicked()
                                                {
                                                    self.build_flow_draft();
                                                }
                                                if ui
                                                    .add_enabled(
                                                        can,
                                                        egui::Button::new("JSON")
                                                            .min_size(egui::vec2(0.0, theme::CTRL_H)),
                                                    )
                                                    .clicked()
                                                {
                                                    match self.export_json() {
                                                        Ok(p) => {
                                                            self.status =
                                                                format!("已导出 {}", p.display())
                                                        }
                                                        Err(e) => self.status = e,
                                                    }
                                                }
                                                if ui
                                                    .add_enabled(
                                                        can,
                                                        egui::Button::new("Markdown")
                                                            .min_size(egui::vec2(0.0, theme::CTRL_H)),
                                                    )
                                                    .clicked()
                                                {
                                                    match self.export_markdown() {
                                                        Ok(p) => {
                                                            self.status =
                                                                format!("已导出 {}", p.display())
                                                        }
                                                        Err(e) => self.status = e,
                                                    }
                                                }
                                                if ui
                                                    .add_enabled(
                                                        can,
                                                        egui::Button::new("HTML")
                                                            .min_size(egui::vec2(0.0, theme::CTRL_H)),
                                                    )
                                                    .clicked()
                                                {
                                                    match self.export_html() {
                                                        Ok(p) => {
                                                            self.status =
                                                                format!("已导出 {}", p.display())
                                                        }
                                                        Err(e) => self.status = e,
                                                    }
                                                }
                                                ui.add_space(4.0);
                                                if ui
                                                    .add_enabled(
                                                        can,
                                                        egui::Button::new(
                                                            RichText::new("保存")
                                                                .color(Color32::WHITE)
                                                                .strong(),
                                                        )
                                                        .fill(theme::colors::ACCENT)
                                                        .min_size(egui::vec2(64.0, theme::CTRL_H)),
                                                    )
                                                    .clicked()
                                                {
                                                    self.save_active();
                                                }
                                            },
                                        );
                                    });

                                    if self.show_ai_settings {
                                        ui.add_space(8.0);
                                        theme::hairline(ui);
                                        theme::field_label(ui, "AI 接入");
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.selectable_value(
                                                &mut self.ai_cfg.provider,
                                                AiProvider::Ccswitch,
                                                "CC Switch 跟随代理",
                                            );
                                            ui.selectable_value(
                                                &mut self.ai_cfg.provider,
                                                AiProvider::Glm,
                                                "智谱 GLM 直连",
                                            );
                                            ui.selectable_value(
                                                &mut self.ai_cfg.provider,
                                                AiProvider::Custom,
                                                "自定义 API",
                                            );
                                        });
                                        match self.ai_cfg.provider {
                                            AiProvider::Glm => {
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.ai_cfg.glm_key,
                                                    )
                                                    .desired_width(f32::INFINITY)
                                                    .hint_text("智谱 API Key")
                                                    .password(true),
                                                );
                                            }
                                            AiProvider::Custom => {
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.ai_cfg.custom_base,
                                                    )
                                                    .desired_width(f32::INFINITY)
                                                    .hint_text("OpenAI 兼容 Base URL"),
                                                );
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.ai_cfg.custom_key,
                                                    )
                                                    .desired_width(f32::INFINITY)
                                                    .hint_text("API Key（可选）")
                                                    .password(true),
                                                );
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.ai_cfg.custom_model,
                                                    )
                                                    .desired_width(f32::INFINITY)
                                                    .hint_text("模型名，如 gpt-4o"),
                                                );
                                            }
                                            AiProvider::Ccswitch => {
                                                ui.label(
                                                    RichText::new(
                                                        "默认免配置：http://127.0.0.1:15721/v1/messages\n\
                                                         （CC Switch 本地代理；顶部 Proxy 需为绿色）",
                                                    )
                                                    .size(12.0)
                                                    .color(theme::colors::MUTED),
                                                );
                                            }
                                        }
                                        if theme::secondary_button(ui, "保存 AI 设置").clicked() {
                                            match self.ai_cfg.save() {
                                                Ok(()) => self.status = "AI 设置已保存".into(),
                                                Err(e) => self.status = e,
                                            }
                                        }
                                    }

                                    if !self.status.is_empty() {
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new(&self.status)
                                                .size(12.0)
                                                .color(theme::colors::MUTED),
                                        );
                                    }
                                });

                                ui.add_space(gap);

                                // —— Body columns ——
                                let body_h = (ui.available_height() - pad).max(280.0);
                                let body_w = ui.available_width();
                                let side_w = 200.0_f32;
                                let main_w = (body_w - side_w - gap).max(320.0);

                                ui.allocate_ui_with_layout(
                                    egui::vec2(body_w, body_h),
                                    egui::Layout::left_to_right(egui::Align::Min),
                                    |ui| {
                                        // Session sidebar
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(side_w, body_h),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                theme::panel_frame().show(ui, |ui| {
                                                    ui.set_min_height(body_h - 4.0);
                                                    ui.set_width(side_w - 4.0);

                                                    theme::toolbar_row(ui, |ui| {
                                                        ui.label(
                                                            RichText::new("会话")
                                                                .size(14.0)
                                                                .strong()
                                                                .color(theme::colors::TEXT),
                                                        );
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                if theme::quiet_button(ui, "刷新")
                                                                    .clicked()
                                                                {
                                                                    self.refresh_sessions();
                                                                }
                                                            },
                                                        );
                                                    });
                                                    ui.add_space(6.0);

                                                    egui::ScrollArea::vertical()
                                                        .id_salt("scribe_sessions")
                                                        .max_height(body_h - 70.0)
                                                        .show(ui, |ui| {
                                                            ui.set_width(ui.available_width());
                                                            let ids = self.sessions.clone();
                                                            if ids.is_empty() {
                                                                ui.add_space(24.0);
                                                                ui.vertical_centered(|ui| {
                                                                    ui.label(
                                                                        RichText::new("暂无会话")
                                                                            .size(12.0)
                                                                            .color(
                                                                                theme::colors::FAINT,
                                                                            ),
                                                                    );
                                                                });
                                                            }
                                                            for id in ids {
                                                                let selected = self
                                                                    .active_id
                                                                    .as_deref()
                                                                    == Some(id.as_str());
                                                                let resp =
                                                                    theme::list_row(ui, selected, &id);
                                                                if resp.clicked() && !self.recording
                                                                {
                                                                    self.load_session(&id, ctx);
                                                                }
                                                                    resp.context_menu(|ui| {
                                                                        if ui.button("复制会话").clicked()
                                                                        {
                                                                            self.duplicate_session(
                                                                                &id, ctx,
                                                                            );
                                                                            ui.close_menu();
                                                                        }
                                                                        if ui.button("删除会话").clicked()
                                                                        {
                                                                            self.delete_session(&id);
                                                                            ui.close_menu();
                                                                        }
                                                                    });
                                                                ui.add_space(2.0);
                                                            }
                                                        });
                                                });
                                            },
                                        );

                                        ui.add_space(gap);

                                        // Editor
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(main_w, body_h),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                theme::panel_frame().show(ui, |ui| {
                                                    ui.set_min_height(body_h - 4.0);
                                                    ui.set_width(main_w - 4.0);
                                                    self.ui_editor(ui, ctx, body_h);
                                                });
                                            },
                                        );
                                    },
                                );
                            });
                        });
                    },
                );
            });
    }

    fn ui_editor(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, body_h: f32) {
        if self.recording {
            ui.vertical_centered(|ui| {
                ui.add_space((body_h * 0.28).clamp(40.0, 120.0));
                ui.label(
                    RichText::new("正在录制")
                        .size(22.0)
                        .strong()
                        .color(theme::colors::DANGER),
                );
                ui.add_space(6.0);
                let n = self.buffer.lock().map(|b| b.len()).unwrap_or(0);
                ui.label(
                    RichText::new(format!("已捕获 {n} 步 · Ctrl+Alt+F10 停止"))
                        .size(13.0)
                        .color(theme::colors::MUTED),
                );
            });
            return;
        }

        let Some(_) = self.session.as_ref() else {
            ui.vertical_centered(|ui| {
                ui.add_space((body_h * 0.28).clamp(40.0, 120.0));
                ui.label(
                    RichText::new("开始录制，或从左侧打开会话")
                        .size(15.0)
                        .color(theme::colors::MUTED),
                );
            });
            return;
        };

        // Title row
        {
            theme::field_label(ui, "文档标题");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_buf)
                        .desired_width((ui.available_width() - 80.0).max(120.0))
                        .margin(egui::Margin::symmetric(10.0, 8.0)),
                );
                if theme::secondary_button(ui, "应用").clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    self.apply_rename_title();
                }
            });
            if let Some(sess) = self.session.as_mut() {
                // keep edit buffer in sync when typing into rename applies to session only on 应用
                let _ = &mut sess.title;
            }
        }

        ui.add_space(12.0);

        let n_steps = self.session.as_ref().map(|s| s.steps.len()).unwrap_or(0);
        if n_steps == 0 {
            ui.label(
                RichText::new("此会话没有步骤。")
                    .color(theme::colors::MUTED),
            );
            return;
        }
        if self.selected_step >= n_steps {
            self.selected_step = n_steps.saturating_sub(1);
        }

        let step_list_w = 148.0;
        let rest_w = (ui.available_width() - step_list_w - 12.0).max(240.0);
        let content_h = (ui.available_height() - 8.0).max(200.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_h),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                // Step list
                ui.allocate_ui_with_layout(
                    egui::vec2(step_list_w, content_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        theme::field_label(ui, "步骤");
                        ui.add_space(6.0);
                        theme::inset_frame().show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("scribe_steps")
                                .max_height(content_h - 40.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let labels: Vec<String> = self
                                        .session
                                        .as_ref()
                                        .map(|s| {
                                            s.steps
                                                .iter()
                                                .enumerate()
                                                .map(|(i, st)| {
                                                    if st.title.is_empty() {
                                                        format!("步骤 {}", i + 1)
                                                    } else {
                                                        format!("{}. {}", i + 1, st.title)
                                                    }
                                                })
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    for (i, label) in labels.iter().enumerate() {
                                        if theme::list_row(ui, self.selected_step == i, label)
                                            .clicked()
                                        {
                                            self.selected_step = i;
                                        }
                                        ui.add_space(2.0);
                                    }
                                });
                        });
                    },
                );

                ui.add_space(12.0);

                // Step detail
                ui.allocate_ui_with_layout(
                    egui::vec2(rest_w, content_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let i = self.selected_step;
                        theme::toolbar_row(ui, |ui| {
                            ui.label(
                                RichText::new(format!("步骤 {}", i + 1))
                                    .size(15.0)
                                    .strong()
                                    .color(theme::colors::TEXT),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::quiet_button(ui, "删除此步").clicked() {
                                        if let Some(sess) = self.session.as_mut() {
                                            if i < sess.steps.len() {
                                                sess.steps.remove(i);
                                            }
                                        }
                                        if i < self.textures.len() {
                                            self.textures.remove(i);
                                        }
                                        if self.selected_step > 0
                                            && self
                                                .session
                                                .as_ref()
                                                .map(|s| self.selected_step >= s.steps.len())
                                                .unwrap_or(false)
                                        {
                                            self.selected_step -= 1;
                                        }
                                    }
                                },
                            );
                        });

                        ui.add_space(8.0);

                        let valid = self
                            .session
                            .as_ref()
                            .map(|s| i < s.steps.len())
                            .unwrap_or(false);
                        if !valid {
                            return;
                        }

                        theme::field_label(ui, "步骤标题");
                        ui.add_space(3.0);
                        if let Some(sess) = self.session.as_mut() {
                            ui.add(
                                egui::TextEdit::singleline(&mut sess.steps[i].title)
                                    .desired_width(ui.available_width())
                                    .hint_text("例如：打开设置"),
                            );
                        }

                        ui.add_space(8.0);
                        theme::field_label(ui, "说明");
                        ui.add_space(3.0);
                        if let Some(sess) = self.session.as_mut() {
                            ui.add(
                                egui::TextEdit::multiline(&mut sess.steps[i].description)
                                    .desired_width(ui.available_width())
                                    .desired_rows(3)
                                    .hint_text("这一步在做什么…"),
                            );
                        }

                        ui.add_space(10.0);

                        let (sx, sy) = self
                            .session
                            .as_ref()
                            .map(|s| (s.steps[i].x, s.steps[i].y))
                            .unwrap_or((0, 0));

                        if let Some(Some(tex)) = self.textures.get(i) {
                            let max_w = (ui.available_width() - 4.0).clamp(180.0, 720.0);
                            let max_h = (ui.available_height() - 8.0).max(120.0);
                            let size = tex.size_vec2();
                            let scale = (max_w / size.x).min(max_h / size.y).min(1.0);
                            let disp = size * scale;
                            let resp = ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(disp)
                                    .rounding(10.0),
                            );
                            // Soft frame around preview
                            ui.painter().rect_stroke(
                                resp.rect.expand(1.0),
                                10.0,
                                egui::Stroke::new(1.0, theme::colors::PANEL_EDGE),
                            );
                            let ox = resp.rect.min.x + sx as f32 / size.x.max(1.0) * disp.x;
                            let oy = resp.rect.min.y + sy as f32 / size.y.max(1.0) * disp.y;
                            ui.painter().circle_filled(
                                egui::pos2(ox, oy),
                                16.0,
                                Color32::from_rgba_unmultiplied(255, 170, 90, 55),
                            );
                            ui.painter().circle_stroke(
                                egui::pos2(ox, oy),
                                16.0,
                                egui::Stroke::new(3.0, Color32::from_rgb(220, 95, 0)),
                            );
                        } else {
                            theme::inset_frame().show(ui, |ui| {
                                ui.set_min_size(egui::vec2(ui.available_width(), 80.0));
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new("无预览图")
                                            .color(theme::colors::FAINT),
                                    );
                                });
                            });
                        }

                        let _ = ctx;
                    },
                );
            },
        );
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn start_click_listener(
    flag: Arc<AtomicBool>,
    counter: Arc<AtomicUsize>,
    buffer: Arc<Mutex<Vec<ScribeStep>>>,
    ignore: IgnoreRect,
    tx: Sender<Result<ScribeStep, String>>,
) {
    thread::spawn(move || {
        let last = Arc::new(Mutex::new((0.0_f64, 0.0_f64)));
        let last_cb = Arc::clone(&last);
        let result = listen(move |event| match event.event_type {
            EventType::MouseMove { x, y } => {
                if let Ok(mut p) = last_cb.lock() {
                    *p = (x, y);
                }
            }
            EventType::ButtonPress(Button::Left) => {
                if !flag.load(Ordering::Relaxed) {
                    return;
                }
                let (x, y) = last.lock().map(|p| (p.0 as i32, p.1 as i32)).unwrap_or((0, 0));
                if let Ok(g) = ignore.lock() {
                    if let Some((l, t, r, b)) = *g {
                        if x >= l && x < r && y >= t && y < b {
                            return; // click on Mouse Suite window
                        }
                    }
                }
                let flag2 = Arc::clone(&flag);
                let counter2 = Arc::clone(&counter);
                let buffer2 = Arc::clone(&buffer);
                let tx2 = tx.clone();
                thread::spawn(move || {
                    thread::sleep(std::time::Duration::from_millis(60));
                    if !flag2.load(Ordering::Relaxed) {
                        return;
                    }
                    let dir = ScribeApp::recording_dir();
                    let _ = fs::create_dir_all(&dir);
                    let idx = counter2.fetch_add(1, Ordering::Relaxed) + 1;
                    let fname = format!(
                        "img_{:03}_{}.png",
                        idx,
                        chrono::Local::now().format("%H%M%S")
                    );
                    let path = dir.join(&fname);
                    match capture_at(x, y, &path) {
                        Ok((px, py, scale, iw, ih)) => {
                            let step = ScribeStep {
                                x: px,
                                y: py,
                                screenshot: fname,
                                ts: now_ts(),
                                scale,
                                img_w: iw,
                                img_h: ih,
                                title: String::new(),
                                description: String::new(),
                            };
                            if let Ok(mut b) = buffer2.lock() {
                                b.push(step.clone());
                            }
                            let _ = tx2.send(Ok(step));
                        }
                        Err(e) => {
                            let _ = tx2.send(Err(e));
                        }
                    }
                });
            }
            _ => {}
        });
        if let Err(e) = result {
            eprintln!("[scribe] rdev listen error: {e:?}");
        }
    });
}

fn capture_at(x: i32, y: i32, path: &Path) -> Result<(i32, i32, f64, u32, u32), String> {
    let cap = crate::screen::capture_at_point(x, y)?;
    let iw = cap.width;
    let ih = cap.height;
    let px = (x - cap.x).clamp(0, iw.saturating_sub(1) as i32);
    let py = (y - cap.y).clamp(0, ih.saturating_sub(1) as i32);
    cap.image.save(path).map_err(|e| format!("save: {e}"))?;
    Ok((px, py, 1.0, iw, ih))
}

fn archive_session(steps: Vec<ScribeStep>) -> Result<String, String> {
    let id = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let img_dir = ScribeApp::session_images(&id);
    fs::create_dir_all(&img_dir).map_err(|e| e.to_string())?;
    let rec = ScribeApp::recording_dir();
    let mut archived = Vec::new();
    for step in steps {
        let src = rec.join(&step.screenshot);
        let dst = img_dir.join(&step.screenshot);
        if src.exists() {
            fs::rename(&src, &dst)
                .or_else(|_| {
                    fs::copy(&src, &dst)?;
                    fs::remove_file(&src)?;
                    Ok::<(), std::io::Error>(())
                })
                .map_err(|e| e.to_string())?;
        }
        archived.push(step);
    }
    let sess = ScribeSession {
        id: id.clone(),
        title: format!("操作指南 {id}"),
        created: now_ts(),
        steps: archived,
    };
    let json = serde_json::to_string_pretty(&sess).map_err(|e| e.to_string())?;
    fs::create_dir_all(ScribeApp::root_dir()).map_err(|e| e.to_string())?;
    fs::write(ScribeApp::session_json(&id), json).map_err(|e| e.to_string())?;
    Ok(id)
}

fn load_texture_thumb(ctx: &egui::Context, path: &Path, name: &str) -> Option<TextureHandle> {
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = (img.width(), img.height());
    let m = w.max(h);
    let rgba = if m > THUMB_MAX {
        let s = THUMB_MAX as f32 / m as f32;
        let nw = ((w as f32) * s).round().max(1.0) as u32;
        let nh = ((h as f32) * s).round().max(1.0) as u32;
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(format!("scribe_{name}"), color, Default::default()))
}

fn annotate_image(path: &Path, x: i32, y: i32) -> Result<RgbaImage, String> {
    let mut img = image::open(path)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let side = img.width().min(img.height()) as i32;
    let r = (side / 28).max(26);
    draw_glow(&mut img, x, y, r);
    draw_cursor(&mut img, x, y);
    Ok(img)
}

fn blend_pixel(img: &mut RgbaImage, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let a = rgba[3] as f32 / 255.0;
    let inv = 1.0 - a;
    p.0[0] = (rgba[0] as f32 * a + p.0[0] as f32 * inv) as u8;
    p.0[1] = (rgba[1] as f32 * a + p.0[1] as f32 * inv) as u8;
    p.0[2] = (rgba[2] as f32 * a + p.0[2] as f32 * inv) as u8;
}

fn draw_circle_outline(img: &mut RgbaImage, cx: i32, cy: i32, r: i32, color: [u8; 4], width: i32) {
    for t in 0..width {
        let rr = r - t;
        if rr <= 0 {
            break;
        }
        let mut x = 0;
        let mut y = rr;
        let mut d = 3 - 2 * rr;
        while x <= y {
            for (ox, oy) in [
                (x, y),
                (y, x),
                (-x, y),
                (-y, x),
                (x, -y),
                (y, -x),
                (-x, -y),
                (-y, -x),
            ] {
                blend_pixel(img, cx + ox, cy + oy, color);
            }
            if d < 0 {
                d += 4 * x + 6;
            } else {
                d += 4 * (x - y) + 10;
                y -= 1;
            }
            x += 1;
        }
    }
}

fn draw_circle_fill(img: &mut RgbaImage, cx: i32, cy: i32, r: i32, color: [u8; 4]) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                blend_pixel(img, cx + dx, cy + dy, color);
            }
        }
    }
}

fn draw_glow(img: &mut RgbaImage, cx: i32, cy: i32, r: i32) {
    draw_circle_fill(img, cx, cy, r, [255, 170, 90, 72]);
    draw_circle_outline(img, cx, cy, r, [220, 95, 0, 255], (r / 8).max(3));
    let rs = (r / 2).max(14);
    draw_circle_outline(img, cx, cy, rs, [190, 145, 105, 215], (r / 12).max(3));
}

fn draw_cursor(img: &mut RgbaImage, cx: i32, cy: i32) {
    let pts = [(0, 0), (0, 17), (4, 13), (7, 19), (9, 18), (6, 11), (11, 11)];
    for i in 0..pts.len() {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % pts.len()];
        draw_line(img, cx + x0, cy + y0, cx + x1, cy + y1, [0, 0, 0, 230]);
    }
    for y in 0..20 {
        for x in 0..12 {
            if point_in_poly(x, y, &pts) {
                let px = (cx + x).clamp(0, img.width() as i32 - 1) as u32;
                let py = (cy + y).clamp(0, img.height() as i32 - 1) as u32;
                img.put_pixel(px, py, Rgba([255, 255, 255, 255]));
            }
        }
    }
}

fn point_in_poly(x: i32, y: i32, pts: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi).max(1) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn draw_line(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    loop {
        blend_pixel(img, x, y, color);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn export_markdown(session: &ScribeSession, out_dir: &Path) -> Result<PathBuf, String> {
    let img_out = out_dir.join("images");
    fs::create_dir_all(&img_out).map_err(|e| e.to_string())?;
    let img_src = ScribeApp::session_images(&session.id);
    let mut lines = vec![format!("# {}", session.title), String::new()];
    for (i, step) in session.steps.iter().enumerate() {
        let title = if step.title.is_empty() {
            format!("第 {} 步", i + 1)
        } else {
            step.title.clone()
        };
        lines.push(format!("## {}. {}", i + 1, title));
        lines.push(String::new());
        let src = img_src.join(&step.screenshot);
        if src.exists() {
            let annotated = annotate_image(&src, step.x, step.y)?;
            let out_name = format!("step_{:03}.jpg", i + 1);
            let mut jpg = Vec::new();
            let rgb = image::DynamicImage::ImageRgba8(annotated).to_rgb8();
            rgb.write_to(&mut Cursor::new(&mut jpg), image::ImageFormat::Jpeg)
                .map_err(|e| e.to_string())?;
            fs::write(img_out.join(&out_name), jpg).map_err(|e| e.to_string())?;
            lines.push(format!("![第{}步](images/{out_name})", i + 1));
            lines.push(String::new());
        }
        if !step.description.is_empty() {
            lines.push(step.description.clone());
            lines.push(String::new());
        }
    }
    let path = out_dir.join("guide.md");
    fs::write(&path, lines.join("\n")).map_err(|e| e.to_string())?;
    Ok(path)
}

fn export_html(session: &ScribeSession, out_path: &Path) -> Result<(), String> {
    use image::GenericImageView;

    let img_src = ScribeApp::session_images(&session.id);
    let mut parts = vec![
        "<!DOCTYPE html><html lang='zh'><head><meta charset='utf-8'>".into(),
        format!("<title>{}</title>", html_escape(&session.title)),
        "<style>".into(),
        "body{font-family:-apple-system,'Segoe UI','Microsoft YaHei',sans-serif;max-width:820px;margin:40px auto;padding:0 20px;color:#222;background:#fafafa}".into(),
        "h1{border-bottom:3px solid #ff9200;padding-bottom:10px}".into(),
        ".step{margin:26px 0;padding:22px;border:1px solid #e3e8ef;border-radius:14px;background:#fff}".into(),
        ".step h2{margin-top:0;color:#cc6a00;font-size:18px}".into(),
        ".desc{color:#444;line-height:1.7;margin-top:12px}".into(),
        ".num{display:inline-block;background:#ff9200;color:#fff;width:26px;height:26px;border-radius:50%;text-align:center;line-height:26px;margin-right:8px;font-size:14px}".into(),
        ".shot{position:relative;display:block;width:100%;margin:6px 0}".into(),
        ".shot img{display:block;width:100%;border-radius:8px;border:1px solid #e3e8ef}".into(),
        ".marker{position:absolute;width:0;height:0}".into(),
        ".marker .pulse{position:absolute;transform:translate(-50%,-50%);width:62px;height:62px;border-radius:50%;border:4px solid #dc5f00;background:rgba(255,170,90,.28);animation:cs-pulse 1.6s ease-out infinite}".into(),
        ".marker .pulse::after{content:'';position:absolute;inset:31%;border-radius:50%;border:3px solid rgba(190,145,105,.92)}".into(),
        ".marker .cur{position:absolute;left:0;top:0;width:18px;height:28px;filter:drop-shadow(0 1px 2px rgba(0,0,0,.35))}".into(),
        "@keyframes cs-pulse{0%{box-shadow:0 0 0 0 rgba(220,95,0,.55)}70%{box-shadow:0 0 0 26px rgba(220,95,0,0)}100%{box-shadow:0 0 0 0 rgba(220,95,0,0)}}".into(),
        "</style></head><body>".into(),
        format!("<h1>{}</h1>", html_escape(&session.title)),
    ];

    for (i, step) in session.steps.iter().enumerate() {
        let title = if step.title.is_empty() {
            format!("第 {} 步", i + 1)
        } else {
            step.title.clone()
        };
        let src = img_src.join(&step.screenshot);
        let mut shot_html = String::new();
        if src.exists() {
            if let Ok(img) = image::open(&src) {
                let (w, h) = if step.img_w > 0 && step.img_h > 0 {
                    (step.img_w, step.img_h)
                } else {
                    img.dimensions()
                };
                let px = if w > 0 {
                    step.x as f64 * step.scale / w as f64 * 100.0
                } else {
                    50.0
                };
                let py = if h > 0 {
                    step.y as f64 * step.scale / h as f64 * 100.0
                } else {
                    50.0
                };
                let mut jpg = Vec::new();
                img.to_rgb8()
                    .write_to(&mut Cursor::new(&mut jpg), image::ImageFormat::Jpeg)
                    .map_err(|e| e.to_string())?;
                let b64 = b64_encode(&jpg);
                shot_html = format!(
                    "<div class='shot'><img src='data:image/jpeg;base64,{b64}' alt='step'>\
                     <div class='marker' style='left:{px:.2}%;top:{py:.2}%'>\
                     <span class='pulse'></span>\
                     <svg class='cur' viewBox='0 0 12 20' xmlns='http://www.w3.org/2000/svg'>\
                     <path d='M0,0 L0,17 L4,13 L7,19 L9,18 L6,11 L11,11 Z' fill='#fff' stroke='#111' stroke-width='1'/></svg>\
                     </div></div>"
                );
            }
        }
        parts.push(format!(
            "<div class='step'><h2><span class='num'>{}</span>{}</h2>{}<p class='desc'>{}</p></div>",
            i + 1,
            html_escape(&title),
            shot_html,
            html_escape(&step.description)
        ));
    }
    parts.push("</body></html>".into());
    fs::write(out_path, parts.join("\n")).map_err(|e| e.to_string())?;
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
