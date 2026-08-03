//! Visual flowchart editor: drag nodes, connect ports, edit props, save/load/run.

use crate::common::ElementCatalogItem;
use crate::flow_ai;
use crate::scribe_ai;
use crate::theme::{self, col, CTRL_H};
use crate::workflow::{self, StepType, WorkflowStep};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

const NODE_W: f32 = 150.0;
const NODE_H: f32 = 72.0;
const PORT_R: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Start,
    End,
    Click,
    Wait,
    Pause,
    Manual,
    LoopStart,
    LoopEnd,
    /// Vision-only condition with True/False out ports.
    IfVision,
    /// Loop while template matches; pairs with LoopEnd.
    LoopWhile,
    /// Keyboard text / special-key input into the focused control.
    TypeText,
}

impl NodeKind {
    fn color(self) -> Color32 {
        match self {
            NodeKind::Start => col().NODE_START,
            NodeKind::End => col().NODE_END,
            NodeKind::Click => col().NODE_CLICK,
            NodeKind::Wait => col().NODE_WAIT,
            NodeKind::Pause => col().NODE_PAUSE,
            NodeKind::Manual => col().NODE_MANUAL,
            NodeKind::LoopStart | NodeKind::LoopEnd | NodeKind::LoopWhile => col().NODE_LOOP,
            NodeKind::IfVision => col().NODE_IF,
            NodeKind::TypeText => col().NODE_TYPE,
        }
    }

    fn title(self) -> &'static str {
        match self {
            NodeKind::Start => crate::i18n::t("flow.node.start"),
            NodeKind::End => crate::i18n::t("flow.node.end"),
            NodeKind::Click => crate::i18n::t("flow.node.click"),
            NodeKind::Wait => crate::i18n::t("flow.node.wait"),
            NodeKind::Pause => crate::i18n::t("flow.node.pause"),
            NodeKind::Manual => crate::i18n::t("flow.node.manual"),
            NodeKind::LoopStart => crate::i18n::t("flow.node.loop_start"),
            NodeKind::LoopEnd => crate::i18n::t("flow.node.loop_end"),
            NodeKind::IfVision => crate::i18n::t("flow.node.if_vision"),
            NodeKind::LoopWhile => crate::i18n::t("flow.node.loop_while"),
            NodeKind::TypeText => crate::i18n::t("flow.node.type_text"),
        }
    }
}

fn default_type_interval_ms() -> u64 {
    30
}

fn default_threshold() -> f32 {
    0.8
}

fn default_loop_times() -> u32 {
    2
}

fn default_retry_ms() -> u64 {
    500
}

fn default_max_times() -> u32 {
    50
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum EdgeBranch {
    // pub for flow_md
    #[default]
    Main,
    True,
    False,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FailAction {
    Skip,
    Abort,
}

impl Default for FailAction {
    fn default() -> Self {
        Self::Skip
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FlowNode {
    pub id: u32,
    pub kind: NodeKind,
    pub pos: [f32; 2],
    // Click
    #[serde(default)]
    pub element_name: String,
    /// Legacy single fallback (merged into `or_elements` at runtime).
    #[serde(default)]
    pub fallback: String,
    /// Extra templates (OR): any match counts / can be clicked.
    #[serde(default)]
    pub or_elements: Vec<String>,
    #[serde(default = "default_threshold")]
    pub threshold: f32,
    #[serde(default)]
    pub pure_vision: bool,
    #[serde(default)]
    pub retries: u32,
    #[serde(default = "default_retry_ms")]
    pub retry_ms: u64,
    #[serde(default)]
    pub on_fail: FailAction,
    // Wait / Loop count
    #[serde(default = "default_loop_times")]
    pub seconds: u32,
    /// Max iterations for LoopWhile (default 50).
    #[serde(default = "default_max_times")]
    pub max_times: u32,
    // Pause / Manual
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub instruction: String,
    /// Keyboard input payload (TypeText).
    #[serde(default)]
    pub type_text: String,
    #[serde(default = "default_type_interval_ms")]
    pub type_interval_ms: u64,
}

impl FlowNode {
    pub(crate) fn new(id: u32, kind: NodeKind, pos: Pos2) -> Self {
        Self {
            id,
            kind,
            pos: [pos.x, pos.y],
            element_name: "element".into(),
            fallback: String::new(),
            or_elements: Vec::new(),
            threshold: 0.8,
            pure_vision: false,
            retries: 0,
            retry_ms: 500,
            on_fail: FailAction::Skip,
            seconds: if kind == NodeKind::LoopStart { 2 } else { 1 },
            max_times: 50,
            message: "请确认后继续".into(),
            instruction: String::new(),
            type_text: if kind == NodeKind::TypeText {
                "hello{Enter}".into()
            } else {
                String::new()
            },
            type_interval_ms: 30,
        }
    }

    fn rect(&self) -> Rect {
        Rect::from_min_size(
            Pos2::new(self.pos[0], self.pos[1]),
            Vec2::new(NODE_W, NODE_H),
        )
    }

    fn in_port(&self) -> Pos2 {
        let r = self.rect();
        Pos2::new(r.left(), r.center().y)
    }

    fn out_port(&self) -> Pos2 {
        let r = self.rect();
        Pos2::new(r.right(), r.center().y)
    }

    fn out_port_true(&self) -> Pos2 {
        let r = self.rect();
        Pos2::new(r.right(), r.top() + NODE_H * 0.32)
    }

    fn out_port_false(&self) -> Pos2 {
        let r = self.rect();
        Pos2::new(r.right(), r.top() + NODE_H * 0.72)
    }

    fn out_port_for(&self, branch: EdgeBranch) -> Pos2 {
        match (self.kind, branch) {
            (NodeKind::IfVision, EdgeBranch::True) => self.out_port_true(),
            (NodeKind::IfVision, EdgeBranch::False) => self.out_port_false(),
            _ => self.out_port(),
        }
    }

    /// OR candidates excluding primary (`element_name`). Empty slots dropped (runtime/compile).
    fn effective_or_elements(&self) -> Vec<String> {
        let mut v = Vec::new();
        if !self.fallback.trim().is_empty() {
            v.push(self.fallback.trim().to_string());
        }
        for e in &self.or_elements {
            let t = e.trim();
            if !t.is_empty() && !v.iter().any(|x| x == t) && t != self.element_name.trim() {
                v.push(t.to_string());
            }
        }
        v
    }

    /// Editor list: keep empty rows so「＋ OR」slots stay visible until filled.
    fn or_elements_for_edit(&self) -> Vec<String> {
        let mut v = self.or_elements.clone();
        if !self.fallback.trim().is_empty() {
            let fb = self.fallback.trim().to_string();
            if !v.iter().any(|x| x.trim() == fb) {
                v.insert(0, fb);
            }
        }
        v
    }

    fn or_subtitle(&self) -> String {
        let ors = self.effective_or_elements();
        if ors.is_empty() {
            self.element_name.clone()
        } else if ors.len() == 1 {
            format!("{} ∨ {}", self.element_name, ors[0])
        } else {
            format!("{} ∨+{}", self.element_name, ors.len())
        }
    }

    fn subtitle(&self) -> String {
        match self.kind {
            NodeKind::Start => "入口".into(),
            NodeKind::End => "出口".into(),
            NodeKind::Click => {
                let mut s = self.or_subtitle();
                if self.pure_vision {
                    s.push_str(" ·纯视觉");
                }
                s
            }
            NodeKind::IfVision => format!("{} ?", self.or_subtitle()),
            NodeKind::Wait => format!("{} 秒", self.seconds),
            NodeKind::TypeText => {
                let chars: Vec<char> = self.type_text.chars().collect();
                if chars.is_empty() {
                    "（空）".into()
                } else if chars.len() > 14 {
                    format!("{}…", chars.into_iter().take(14).collect::<String>())
                } else {
                    self.type_text.clone()
                }
            }
            NodeKind::LoopStart => format!("×{}", self.seconds.max(1)),
            NodeKind::LoopWhile => {
                format!("{} ≤{}", self.or_subtitle(), self.max_times.max(1))
            }
            NodeKind::LoopEnd => "回到循环".into(),
            NodeKind::Pause | NodeKind::Manual => {
                let chars: Vec<char> = self.message.chars().collect();
                if chars.len() > 14 {
                    format!("{}…", chars.into_iter().take(14).collect::<String>())
                } else {
                    self.message.clone()
                }
            }
        }
    }

    fn to_step(&self) -> Option<StepType> {
        match self.kind {
            NodeKind::Start | NodeKind::End | NodeKind::IfVision => None,
            NodeKind::Click => Some(StepType::Click {
                element_name: self.element_name.clone(),
                or_elements: self.effective_or_elements(),
                threshold: Some(self.threshold.clamp(0.1, 1.0)),
                pure_vision: Some(self.pure_vision),
                retries: Some(self.retries),
                retry_ms: Some(self.retry_ms.max(0)),
                on_fail: Some(match self.on_fail {
                    FailAction::Skip => workflow::ClickFailAction::Skip,
                    FailAction::Abort => workflow::ClickFailAction::Abort,
                }),
            }),
            NodeKind::Wait => Some(StepType::Wait {
                seconds: self.seconds.max(1),
            }),
            NodeKind::TypeText => Some(StepType::TypeText {
                text: self.type_text.clone(),
                interval_ms: Some(self.type_interval_ms.min(2000)),
            }),
            NodeKind::LoopStart => Some(StepType::LoopStart {
                times: self.seconds.max(1),
            }),
            NodeKind::LoopWhile => Some(StepType::LoopWhileStart {
                element_name: self.element_name.clone(),
                or_elements: self.effective_or_elements(),
                threshold: Some(self.threshold.clamp(0.1, 1.0)),
                retries: Some(self.retries),
                retry_ms: Some(self.retry_ms.max(0)),
                max_times: self.max_times.max(1),
            }),
            NodeKind::LoopEnd => Some(StepType::LoopEnd),
            NodeKind::Pause => Some(StepType::Pause {
                message: self.message.clone(),
            }),
            NodeKind::Manual => Some(StepType::Manual {
                message: self.message.clone(),
                instruction: if self.instruction.trim().is_empty() {
                    None
                } else {
                    Some(self.instruction.trim().to_string())
                },
            }),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FlowEdge {
    pub from: u32,
    pub to: u32,
    #[serde(default)]
    pub branch: EdgeBranch,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct FlowDocument {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub next_id: u32,
}

#[derive(Clone)]
struct FlowSnapshot {
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
    next_id: u32,
}

const MAX_UNDO: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragMode {
    None,
    Node(u32),
    Pan,
    Connect { from: u32, branch: EdgeBranch },
    Marquee,
}

pub struct FlowEditor {
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
    next_id: u32,
    /// Multi-selection; `primary` drives the inspector.
    selected: HashSet<u32>,
    primary: Option<u32>,
    clipboard: Vec<FlowNode>,
    clipboard_edges: Vec<FlowEdge>,
    undo_stack: Vec<FlowSnapshot>,
    redo_stack: Vec<FlowSnapshot>,
    drag: DragMode,
    drag_offset: Vec2,
    pan: Vec2,
    last_pointer: Pos2,
    marquee_a: Option<Pos2>,
    marquee_b: Option<Pos2>,
    path: String,
    /// Human title (used in Markdown export).
    title: String,
    /// Short description for Markdown blockquote.
    description: String,
    status: String,
    /// When set, main should hand steps to clicker and clear.
    pub pending_run: Option<Vec<WorkflowStep>>,
    /// When set, main hides UI then starts named element capture.
    pub pending_screenshot: Option<String>,
    /// Element catalog from recorder for visual pickers.
    element_catalog: Vec<ElementCatalogItem>,
    catalog_tex: HashMap<String, egui::TextureHandle>,
    /// Open visual library popup keyed by picker id_salt.
    picker_open: Option<String>,
    /// Node currently executing (from clicker), drawn highlighted.
    run_highlight: Option<u32>,
    /// One undo snapshot per property-edit session on the same selection.
    props_undo_pushed: bool,
    /// Natural-language prompt for AI flow generation.
    ai_prompt: String,
    ai_busy: Arc<AtomicBool>,
    ai_job: Option<Receiver<Result<(String, String, FlowDocument), String>>>,
}

impl FlowEditor {
    pub fn new() -> Self {
        let mut ed = Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 1,
            selected: HashSet::new(),
            primary: None,
            clipboard: Vec::new(),
            clipboard_edges: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            drag: DragMode::None,
            drag_offset: Vec2::ZERO,
            pan: Vec2::ZERO,
            last_pointer: Pos2::ZERO,
            marquee_a: None,
            marquee_b: None,
            path: String::new(),
            title: String::new(),
            description: String::new(),
            status: crate::i18n::t("flow.status.init").into(),
            pending_run: None,
            pending_screenshot: None,
            element_catalog: Vec::new(),
            catalog_tex: HashMap::new(),
            picker_open: None,
            run_highlight: None,
            props_undo_pushed: false,
            ai_prompt: String::new(),
            ai_busy: Arc::new(AtomicBool::new(false)),
            ai_job: None,
        };
        ed.reset_default_graph();
        ed.undo_stack.clear();
        ed.redo_stack.clear();
        ed
    }

    pub fn set_element_catalog(&mut self, items: Vec<ElementCatalogItem>) {
        let same = self.element_catalog.len() == items.len()
            && self
                .element_catalog
                .iter()
                .zip(items.iter())
                .all(|(a, b)| a.name == b.name && a.preview_path == b.preview_path);
        if !same {
            self.catalog_tex.clear();
            self.element_catalog = items;
        }
    }

    fn ensure_catalog_tex(&mut self, ctx: &egui::Context, name: &str, path: &str) {
        if path.is_empty() || self.catalog_tex.contains_key(name) {
            return;
        }
        if let Some(tex) = load_flow_thumb(ctx, path, name) {
            self.catalog_tex.insert(name.to_string(), tex);
        }
    }

    pub fn set_run_highlight(&mut self, node_id: Option<u32>) {
        self.run_highlight = node_id;
    }

    fn begin_prop_session_if_needed(&mut self, before: FlowSnapshot) {
        if !self.props_undo_pushed {
            self.undo_stack.push(before);
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
            self.props_undo_pushed = true;
        }
    }

    fn snapshot(&self) -> FlowSnapshot {
        FlowSnapshot {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            next_id: self.next_id,
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn apply_snapshot(&mut self, snap: FlowSnapshot) {
        self.nodes = snap.nodes;
        self.edges = snap.edges;
        self.next_id = snap.next_id;
        self.selected
            .retain(|id| self.nodes.iter().any(|n| n.id == *id));
        if self
            .primary
            .map(|p| !self.selected.contains(&p))
            .unwrap_or(true)
        {
            self.primary = self.selected.iter().next().copied();
        }
    }

    pub fn agent_undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            self.status = "没有可撤销的操作".into();
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.apply_snapshot(prev);
        self.props_undo_pushed = false;
        self.status = "已撤销".into();
        true
    }

    pub fn agent_redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            self.status = "没有可重做的操作".into();
            return false;
        };
        self.undo_stack.push(self.snapshot());
        self.apply_snapshot(next);
        self.props_undo_pushed = false;
        self.status = "已重做".into();
        true
    }

    fn select_only(&mut self, id: u32) {
        self.selected.clear();
        self.selected.insert(id);
        self.primary = Some(id);
        self.props_undo_pushed = false;
    }

    fn toggle_select(&mut self, id: u32) {
        if self.selected.contains(&id) {
            self.selected.remove(&id);
            if self.primary == Some(id) {
                self.primary = self.selected.iter().next().copied();
            }
        } else {
            self.selected.insert(id);
            self.primary = Some(id);
        }
        self.props_undo_pushed = false;
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.primary = None;
        self.props_undo_pushed = false;
    }

    fn is_selected(&self, id: u32) -> bool {
        self.selected.contains(&id)
    }

    /// Agent entrypoint: reset to seed graph.
    pub fn agent_reset(&mut self) {
        self.reset_default_graph();
    }

    /// Agent entrypoint: add node and return id.
    pub fn agent_add_node(&mut self, kind: NodeKind) -> u32 {
        self.push_undo();
        let pos = Pos2::new(200.0 - self.pan.x, 160.0 - self.pan.y);
        let id = self.alloc_id();
        self.nodes.push(FlowNode::new(id, kind, pos));
        self.select_only(id);
        self.status = format!("已添加「{}」节点", kind.title());
        id
    }

    /// Agent entrypoint: connect nodes (from out -> to in).
    pub fn agent_connect(&mut self, from: u32, to: u32) {
        self.connect(from, to, EdgeBranch::Main);
    }

    /// Agent entrypoint: connect with branch (`main`|`true`|`false`).
    pub fn agent_connect_branch(&mut self, from: u32, to: u32, branch: &str) {
        let b = match branch.to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "是" => EdgeBranch::True,
            "false" | "f" | "no" | "否" => EdgeBranch::False,
            _ => EdgeBranch::Main,
        };
        self.connect(from, to, b);
    }

    /// Agent entrypoint: load flow document or workflow txt.
    pub fn agent_load(&mut self, path: &str) -> Result<(), String> {
        self.load_flow(path)
    }

    /// Agent entrypoint: save flow json/md (+ companion txt).
    pub fn agent_save(&mut self, path: &str) -> Result<(), String> {
        self.save_flow(path)
    }

    /// Load built-in example: vision match → click, 10 successful iterations.
    pub fn load_example_vision_click_10(&mut self) -> Result<(), String> {
        let (title, doc) =
            crate::flow_md::import_markdown(crate::flow_md::EXAMPLE_VISION_CLICK_10_MD)?;
        self.push_undo();
        self.nodes = doc.nodes;
        self.edges = doc.edges;
        self.next_id = doc.next_id;
        self.title = title;
        self.description = "仅匹配成功才点击；否分支重试，不占循环次数。".into();
        self.path = "workflows/examples/vision-click-10.md".into();
        self.clear_selection();
        Ok(())
    }

    fn apply_generated_flow(&mut self, title: String, description: String, doc: FlowDocument) {
        self.push_undo();
        self.nodes = doc.nodes;
        self.edges = doc.edges;
        self.next_id = doc.next_id;
        self.title = title;
        self.description = description;
        self.clear_selection();
        self.layout_nodes();
    }

    fn request_ai_generate(&mut self) {
        if self.ai_busy.load(Ordering::Relaxed) {
            return;
        }
        let prompt = self.ai_prompt.trim().to_string();
        if prompt.is_empty() {
            self.status = "请先输入自然语言流程描述".into();
            return;
        }
        let names: Vec<String> = self
            .element_catalog
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let cfg = scribe_ai::AiConfig::load();
        let (tx, rx) = mpsc::channel();
        self.ai_job = Some(rx);
        self.ai_busy.store(true, Ordering::Relaxed);
        self.status = "AI 正在生成流程图…".into();
        let busy = self.ai_busy.clone();
        thread::spawn(move || {
            let r = flow_ai::generate_flow_document(&prompt, &names, &cfg);
            let _ = tx.send(r);
            busy.store(false, Ordering::Relaxed);
        });
    }

    fn drain_ai(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.ai_job.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok((title, description, doc))) => {
                let n = doc.nodes.len();
                let e = doc.edges.len();
                self.apply_generated_flow(title, description, doc);
                self.ai_job = None;
                self.ai_busy.store(false, Ordering::Relaxed);
                self.status = format!("AI 已生成流程图（{n} 节点 / {e} 边），可继续编辑");
                ctx.request_repaint();
            }
            Ok(Err(err)) => {
                self.ai_job = None;
                self.ai_busy.store(false, Ordering::Relaxed);
                self.status = format!("AI 生成失败: {err}");
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(TryRecvError::Disconnected) => {
                self.ai_job = None;
                self.ai_busy.store(false, Ordering::Relaxed);
                if !self.status.starts_with("AI 已生成") && !self.status.starts_with("AI 生成失败")
                {
                    self.status = "AI 生成中断（后台任务异常退出）".into();
                }
                ctx.request_repaint();
            }
        }
    }

    fn ui_ai_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(crate::i18n::t("flow.ai.title")).strong().size(13.0));
        ui.label(
            egui::RichText::new(crate::i18n::t("flow.ai.subtitle"))
                .size(10.0)
                .color(col().MUTED),
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.ai_prompt)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .hint_text(crate::i18n::t("flow.ai.hint")),
        );
        ui.add_space(4.0);
        let ai_busy = self.ai_busy.load(Ordering::Relaxed);
        ui.add_enabled_ui(!ai_busy, |ui| {
            let gen = egui::Button::new(
                egui::RichText::new(if ai_busy {
                    crate::i18n::t("flow.ai.busy")
                } else {
                    crate::i18n::t("flow.ai.title")
                })
                .color(Color32::WHITE)
                .strong(),
            )
            .fill(col().ACCENT)
            .min_size(Vec2::new(ui.available_width().max(120.0), CTRL_H));
            if ui.add(gen).clicked() {
                self.request_ai_generate();
            }
        });
        if ai_busy {
            ui.label(
                egui::RichText::new(crate::i18n::t("flow.ai.wait"))
                    .size(11.0)
                    .color(col().ACCENT),
            );
        } else if self.status.starts_with("AI 生成失败")
            || self.status.starts_with("请先输入自然语言")
            || self.status.starts_with("AI 生成中断")
        {
            ui.label(
                egui::RichText::new(&self.status)
                    .size(11.0)
                    .color(Color32::from_rgb(200, 80, 70)),
            );
        } else if self.status.starts_with("AI 已生成") || self.status.starts_with("AI 正在生成")
        {
            ui.label(
                egui::RichText::new(&self.status)
                    .size(11.0)
                    .color(col().ACCENT),
            );
        }
    }

    /// Agent: generate flow from natural language; optionally replace canvas.
    pub fn agent_ai_generate(
        &mut self,
        prompt: &str,
        replace: bool,
    ) -> Result<(String, usize, usize), String> {
        let names: Vec<String> = self
            .element_catalog
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let cfg = scribe_ai::AiConfig::load();
        let (title, description, doc) = flow_ai::generate_flow_document(prompt, &names, &cfg)?;
        let nodes = doc.nodes.len();
        let edges = doc.edges.len();
        if replace {
            self.apply_generated_flow(title.clone(), description, doc);
            self.status = format!("AI 已生成「{title}」（{nodes} 节点）");
        }
        Ok((title, nodes, edges))
    }

    /// Agent entrypoint: status string.
    pub fn status_text(&self) -> &str {
        &self.status
    }

    /// Agent entrypoint: get primary selected node id.
    #[allow(dead_code)]
    pub fn selected_node_id(&self) -> Option<u32> {
        self.primary
    }

    /// Agent entrypoint: auto-layout along Start → End chain.
    pub fn agent_auto_layout(&mut self) {
        self.auto_layout();
    }

    /// Agent entrypoint: duplicate current selection (copy + paste).
    pub fn agent_duplicate_selection(&mut self) -> usize {
        self.copy_selection();
        self.paste_clipboard()
    }

    /// Agent entrypoint: list node ids/kinds for graph planning.
    pub fn agent_nodes_overview(&self) -> Vec<(u32, NodeKind)> {
        self.nodes.iter().map(|n| (n.id, n.kind)).collect()
    }

    /// Agent entrypoint: rebuild the whole graph from linear workflow steps.
    pub fn agent_build_from_steps(&mut self, steps: &[WorkflowStep]) {
        self.push_undo();
        self.nodes.clear();
        self.edges.clear();
        self.next_id = 1;

        let start = FlowNode::new(self.alloc_id(), NodeKind::Start, Pos2::new(80.0, 180.0));
        let mut prev = start.id;
        self.nodes.push(start);

        let mut x = 300.0;
        for step in steps {
            let node = match &step.step_type {
                StepType::Click {
                    element_name,
                    or_elements,
                    threshold,
                    pure_vision,
                    retries,
                    retry_ms,
                    on_fail,
                } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::Click, Pos2::new(x, 180.0));
                    n.element_name = element_name.clone();
                    n.or_elements = or_elements.clone();
                    n.fallback.clear();
                    if let Some(t) = threshold {
                        n.threshold = *t;
                    }
                    if let Some(pv) = pure_vision {
                        n.pure_vision = *pv;
                    }
                    if let Some(r) = retries {
                        n.retries = *r;
                    }
                    if let Some(ms) = retry_ms {
                        n.retry_ms = *ms;
                    }
                    if let Some(f) = on_fail {
                        n.on_fail = match f {
                            workflow::ClickFailAction::Skip => FailAction::Skip,
                            workflow::ClickFailAction::Abort => FailAction::Abort,
                        };
                    }
                    n
                }
                StepType::Wait { seconds } => {
                    let mut n = FlowNode::new(self.alloc_id(), NodeKind::Wait, Pos2::new(x, 180.0));
                    n.seconds = *seconds;
                    n
                }
                StepType::TypeText { text, interval_ms } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::TypeText, Pos2::new(x, 180.0));
                    n.type_text = text.clone();
                    if let Some(ms) = interval_ms {
                        n.type_interval_ms = *ms;
                    }
                    n
                }
                StepType::LoopStart { times } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::LoopStart, Pos2::new(x, 180.0));
                    n.seconds = *times;
                    n
                }
                StepType::LoopWhileStart {
                    element_name,
                    or_elements,
                    threshold,
                    retries,
                    retry_ms,
                    max_times,
                } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::LoopWhile, Pos2::new(x, 180.0));
                    n.element_name = element_name.clone();
                    n.or_elements = or_elements.clone();
                    if let Some(t) = threshold {
                        n.threshold = *t;
                    }
                    if let Some(r) = retries {
                        n.retries = *r;
                    }
                    if let Some(ms) = retry_ms {
                        n.retry_ms = *ms;
                    }
                    n.max_times = (*max_times).max(1);
                    n
                }
                StepType::IfVision {
                    element_name,
                    or_elements,
                    threshold,
                    retries,
                    retry_ms,
                    ..
                } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::IfVision, Pos2::new(x, 180.0));
                    n.element_name = element_name.clone();
                    n.or_elements = or_elements.clone();
                    if let Some(t) = threshold {
                        n.threshold = *t;
                    }
                    if let Some(r) = retries {
                        n.retries = *r;
                    }
                    if let Some(ms) = retry_ms {
                        n.retry_ms = *ms;
                    }
                    n
                }
                StepType::Goto { .. } => {
                    // Compile artifact — skip in linear rebuild.
                    continue;
                }
                StepType::LoopEnd => {
                    FlowNode::new(self.alloc_id(), NodeKind::LoopEnd, Pos2::new(x, 180.0))
                }
                StepType::Pause { message } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::Pause, Pos2::new(x, 180.0));
                    n.message = message.clone();
                    n
                }
                StepType::Manual {
                    message,
                    instruction,
                } => {
                    let mut n =
                        FlowNode::new(self.alloc_id(), NodeKind::Manual, Pos2::new(x, 180.0));
                    n.message = message.clone();
                    n.instruction = instruction.clone().unwrap_or_default();
                    n
                }
            };
            let id = node.id;
            self.nodes.push(node);
            self.edges.push(FlowEdge {
                from: prev,
                to: id,
                branch: EdgeBranch::Main,
            });
            prev = id;
            x += 220.0;
        }

        let end = FlowNode::new(self.alloc_id(), NodeKind::End, Pos2::new(x, 180.0));
        let end_id = end.id;
        self.nodes.push(end);
        self.edges.push(FlowEdge {
            from: prev,
            to: end_id,
            branch: EdgeBranch::Main,
        });

        self.selected.clear();
        if let Some(n) = self.nodes.get(1) {
            self.select_only(n.id);
        } else {
            self.primary = None;
        }
        self.status = format!("已按 {} 步重建流程图", steps.len());
    }

    fn reset_default_graph(&mut self) {
        self.push_undo();
        self.nodes.clear();
        self.edges.clear();
        self.next_id = 1;
        let start = FlowNode::new(self.alloc_id(), NodeKind::Start, Pos2::new(80.0, 180.0));
        let click = FlowNode::new(self.alloc_id(), NodeKind::Click, Pos2::new(300.0, 180.0));
        let end = FlowNode::new(self.alloc_id(), NodeKind::End, Pos2::new(520.0, 180.0));
        let sid = start.id;
        let cid = click.id;
        let eid = end.id;
        self.nodes.push(start);
        self.nodes.push(click);
        self.nodes.push(end);
        self.edges.push(FlowEdge {
            from: sid,
            to: cid,
            branch: EdgeBranch::Main,
        });
        self.edges.push(FlowEdge {
            from: cid,
            to: eid,
            branch: EdgeBranch::Main,
        });
        self.select_only(cid);
        self.status = "已重置为示例流程".into();
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn add_node(&mut self, kind: NodeKind) {
        let _ = self.agent_add_node(kind);
    }

    fn node_mut(&mut self, id: u32) -> Option<&mut FlowNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    fn node(&self, id: u32) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        self.push_undo();
        let ids: Vec<u32> = self.selected.iter().copied().collect();
        let mut removed = 0usize;
        for id in ids {
            if let Some(n) = self.node(id) {
                if n.kind == NodeKind::Start {
                    continue;
                }
            }
            self.nodes.retain(|n| n.id != id);
            self.edges.retain(|e| e.from != id && e.to != id);
            self.selected.remove(&id);
            removed += 1;
        }
        if self
            .primary
            .map(|p| !self.selected.contains(&p))
            .unwrap_or(true)
        {
            self.primary = self.selected.iter().next().copied();
        }
        if removed == 0 {
            self.status = "不能删除「开始」节点".into();
        } else {
            self.status = format!("已删除 {} 个节点", removed);
        }
    }

    fn copy_selection(&mut self) {
        let ids: HashSet<u32> = self
            .selected
            .iter()
            .copied()
            .filter(|&id| {
                self.node(id)
                    .map(|n| n.kind != NodeKind::Start)
                    .unwrap_or(false)
            })
            .collect();
        if ids.is_empty() {
            self.status = "没有可复制的节点（开始节点不可复制）".into();
            self.clipboard.clear();
            self.clipboard_edges.clear();
            return;
        }
        self.clipboard = self
            .nodes
            .iter()
            .filter(|n| ids.contains(&n.id))
            .cloned()
            .collect();
        self.clipboard_edges = self
            .edges
            .iter()
            .filter(|e| ids.contains(&e.from) && ids.contains(&e.to))
            .cloned()
            .collect();
        self.status = format!("已复制 {} 个节点", self.clipboard.len());
    }

    fn paste_clipboard(&mut self) -> usize {
        if self.clipboard.is_empty() {
            self.status = "剪贴板为空".into();
            return 0;
        }
        self.push_undo();
        let to_paste = self.clipboard.clone();
        let edges = self.clipboard_edges.clone();
        let mut id_map: HashMap<u32, u32> = HashMap::new();
        let offset = Vec2::new(40.0, 40.0);
        let mut new_ids = Vec::new();
        for old in &to_paste {
            let new_id = self.alloc_id();
            id_map.insert(old.id, new_id);
            let mut n = old.clone();
            n.id = new_id;
            n.pos[0] += offset.x;
            n.pos[1] += offset.y;
            new_ids.push(new_id);
            self.nodes.push(n);
        }
        for e in &edges {
            if let (Some(&from), Some(&to)) = (id_map.get(&e.from), id_map.get(&e.to)) {
                self.edges.push(FlowEdge {
                    from,
                    to,
                    branch: e.branch,
                });
            }
        }
        // Shift clipboard for next paste
        for n in &mut self.clipboard {
            n.pos[0] += offset.x;
            n.pos[1] += offset.y;
        }
        self.selected = new_ids.iter().copied().collect();
        self.primary = new_ids.first().copied();
        self.status = format!("已粘贴 {} 个节点", new_ids.len());
        new_ids.len()
    }

    fn auto_layout(&mut self) {
        self.push_undo();
        self.layout_nodes();
    }

    /// Reposition nodes along the Start→… chain without pushing undo.
    fn layout_nodes(&mut self) {
        let start = match self.nodes.iter().find(|n| n.kind == NodeKind::Start) {
            Some(n) => n.id,
            None => {
                self.status = "缺少开始节点，无法自动布局".into();
                return;
            }
        };
        let mut by_from: HashMap<u32, u32> = HashMap::new();
        for e in &self.edges {
            by_from.insert(e.from, e.to);
        }

        let mut ordered = Vec::new();
        let mut cur = start;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(cur) {
                break;
            }
            ordered.push(cur);
            match by_from.get(&cur) {
                Some(&next) => cur = next,
                None => break,
            }
        }

        let mut x = 80.0_f32;
        let y = 180.0_f32;
        let gap = 220.0_f32;
        for id in &ordered {
            if let Some(n) = self.node_mut(*id) {
                n.pos = [x, y];
                x += gap;
            }
        }

        // Orphans (not on main chain) go to a second row
        let mut orphan_x = 80.0_f32;
        let orphan_y = 320.0_f32;
        let ordered_set: HashSet<u32> = ordered.into_iter().collect();
        for n in &mut self.nodes {
            if !ordered_set.contains(&n.id) {
                n.pos = [orphan_x, orphan_y];
                orphan_x += gap;
            }
        }
        self.status = "已自动布局（主链横向，游离节点第二行）".into();
    }

    fn connect(&mut self, from: u32, to: u32, requested: EdgeBranch) {
        let Some(a) = self.node(from) else { return };
        let Some(b) = self.node(to) else { return };
        // Allow IfVision「否」自环：未达阈值则重试，不消耗循环次数
        let false_self_retry =
            from == to && a.kind == NodeKind::IfVision && requested == EdgeBranch::False;
        if from == to && !false_self_retry {
            return;
        }
        if a.kind == NodeKind::End || b.kind == NodeKind::Start {
            self.status = "连线方向无效（结束不能引出，开始不能接入）".into();
            return;
        }
        let from_kind = a.kind;
        let branch = if from_kind == NodeKind::IfVision {
            match requested {
                EdgeBranch::True | EdgeBranch::False => requested,
                EdgeBranch::Main => {
                    if self.edge_target(from, EdgeBranch::True).is_none() {
                        EdgeBranch::True
                    } else {
                        EdgeBranch::False
                    }
                }
            }
        } else {
            EdgeBranch::Main
        };
        if from == to && branch != EdgeBranch::False {
            return;
        }

        self.push_undo();
        if from_kind == NodeKind::IfVision {
            self.edges
                .retain(|e| !(e.from == from && e.branch == branch));
        } else {
            self.edges.retain(|e| e.from != from);
        }
        // Multiple incoming allowed (branch joins).
        self.edges.push(FlowEdge { from, to, branch });
        let tag = match branch {
            EdgeBranch::True => " [是]",
            EdgeBranch::False => " [否]",
            EdgeBranch::Main => "",
        };
        self.status = format!("已连接 #{} → #{}{}", from, to, tag);
    }

    fn edge_target(&self, from: u32, branch: EdgeBranch) -> Option<u32> {
        self.edges
            .iter()
            .find(|e| e.from == from && e.branch == branch)
            .map(|e| e.to)
    }

    fn any_out_target(&self, from: u32) -> Option<u32> {
        self.edges.iter().find(|e| e.from == from).map(|e| e.to)
    }

    fn descendants(&self, start: u32) -> HashSet<u32> {
        let mut out = HashSet::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if !out.insert(id) {
                continue;
            }
            for e in self.edges.iter().filter(|e| e.from == id) {
                stack.push(e.to);
            }
        }
        out
    }

    /// First node of `kind` reachable from `start` along Main outs (no IfVision branches).
    fn find_kind_along(&self, start: u32, kind: NodeKind) -> Option<u32> {
        let mut cur = Some(start);
        let mut seen = HashSet::new();
        while let Some(id) = cur {
            if !seen.insert(id) {
                break;
            }
            let n = self.node(id)?;
            if n.kind == kind {
                return Some(id);
            }
            if matches!(
                n.kind,
                NodeKind::End | NodeKind::LoopEnd | NodeKind::IfVision
            ) && n.kind != kind
            {
                // Stop at control boundaries (unless looking for that kind).
                if n.kind == NodeKind::End || n.kind == NodeKind::LoopEnd {
                    break;
                }
            }
            cur = if n.kind == NodeKind::IfVision {
                self.edge_target(id, EdgeBranch::True)
            } else {
                self.edge_target(id, EdgeBranch::Main)
                    .or_else(|| self.any_out_target(id))
            };
        }
        None
    }

    /// 「否」最终回到同一视觉条件 → 未达标重试，不走循环结束。
    fn else_retries_if(&self, if_id: u32, else_to: u32) -> bool {
        if else_to == if_id {
            return true;
        }
        let mut cur = Some(else_to);
        let mut seen = HashSet::new();
        while let Some(id) = cur {
            if id == if_id {
                return true;
            }
            if !seen.insert(id) {
                break;
            }
            let Some(n) = self.node(id) else { break };
            if matches!(
                n.kind,
                NodeKind::LoopEnd | NodeKind::End | NodeKind::LoopStart | NodeKind::LoopWhile
            ) {
                return false;
            }
            if n.kind == NodeKind::IfVision {
                return false;
            }
            cur = self
                .edge_target(id, EdgeBranch::Main)
                .or_else(|| self.any_out_target(id));
        }
        false
    }

    /// First common descendant of `a` and `b` along a walk from `a` (join point).
    fn find_join(&self, a: u32, b: u32) -> Option<u32> {
        let from_b = self.descendants(b);
        let mut cur = Some(a);
        let mut seen = HashSet::new();
        while let Some(c) = cur {
            if !seen.insert(c) {
                break;
            }
            if from_b.contains(&c) {
                return Some(c);
            }
            cur = if self.node(c).map(|n| n.kind) == Some(NodeKind::IfVision) {
                self.edge_target(c, EdgeBranch::True)
                    .or_else(|| self.edge_target(c, EdgeBranch::False))
            } else {
                self.edge_target(c, EdgeBranch::Main)
                    .or_else(|| self.any_out_target(c))
            };
        }
        None
    }

    /// Static checks before compile/run.
    pub fn validate_flow(&self) -> Result<(), String> {
        let start = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Start)
            .ok_or_else(|| "缺少开始节点".to_string())?;
        if self.any_out_target(start.id).is_none() {
            return Err("开始节点没有连出线".into());
        }

        let reach = self.descendants(start.id);
        for n in &self.nodes {
            if n.kind == NodeKind::IfVision {
                if self.edge_target(n.id, EdgeBranch::True).is_none() {
                    return Err(format!(
                        "「视觉条件」#{} 缺少 True（是）出边，请从上方端口连出",
                        n.id
                    ));
                }
                if self.edge_target(n.id, EdgeBranch::False).is_none() {
                    return Err(format!(
                        "「视觉条件」#{} 缺少 False（否）出边，请从下方端口连出",
                        n.id
                    ));
                }
            }
            if !reach.contains(&n.id) && n.kind != NodeKind::Start {
                match n.kind {
                    NodeKind::End
                    | NodeKind::Click
                    | NodeKind::Wait
                    | NodeKind::Pause
                    | NodeKind::Manual
                    | NodeKind::LoopStart
                    | NodeKind::LoopEnd
                    | NodeKind::IfVision
                    | NodeKind::LoopWhile
                    | NodeKind::TypeText => {
                        return Err(format!(
                            "存在孤立节点「{}」#{}（从开始不可达）",
                            n.kind.title(),
                            n.id
                        ));
                    }
                    NodeKind::Start => {}
                }
            }
        }

        self.check_loop_balance(start.id, 0, &mut HashSet::new())?;
        Ok(())
    }

    fn check_loop_balance(
        &self,
        cur: u32,
        depth: i32,
        visiting: &mut HashSet<u32>,
    ) -> Result<(), String> {
        if !visiting.insert(cur) {
            return Ok(()); // back-edge / reentry — ignore for balance on shared tails
        }
        let node = self
            .node(cur)
            .ok_or_else(|| format!("节点 #{} 不存在", cur))?;
        let mut depth = depth;
        match node.kind {
            NodeKind::LoopStart | NodeKind::LoopWhile => depth += 1,
            NodeKind::LoopEnd => {
                if depth <= 0 {
                    visiting.remove(&cur);
                    return Err(format!("「循环结束」#{} 缺少配对的循环开始", cur));
                }
                depth -= 1;
            }
            NodeKind::End => {
                visiting.remove(&cur);
                if depth != 0 {
                    return Err(format!("到达结束时仍有 {} 层未闭合的循环", depth));
                }
                return Ok(());
            }
            _ => {}
        }

        let result = if node.kind == NodeKind::IfVision {
            let t = self
                .edge_target(cur, EdgeBranch::True)
                .ok_or_else(|| format!("「视觉条件」#{} 缺少 True 出边", cur))?;
            let f = self
                .edge_target(cur, EdgeBranch::False)
                .ok_or_else(|| format!("「视觉条件」#{} 缺少 False 出边", cur))?;
            self.check_loop_balance(t, depth, visiting)?;
            self.check_loop_balance(f, depth, visiting)
        } else if let Some(next) = self
            .edge_target(cur, EdgeBranch::Main)
            .or_else(|| self.any_out_target(cur))
        {
            self.check_loop_balance(next, depth, visiting)
        } else if node.kind != NodeKind::End {
            Err(format!("节点「{}」#{} 没有连出线", node.kind.title(), cur))
        } else {
            Ok(())
        };
        visiting.remove(&cur);
        result
    }

    /// Walk Start → … → End (with branches) and collect executable steps with jumps.
    pub fn compile_steps(&self) -> Result<Vec<WorkflowStep>, String> {
        self.validate_flow()?;
        let start = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Start)
            .ok_or_else(|| "缺少开始节点".to_string())?;

        let mut steps = Vec::new();
        self.compile_from(start.id, None, &mut steps)?;
        if steps.is_empty() {
            return Err("流程中没有可执行步骤".into());
        }
        Ok(steps)
    }

    fn compile_from(
        &self,
        start: u32,
        stop_at: Option<u32>,
        steps: &mut Vec<WorkflowStep>,
    ) -> Result<(), String> {
        let mut cur = start;
        let mut guard = 0u32;
        loop {
            guard += 1;
            if guard > 10_000 {
                return Err("编译超时：可能存在异常环路".into());
            }
            if stop_at == Some(cur) {
                return Ok(());
            }
            let node = self
                .node(cur)
                .ok_or_else(|| format!("节点 #{} 不存在", cur))?;
            if node.kind == NodeKind::End {
                return Ok(());
            }

            if node.kind == NodeKind::IfVision {
                let then_to = self
                    .edge_target(cur, EdgeBranch::True)
                    .ok_or_else(|| format!("「视觉条件」#{} 缺少 True 出边", cur))?;
                let else_to = self
                    .edge_target(cur, EdgeBranch::False)
                    .ok_or_else(|| format!("「视觉条件」#{} 缺少 False 出边", cur))?;

                // 否 → 回到本条件：未达标重试，只有「是→点击→循环结束」才算一次
                if self.else_retries_if(cur, else_to) {
                    let loop_end_id = self
                        .find_kind_along(then_to, NodeKind::LoopEnd)
                        .ok_or_else(|| {
                            format!(
                                "「视觉条件」#{} 的「是」路径需连到「循环结束」，\
                                 这样未达标重试才不占用循环次数",
                                cur
                            )
                        })?;

                    let if_pc = steps.len();
                    steps.push(WorkflowStep::with_node(
                        StepType::IfVision {
                            element_name: node.element_name.clone(),
                            or_elements: node.effective_or_elements(),
                            threshold: Some(node.threshold.clamp(0.1, 1.0)),
                            retries: Some(node.retries),
                            retry_ms: Some(node.retry_ms),
                            then_jump: 0,
                            else_jump: 0,
                        },
                        Some(node.id),
                    ));

                    let then_pc = steps.len();
                    self.compile_from(then_to, Some(loop_end_id), steps)?;
                    if let Some(le) = self.node(loop_end_id) {
                        if let Some(st) = le.to_step() {
                            steps.push(WorkflowStep::with_node(st, Some(le.id)));
                        }
                    }

                    let else_jump = if else_to == cur {
                        if_pc
                    } else {
                        let else_pc = steps.len();
                        self.compile_from(else_to, Some(cur), steps)?;
                        let goto_pc = steps.len();
                        steps.push(WorkflowStep::with_node(
                            StepType::Goto { jump: if_pc },
                            None,
                        ));
                        let _ = goto_pc;
                        else_pc
                    };

                    if let StepType::IfVision {
                        then_jump,
                        else_jump: ej,
                        ..
                    } = &mut steps[if_pc].step_type
                    {
                        *then_jump = then_pc;
                        *ej = else_jump;
                    }

                    match self
                        .edge_target(loop_end_id, EdgeBranch::Main)
                        .or_else(|| self.any_out_target(loop_end_id))
                    {
                        Some(next) => {
                            cur = next;
                            continue;
                        }
                        None => return Ok(()),
                    }
                }

                let join = self.find_join(then_to, else_to);

                let if_pc = steps.len();
                steps.push(WorkflowStep::with_node(
                    StepType::IfVision {
                        element_name: node.element_name.clone(),
                        or_elements: node.effective_or_elements(),
                        threshold: Some(node.threshold.clamp(0.1, 1.0)),
                        retries: Some(node.retries),
                        retry_ms: Some(node.retry_ms),
                        then_jump: 0,
                        else_jump: 0,
                    },
                    Some(node.id),
                ));

                let then_pc = steps.len();
                self.compile_from(then_to, join, steps)?;
                let goto_pc = steps.len();
                steps.push(WorkflowStep::with_node(StepType::Goto { jump: 0 }, None));

                let else_pc = steps.len();
                self.compile_from(else_to, join, steps)?;
                let after_else = steps.len();

                if let StepType::IfVision {
                    then_jump,
                    else_jump,
                    ..
                } = &mut steps[if_pc].step_type
                {
                    *then_jump = then_pc;
                    *else_jump = else_pc;
                }
                if let StepType::Goto { jump } = &mut steps[goto_pc].step_type {
                    *jump = after_else;
                }

                match join {
                    Some(j) if self.node(j).map(|n| n.kind) == Some(NodeKind::End) => {
                        return Ok(());
                    }
                    Some(j) => {
                        cur = j;
                        continue;
                    }
                    None => return Ok(()),
                }
            }

            if let Some(st) = node.to_step() {
                steps.push(WorkflowStep::with_node(st, Some(node.id)));
            }
            match self
                .edge_target(cur, EdgeBranch::Main)
                .or_else(|| self.any_out_target(cur))
            {
                Some(next) => cur = next,
                None => {
                    return Err(format!("节点「{}」没有连出线", node.kind.title()));
                }
            }
        }
    }

    fn save_flow(&mut self, path: &str) -> Result<(), String> {
        let doc = FlowDocument {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            next_id: self.next_id,
        };
        if path.ends_with(".md") {
            let title = if self.title.trim().is_empty() {
                std::path::Path::new(path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("未命名流程")
                    .to_string()
            } else {
                self.title.clone()
            };
            let md = crate::flow_md::export_markdown(&title, &self.description, &doc)?;
            std::fs::write(path, md).map_err(|e| e.to_string())?;
            self.title = title;
        } else {
            let json = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
            std::fs::write(path, json).map_err(|e| e.to_string())?;
        }
        // Companion .txt for clicker (skip for .md; still write alongside basename)
        if let Ok(steps) = self.compile_steps() {
            let txt_path = if path.ends_with(".flow.json") {
                path.trim_end_matches(".flow.json").to_string() + ".txt"
            } else if path.ends_with(".md") {
                path.trim_end_matches(".md").to_string() + ".txt"
            } else if path.ends_with(".json") {
                path.trim_end_matches(".json").to_string() + ".txt"
            } else {
                format!("{}.txt", path)
            };
            let _ = std::fs::write(&txt_path, workflow::steps_to_text(&steps));
        }
        self.path = path.to_string();
        Ok(())
    }

    fn load_flow(&mut self, path: &str) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if path.ends_with(".md") {
            let (title, doc) = crate::flow_md::import_markdown(&content)?;
            self.push_undo();
            self.nodes = doc.nodes;
            self.edges = doc.edges;
            self.next_id = doc.next_id;
            self.title = title;
            self.path = path.to_string();
            self.clear_selection();
            return Ok(());
        }
        if path.ends_with(".json") {
            let doc: FlowDocument = serde_json::from_str(&content).map_err(|e| e.to_string())?;
            self.push_undo();
            self.nodes = doc.nodes;
            self.edges = doc.edges;
            self.next_id = doc.next_id;
            self.path = path.to_string();
            self.clear_selection();
            Ok(())
        } else {
            // Import linear .txt as vertical chain
            let steps = workflow::parse_workflow_text(&content)?;
            self.push_undo();
            self.nodes.clear();
            self.edges.clear();
            self.next_id = 1;
            let mut y = 80.0;
            let start = FlowNode::new(self.alloc_id(), NodeKind::Start, Pos2::new(80.0, y));
            let mut prev = start.id;
            self.nodes.push(start);
            y += 110.0;
            for step in steps {
                let mut node = match step.step_type {
                    StepType::Click {
                        element_name,
                        or_elements,
                        threshold,
                        pure_vision,
                        retries,
                        retry_ms,
                        on_fail,
                    } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::Click, Pos2::new(80.0, y));
                        n.element_name = element_name;
                        n.or_elements = or_elements;
                        n.fallback.clear();
                        if let Some(t) = threshold {
                            n.threshold = t;
                        }
                        if let Some(pv) = pure_vision {
                            n.pure_vision = pv;
                        }
                        if let Some(r) = retries {
                            n.retries = r;
                        }
                        if let Some(ms) = retry_ms {
                            n.retry_ms = ms;
                        }
                        if let Some(f) = on_fail {
                            n.on_fail = match f {
                                workflow::ClickFailAction::Skip => FailAction::Skip,
                                workflow::ClickFailAction::Abort => FailAction::Abort,
                            };
                        }
                        n
                    }
                    StepType::Wait { seconds } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::Wait, Pos2::new(80.0, y));
                        n.seconds = seconds;
                        n
                    }
                    StepType::TypeText { text, interval_ms } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::TypeText, Pos2::new(80.0, y));
                        n.type_text = text;
                        if let Some(ms) = interval_ms {
                            n.type_interval_ms = ms;
                        }
                        n
                    }
                    StepType::LoopStart { times } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::LoopStart, Pos2::new(80.0, y));
                        n.seconds = times;
                        n
                    }
                    StepType::LoopWhileStart {
                        element_name,
                        or_elements,
                        threshold,
                        retries,
                        retry_ms,
                        max_times,
                    } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::LoopWhile, Pos2::new(80.0, y));
                        n.element_name = element_name;
                        n.or_elements = or_elements;
                        if let Some(t) = threshold {
                            n.threshold = t;
                        }
                        if let Some(r) = retries {
                            n.retries = r;
                        }
                        if let Some(ms) = retry_ms {
                            n.retry_ms = ms;
                        }
                        n.max_times = max_times.max(1);
                        n
                    }
                    StepType::IfVision {
                        element_name,
                        or_elements,
                        threshold,
                        retries,
                        retry_ms,
                        ..
                    } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::IfVision, Pos2::new(80.0, y));
                        n.element_name = element_name;
                        n.or_elements = or_elements;
                        if let Some(t) = threshold {
                            n.threshold = t;
                        }
                        if let Some(r) = retries {
                            n.retries = r;
                        }
                        if let Some(ms) = retry_ms {
                            n.retry_ms = ms;
                        }
                        n
                    }
                    StepType::Goto { .. } => continue,
                    StepType::LoopEnd => {
                        FlowNode::new(self.alloc_id(), NodeKind::LoopEnd, Pos2::new(80.0, y))
                    }
                    StepType::Pause { message } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::Pause, Pos2::new(80.0, y));
                        n.message = message;
                        n
                    }
                    StepType::Manual {
                        message,
                        instruction,
                    } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::Manual, Pos2::new(80.0, y));
                        n.message = message;
                        n.instruction = instruction.unwrap_or_default();
                        n
                    }
                };
                let id = node.id;
                // place horizontally
                node.pos = [
                    80.0 + (y - 80.0) * 0.0 + (self.nodes.len() as f32 - 1.0) * 180.0,
                    180.0,
                ];
                self.edges.push(FlowEdge {
                    from: prev,
                    to: id,
                    branch: EdgeBranch::Main,
                });
                prev = id;
                self.nodes.push(node);
                y += 110.0;
            }
            let end = FlowNode::new(
                self.alloc_id(),
                NodeKind::End,
                Pos2::new(80.0 + (self.nodes.len() as f32) * 180.0, 180.0),
            );
            let eid = end.id;
            self.edges.push(FlowEdge {
                from: prev,
                to: eid,
                branch: EdgeBranch::Main,
            });
            self.nodes.push(end);
            self.path = path.to_string();
            Ok(())
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.drain_ai(ctx);

        // Left toolbox — width follows window / language; user can drag to resize.
        let toolbox_default = if crate::i18n::lang() == crate::i18n::Lang::En {
            178.0
        } else {
            158.0
        };
        egui::SidePanel::left("flow_toolbox")
            .default_width(toolbox_default)
            .width_range(132.0..=260.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(col().PANEL_ELEVATED)
                    .stroke(Stroke::new(1.0, col().PANEL_EDGE))
                    .inner_margin(egui::Margin::same(10.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::t("flow.toolbox.title"))
                        .size(13.0)
                        .strong()
                        .color(col().TEXT),
                );
                ui.label(
                    egui::RichText::new(crate::i18n::t("flow.toolbox.subtitle"))
                        .size(10.0)
                        .color(col().MUTED),
                );
                ui.add_space(8.0);
                for (kind, hint) in [
                    (NodeKind::Click, "模板元素点击"),
                    (NodeKind::TypeText, "向当前焦点输入文字/按键"),
                    (NodeKind::IfVision, "匹配成功走是，失败走否"),
                    (NodeKind::Wait, "延时等待"),
                    (NodeKind::LoopStart, "循环开始（固定次数）"),
                    (NodeKind::LoopWhile, "匹配到则继续循环（防死循环有上限）"),
                    (NodeKind::LoopEnd, "循环结束"),
                    (NodeKind::Pause, "确认后继续"),
                    (NodeKind::Manual, "人工介入"),
                    (NodeKind::End, "流程结束"),
                ] {
                    if theme::fill_button(ui, kind.title(), kind.color())
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.add_node(kind);
                    }
                    ui.add_space(3.0);
                }
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                if ui.button(crate::i18n::t("flow.btn.reset")).clicked() {
                    self.reset_default_graph();
                }
                if ui.button(crate::i18n::t("flow.btn.layout")).clicked() {
                    self.auto_layout();
                }
                if ui.button(crate::i18n::t("flow.btn.undo")).clicked() {
                    self.agent_undo();
                }
                if ui.button(crate::i18n::t("flow.btn.redo")).clicked() {
                    self.agent_redo();
                }
                if ui.button(crate::i18n::t("flow.btn.copy")).clicked() {
                    self.copy_selection();
                }
                if ui.button(crate::i18n::t("flow.btn.paste")).clicked() {
                    self.paste_clipboard();
                }
                if ui.button(crate::i18n::t("flow.btn.delete")).clicked() {
                    self.delete_selected();
                }
            });

        // Right inspector — scales with window; drag edge to resize.
        let inspector_default = if crate::i18n::lang() == crate::i18n::Lang::En {
            260.0
        } else {
            228.0
        };
        egui::SidePanel::right("flow_inspector")
            .default_width(inspector_default)
            .width_range(200.0..=360.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(col().PANEL_ELEVATED)
                    .stroke(Stroke::new(1.0, col().PANEL_EDGE))
                    .inner_margin(egui::Margin::same(10.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("flow_inspector_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // AI first so it's never clipped below the fold.
                        self.ui_ai_panel(ui);
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(
                            egui::RichText::new(crate::i18n::t("flow.inspector.title"))
                                .size(13.0)
                                .strong()
                                .color(col().TEXT),
                        );
                        ui.add_space(8.0);
                        let mut request_shot: Option<String> = None;
                        let catalog = self.element_catalog.clone();
                        for item in &catalog {
                            self.ensure_catalog_tex(ctx, &item.name, &item.preview_path);
                        }

                        if let Some(id) = self.primary {
                            let before = self.snapshot();
                            let multi_hint = if self.selected.len() > 1 {
                                format!("  (+{})", self.selected.len() - 1)
                            } else {
                                String::new()
                            };
                            let mut props_changed = false;

                            let kind = self.nodes.iter().find(|n| n.id == id).map(|n| n.kind);
                            ui.label(format!(
                                "#{}  {}{}",
                                id,
                                kind.map(|k| k.title()).unwrap_or("?"),
                                multi_hint
                            ));
                            ui.add_space(6.0);

                            match kind {
                                Some(NodeKind::Click) => {
                                    let mut name = String::new();
                                    let mut or_elements = Vec::new();
                                    let mut threshold = 0.85_f32;
                                    let mut pure_vision = false;
                                    let mut retries = 0_u32;
                                    let mut retry_ms = 300_u64;
                                    let mut on_fail = FailAction::Skip;
                                    if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
                                        name = n.element_name.clone();
                                        or_elements = n.or_elements_for_edit();
                                        threshold = n.threshold;
                                        pure_vision = n.pure_vision;
                                        retries = n.retries;
                                        retry_ms = n.retry_ms;
                                        on_fail = n.on_fail;
                                    }
                                    ui.label(crate::i18n::t("flow.field.element"));
                                    props_changed |= self.element_name_picker(
                                        ui,
                                        ctx,
                                        "pick_click",
                                        &mut name,
                                        false,
                                    );
                                    props_changed |= self.or_elements_editor(
                                        ui,
                                        ctx,
                                        "or_click",
                                        &mut or_elements,
                                    );
                                    ui.add_space(6.0);
                                    ui.label(crate::i18n::t("flow.field.threshold"));
                                    props_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut threshold, 0.5..=0.99)
                                                .fixed_decimals(2),
                                        )
                                        .changed();
                                    props_changed |= ui
                                        .checkbox(&mut pure_vision, crate::i18n::t("flow.field.pure_vision"))
                                        .changed();
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.retries"));
                                        props_changed |= ui
                                            .add(egui::DragValue::new(&mut retries).range(0..=20))
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.retry_ms"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut retry_ms)
                                                    .range(0..=60000),
                                            )
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.on_fail"));
                                        let prev = on_fail;
                                        egui::ComboBox::from_id_salt("on_fail")
                                            .selected_text(match on_fail {
                                                FailAction::Skip => crate::i18n::t("flow.fail.skip"),
                                                FailAction::Abort => crate::i18n::t("flow.fail.abort"),
                                            })
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut on_fail,
                                                    FailAction::Skip,
                                                    crate::i18n::t("flow.fail.skip"),
                                                );
                                                ui.selectable_value(
                                                    &mut on_fail,
                                                    FailAction::Abort,
                                                    crate::i18n::t("flow.fail.abort"),
                                                );
                                            });
                                        if on_fail != prev {
                                            props_changed = true;
                                        }
                                    });
                                    ui.add_space(6.0);
                                    if ui
                                        .button(crate::i18n::t("flow.btn.shot"))
                                        .on_hover_text("先隐藏软件窗口，完全隐藏后再截屏框选")
                                        .clicked()
                                    {
                                        request_shot = Some(name.clone());
                                    }
                                    if let Some(n) = self.node_mut(id) {
                                        n.element_name = name;
                                        n.or_elements = or_elements;
                                        n.fallback.clear();
                                        n.threshold = threshold;
                                        n.pure_vision = pure_vision;
                                        n.retries = retries;
                                        n.retry_ms = retry_ms;
                                        n.on_fail = on_fail;
                                    }
                                }
                                Some(NodeKind::Wait) => {
                                    if let Some(node) = self.node_mut(id) {
                                        ui.label(crate::i18n::t("flow.field.seconds"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut node.seconds)
                                                    .range(1..=3600),
                                            )
                                            .changed();
                                    }
                                }
                                Some(NodeKind::TypeText) => {
                                    let mut text = String::new();
                                    let mut interval = 30_u64;
                                    if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
                                        text = n.type_text.clone();
                                        interval = n.type_interval_ms;
                                    }
                                    ui.label(crate::i18n::t("flow.field.type_text"));
                                    props_changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut text)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(3)
                                                .hint_text("文字或 {Enter} {Tab} {Ctrl+V}"),
                                        )
                                        .changed();
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.type_ms"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut interval).range(0..=2000),
                                            )
                                            .changed();
                                    });
                                    ui.label(
                                        egui::RichText::new(
                                            "支持中文；特殊键：{Enter} {Tab} {Esc} {Backspace}\n\
                                     {Ctrl+A/C/V/X/Z}；字面量大括号写 {{ }}",
                                        )
                                        .color(col().MUTED)
                                        .size(11.0),
                                    );
                                    if let Some(n) = self.node_mut(id) {
                                        n.type_text = text;
                                        n.type_interval_ms = interval;
                                    }
                                }
                                Some(NodeKind::LoopStart) => {
                                    if let Some(node) = self.node_mut(id) {
                                        ui.label(crate::i18n::t("flow.field.loop_times"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut node.seconds)
                                                    .range(1..=9999),
                                            )
                                            .changed();
                                    }
                                }
                                Some(NodeKind::LoopWhile) => {
                                    let mut name = String::new();
                                    let mut or_elements = Vec::new();
                                    let mut threshold = 0.85_f32;
                                    let mut retries = 0_u32;
                                    let mut retry_ms = 300_u64;
                                    let mut max_times = 10_u32;
                                    if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
                                        name = n.element_name.clone();
                                        or_elements = n.or_elements_for_edit();
                                        threshold = n.threshold;
                                        retries = n.retries;
                                        retry_ms = n.retry_ms;
                                        max_times = n.max_times;
                                    }
                                    ui.label("元素（匹配则继续）");
                                    props_changed |= self.element_name_picker(
                                        ui,
                                        ctx,
                                        "pick_while",
                                        &mut name,
                                        false,
                                    );
                                    props_changed |= self.or_elements_editor(
                                        ui,
                                        ctx,
                                        "or_while",
                                        &mut or_elements,
                                    );
                                    ui.add_space(4.0);
                                    ui.label(crate::i18n::t("flow.field.threshold"));
                                    props_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut threshold, 0.5..=0.99)
                                                .fixed_decimals(2),
                                        )
                                        .changed();
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.retries"));
                                        props_changed |= ui
                                            .add(egui::DragValue::new(&mut retries).range(0..=20))
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.retry_ms"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut retry_ms)
                                                    .range(0..=60000),
                                            )
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.max_times"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut max_times)
                                                    .range(1..=9999),
                                            )
                                            .changed();
                                    });
                                    ui.label(
                                        egui::RichText::new(
                                            "与「循环结束」配对；匹配失败或达上限则退出",
                                        )
                                        .color(col().MUTED),
                                    );
                                    ui.add_space(6.0);
                                    if ui
                                        .button(crate::i18n::t("flow.btn.shot"))
                                        .on_hover_text("先隐藏软件窗口，完全隐藏后再截屏框选")
                                        .clicked()
                                    {
                                        request_shot = Some(name.clone());
                                    }
                                    if let Some(n) = self.node_mut(id) {
                                        n.element_name = name;
                                        n.or_elements = or_elements;
                                        n.fallback.clear();
                                        n.threshold = threshold;
                                        n.retries = retries;
                                        n.retry_ms = retry_ms;
                                        n.max_times = max_times;
                                    }
                                }
                                Some(NodeKind::IfVision) => {
                                    let mut name = String::new();
                                    let mut or_elements = Vec::new();
                                    let mut threshold = 0.85_f32;
                                    let mut retries = 0_u32;
                                    let mut retry_ms = 300_u64;
                                    if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
                                        name = n.element_name.clone();
                                        or_elements = n.or_elements_for_edit();
                                        threshold = n.threshold;
                                        retries = n.retries;
                                        retry_ms = n.retry_ms;
                                    }
                                    ui.label("元素（只判断不点击）");
                                    props_changed |= self
                                        .element_name_picker(ui, ctx, "pick_if", &mut name, false);
                                    props_changed |=
                                        self.or_elements_editor(ui, ctx, "or_if", &mut or_elements);
                                    ui.add_space(4.0);
                                    ui.label(crate::i18n::t("flow.field.threshold"));
                                    props_changed |= ui
                                        .add(
                                            egui::Slider::new(&mut threshold, 0.5..=0.99)
                                                .fixed_decimals(2),
                                        )
                                        .changed();
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.retries"));
                                        props_changed |= ui
                                            .add(egui::DragValue::new(&mut retries).range(0..=20))
                                            .changed();
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(crate::i18n::t("flow.field.retry_ms"));
                                        props_changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut retry_ms)
                                                    .range(0..=60000),
                                            )
                                            .changed();
                                    });
                                    ui.label(
                                        egui::RichText::new(
                                            "上端口=是(匹配) · 下端口=否(未匹配)\n\
                                     否接回本节点=未达标重试（不占循环次数）",
                                        )
                                        .color(col().MUTED),
                                    );
                                    ui.add_space(6.0);
                                    if ui
                                        .button(crate::i18n::t("flow.btn.shot"))
                                        .on_hover_text("先隐藏软件窗口，完全隐藏后再截屏框选")
                                        .clicked()
                                    {
                                        request_shot = Some(name.clone());
                                    }
                                    if let Some(n) = self.node_mut(id) {
                                        n.element_name = name;
                                        n.or_elements = or_elements;
                                        n.fallback.clear();
                                        n.threshold = threshold;
                                        n.retries = retries;
                                        n.retry_ms = retry_ms;
                                    }
                                }
                                Some(NodeKind::LoopEnd) => {
                                    ui.label(
                                        egui::RichText::new(
                                            "与「循环开始/条件循环」配对，之间的步骤会重复执行",
                                        )
                                        .color(col().MUTED),
                                    );
                                }
                                Some(NodeKind::Pause) => {
                                    if let Some(node) = self.node_mut(id) {
                                        ui.label(crate::i18n::t("flow.field.message"));
                                        props_changed |=
                                            ui.text_edit_multiline(&mut node.message).changed();
                                    }
                                }
                                Some(NodeKind::Manual) => {
                                    if let Some(node) = self.node_mut(id) {
                                        ui.label(crate::i18n::t("flow.field.message"));
                                        props_changed |=
                                            ui.text_edit_multiline(&mut node.message).changed();
                                        ui.label(crate::i18n::t("flow.field.instruction"));
                                        props_changed |=
                                            ui.text_edit_multiline(&mut node.instruction).changed();
                                    }
                                }
                                Some(NodeKind::Start) | Some(NodeKind::End) => {
                                    ui.label(
                                        egui::RichText::new(crate::i18n::t("flow.inspector.system"))
                                            .color(col().MUTED),
                                    );
                                }
                                None => {}
                            }

                            if props_changed {
                                self.begin_prop_session_if_needed(before);
                            }
                        } else {
                            ui.label(
                                egui::RichText::new(crate::i18n::t("flow.inspector.select")).color(col().MUTED),
                            );
                        }
                        if let Some(name) = request_shot {
                            let name = name.trim().to_string();
                            if name.is_empty() {
                                self.status = "请先填写元素名再截屏".into();
                            } else {
                                self.pending_screenshot = Some(name.clone());
                                self.status = format!("准备截屏绑定「{}」…", name);
                            }
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(crate::i18n::t("flow.section.file")).strong());
                        ui.add_space(4.0);
                        ui.label(crate::i18n::t("flow.field.title"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.title)
                                .desired_width(f32::INFINITY)
                                .hint_text("视觉成功点击 10 次"),
                        );
                        ui.add_space(2.0);
                        ui.label(crate::i18n::t("flow.field.desc"));
                        ui.add(
                            egui::TextEdit::multiline(&mut self.description)
                                .desired_width(f32::INFINITY)
                                .desired_rows(2)
                                .hint_text("仅匹配成功才点击…"),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button(crate::i18n::t("flow.btn.open")).clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Flow", &["md", "flow.json", "json", "txt"])
                                    .pick_file()
                                {
                                    let p = path.to_string_lossy().to_string();
                                    match self.load_flow(&p) {
                                        Ok(()) => self.status = format!("已打开 {}", p),
                                        Err(e) => self.status = format!("打开失败: {}", e),
                                    }
                                }
                            }
                            if ui.button(crate::i18n::t("flow.btn.save")).clicked() {
                                let default = if self.path.is_empty() {
                                    if self.title.trim().is_empty() {
                                        "workflow.md".to_string()
                                    } else {
                                        format!("{}.md", self.title.trim())
                                    }
                                } else {
                                    self.path.clone()
                                };
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Markdown Flow", &["md"])
                                    .add_filter("Flow JSON", &["flow.json", "json"])
                                    .set_file_name(&default)
                                    .save_file()
                                {
                                    let p = path.to_string_lossy().to_string();
                                    match self.save_flow(&p) {
                                        Ok(()) => self.status = format!("已保存 {}", p),
                                        Err(e) => self.status = format!("保存失败: {}", e),
                                    }
                                }
                            }
                        });
                        if ui
                            .button(crate::i18n::t("flow.btn.example"))
                            .on_hover_text("成功匹配才点击，共 10 次；未达标重试不占次数")
                            .clicked()
                        {
                            match self.load_example_vision_click_10() {
                                Ok(()) => self.status = "已加载示例：视觉成功点击 10 次".into(),
                                Err(e) => self.status = format!("加载示例失败: {e}"),
                            }
                        }
                        ui.add_space(8.0);
                        let run = egui::Button::new(
                            egui::RichText::new(crate::i18n::t("flow.btn.run"))
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(col().ACCENT)
                        .stroke(egui::Stroke::NONE)
                        .min_size(Vec2::new(ui.available_width().max(120.0), CTRL_H + 4.0));
                        if ui.add(run).clicked() {
                            match self.compile_steps() {
                                Ok(steps) => {
                                    self.status = format!("开始执行 {} 步…", steps.len());
                                    self.pending_run = Some(steps);
                                }
                                Err(e) => self.status = format!("无法运行: {}", e),
                            }
                        }
                    }); // ScrollArea
            });

        egui::TopBottomPanel::bottom("flow_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&self.status)
                        .size(12.0)
                        .color(col().MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::t("flow.canvas.hints"))
                            .size(11.0)
                            .color(col().MUTED),
                    );
                });
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(col().CANVAS))
            .show(ctx, |ui| {
                self.draw_canvas(ui);
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            self.delete_selected();
        }
        let ctrl = ctx.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let shift = ctx.input(|i| i.modifiers.shift);
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::C)) {
            self.copy_selection();
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::V)) {
            self.paste_clipboard();
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::A)) {
            self.selected = self.nodes.iter().map(|n| n.id).collect();
            self.primary = self.nodes.first().map(|n| n.id);
            self.status = format!("已全选 {} 个节点", self.selected.len());
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::L)) {
            self.auto_layout();
        }
        if ctrl && !shift && ctx.input(|i| i.key_pressed(egui::Key::Z)) {
            self.agent_undo();
        }
        if (ctrl && ctx.input(|i| i.key_pressed(egui::Key::Y)))
            || (ctrl && shift && ctx.input(|i| i.key_pressed(egui::Key::Z)))
        {
            self.agent_redo();
        }
    }

    fn draw_canvas(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = response.rect;

        // Grid
        let grid = 24.0;
        let ox = (self.pan.x % grid + grid) % grid;
        let oy = (self.pan.y % grid + grid) % grid;
        let mut x = canvas.left() + ox;
        while x < canvas.right() {
            painter.line_segment(
                [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
                Stroke::new(1.0, col().GRID),
            );
            x += grid;
        }
        let mut y = canvas.top() + oy;
        while y < canvas.bottom() {
            painter.line_segment(
                [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
                Stroke::new(1.0, col().GRID),
            );
            y += grid;
        }

        let to_screen = |p: Pos2, pan: Vec2| p + pan + canvas.min.to_vec2();
        let from_screen = |p: Pos2, pan: Vec2, origin: Pos2| p - pan - origin.to_vec2();

        // Edges
        for e in &self.edges {
            let Some(a) = self.nodes.iter().find(|n| n.id == e.from) else {
                continue;
            };
            let Some(b) = self.nodes.iter().find(|n| n.id == e.to) else {
                continue;
            };
            let p0 = to_screen(a.out_port_for(e.branch), self.pan);
            let p1 = to_screen(b.in_port(), self.pan);
            let color = match e.branch {
                EdgeBranch::True => Color32::from_rgb(52, 211, 153),
                EdgeBranch::False => Color32::from_rgb(251, 113, 133),
                EdgeBranch::Main => col().WIRE,
            };
            draw_bezier(&painter, p0, p1, color, 2.0);
        }

        // Connecting preview
        if let DragMode::Connect { from, branch } = self.drag {
            if let Some(a) = self.nodes.iter().find(|n| n.id == from) {
                let p0 = to_screen(a.out_port_for(branch), self.pan);
                let p1 = self.last_pointer;
                draw_bezier(&painter, p0, p1, col().ACCENT_DIM, 2.0);
            }
        }

        // Pointer
        if let Some(pos) = response.interact_pointer_pos() {
            self.last_pointer = pos;
        }

        // Nodes hit-test (topmost first)
        let mut hit_node: Option<u32> = None;
        let mut hit_out: Option<(u32, EdgeBranch)> = None;
        let mut hit_in: Option<u32> = None;
        if let Some(pos) = response.interact_pointer_pos() {
            for n in self.nodes.iter().rev() {
                let r = n.rect().translate(self.pan + canvas.min.to_vec2());
                if n.kind == NodeKind::IfVision {
                    let t = to_screen(n.out_port_true(), self.pan);
                    let f = to_screen(n.out_port_false(), self.pan);
                    if t.distance(pos) <= PORT_R + 4.0 {
                        hit_out = Some((n.id, EdgeBranch::True));
                        break;
                    }
                    if f.distance(pos) <= PORT_R + 4.0 {
                        hit_out = Some((n.id, EdgeBranch::False));
                        break;
                    }
                } else if n.kind != NodeKind::End {
                    let out_c = to_screen(n.out_port(), self.pan);
                    if out_c.distance(pos) <= PORT_R + 4.0 {
                        hit_out = Some((n.id, EdgeBranch::Main));
                        break;
                    }
                }
                let in_c = to_screen(n.in_port(), self.pan);
                if in_c.distance(pos) <= PORT_R + 4.0 && n.kind != NodeKind::Start {
                    hit_in = Some(n.id);
                    break;
                }
                if r.contains(pos) {
                    hit_node = Some(n.id);
                    break;
                }
            }
        }

        // Input handling
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let shift = ui.input(|i| i.modifiers.shift);

        if response.drag_started() {
            if let Some((id, branch)) = hit_out {
                self.drag = DragMode::Connect { from: id, branch };
            } else if let Some(id) = hit_node {
                if ctrl {
                    self.toggle_select(id);
                } else if shift {
                    self.selected.insert(id);
                    self.primary = Some(id);
                } else if !self.is_selected(id) {
                    self.select_only(id);
                } else {
                    self.primary = Some(id);
                }
                if let Some(n) = self.node(id) {
                    let origin = n.rect().min;
                    if let Some(pos) = response.interact_pointer_pos() {
                        let world = from_screen(pos, self.pan, canvas.min);
                        self.drag_offset = world - origin;
                    }
                }
                self.push_undo();
                self.drag = DragMode::Node(id);
            } else if let Some(pos) = response.interact_pointer_pos() {
                // Empty canvas: Ctrl+drag = marquee, otherwise pan
                if ctrl {
                    self.marquee_a = Some(pos);
                    self.marquee_b = Some(pos);
                    self.drag = DragMode::Marquee;
                } else {
                    self.drag = DragMode::Pan;
                }
            }
        }

        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            match self.drag {
                DragMode::Node(id) => {
                    let move_ids: Vec<u32> = if self.is_selected(id) {
                        self.selected.iter().copied().collect()
                    } else {
                        vec![id]
                    };
                    for nid in move_ids {
                        if let Some(n) = self.node_mut(nid) {
                            n.pos[0] += delta.x;
                            n.pos[1] += delta.y;
                        }
                    }
                }
                DragMode::Pan => {
                    self.pan += delta;
                }
                DragMode::Marquee => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.marquee_b = Some(pos);
                    }
                }
                DragMode::Connect { .. } | DragMode::None => {}
            }
        }

        if response.drag_stopped() {
            match self.drag {
                DragMode::Connect { from, branch } => {
                    if let Some(to) = hit_in {
                        self.connect(from, to, branch);
                    } else if let Some(to) = hit_node {
                        if to != from {
                            self.connect(from, to, branch);
                        }
                    }
                }
                DragMode::Marquee => {
                    if let (Some(a), Some(b)) = (self.marquee_a, self.marquee_b) {
                        let screen_rect = Rect::from_two_pos(a, b);
                        // Ignore tiny marquee (treat as click clear)
                        if screen_rect.width() > 4.0 || screen_rect.height() > 4.0 {
                            let mut count = 0usize;
                            for n in &self.nodes {
                                let r = n.rect().translate(self.pan + canvas.min.to_vec2());
                                if screen_rect.intersects(r) {
                                    self.selected.insert(n.id);
                                    count += 1;
                                }
                            }
                            self.primary = self.selected.iter().next().copied();
                            self.status = format!("框选了 {} 个节点", count);
                        }
                    }
                    self.marquee_a = None;
                    self.marquee_b = None;
                }
                _ => {}
            }
            self.drag = DragMode::None;
        }

        if response.clicked() && !response.dragged() {
            if let Some(id) = hit_node.or(hit_out.map(|(id, _)| id)).or(hit_in) {
                if ctrl {
                    self.toggle_select(id);
                } else {
                    self.select_only(id);
                }
            } else if !ctrl {
                self.clear_selection();
            }
        }

        // Draw marquee
        if let (Some(a), Some(b)) = (self.marquee_a, self.marquee_b) {
            let r = Rect::from_two_pos(a, b);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(45, 212, 191, 40));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, col().ACCENT_DIM));
        }

        // Draw nodes
        for n in &self.nodes {
            let r = n.rect().translate(self.pan + canvas.min.to_vec2());
            let accent = n.kind.color();
            let selected_here = self.is_selected(n.id);
            let running_here = self.run_highlight == Some(n.id);

            if running_here {
                painter.rect_filled(
                    r.expand(4.0),
                    10.0,
                    Color32::from_rgba_unmultiplied(250, 204, 21, 40),
                );
            }
            painter.rect_filled(r, 8.0, col().NODE_BG);
            painter.rect_stroke(
                r,
                8.0,
                Stroke::new(
                    if running_here {
                        3.0
                    } else if selected_here {
                        2.5
                    } else {
                        1.0
                    },
                    if running_here {
                        Color32::from_rgb(250, 204, 21)
                    } else if selected_here {
                        col().NODE_SEL
                    } else {
                        accent
                    },
                ),
            );
            // Top accent bar
            let bar = Rect::from_min_max(r.min, Pos2::new(r.max.x, r.min.y + 6.0));
            painter.rect_filled(bar, 8.0, accent);
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(r.min.x, r.min.y + 3.0),
                    Pos2::new(r.max.x, r.min.y + 6.0),
                ),
                0.0,
                accent,
            );

            painter.text(
                Pos2::new(r.center().x, r.min.y + 22.0),
                egui::Align2::CENTER_CENTER,
                n.kind.title(),
                FontId::proportional(14.0),
                Color32::WHITE,
            );
            painter.text(
                Pos2::new(r.center().x, r.min.y + 44.0),
                egui::Align2::CENTER_CENTER,
                n.subtitle(),
                FontId::proportional(11.0),
                Color32::from_rgb(186, 230, 253),
            );

            // Ports
            if n.kind != NodeKind::Start {
                let c = to_screen(n.in_port(), self.pan);
                painter.circle_filled(c, PORT_R, col().CANVAS);
                painter.circle_stroke(c, PORT_R, Stroke::new(2.0, accent));
            }
            if n.kind == NodeKind::IfVision {
                let t = to_screen(n.out_port_true(), self.pan);
                let f = to_screen(n.out_port_false(), self.pan);
                painter.circle_filled(t, PORT_R, Color32::from_rgb(52, 211, 153));
                painter.circle_stroke(t, PORT_R, Stroke::new(1.5, Color32::WHITE));
                painter.text(
                    Pos2::new(t.x + 12.0, t.y),
                    egui::Align2::LEFT_CENTER,
                    crate::i18n::t("flow.branch.yes"),
                    FontId::proportional(10.0),
                    Color32::from_rgb(52, 211, 153),
                );
                painter.circle_filled(f, PORT_R, Color32::from_rgb(251, 113, 133));
                painter.circle_stroke(f, PORT_R, Stroke::new(1.5, Color32::WHITE));
                painter.text(
                    Pos2::new(f.x + 12.0, f.y),
                    egui::Align2::LEFT_CENTER,
                    crate::i18n::t("flow.branch.no"),
                    FontId::proportional(10.0),
                    Color32::from_rgb(251, 113, 133),
                );
            } else if n.kind != NodeKind::End {
                let c = to_screen(n.out_port(), self.pan);
                painter.circle_filled(c, PORT_R, accent);
                painter.circle_stroke(c, PORT_R, Stroke::new(1.5, Color32::WHITE));
            }
        }

        // Title overlay
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(canvas.left() + 10.0, canvas.top() + 10.0),
                Vec2::new(108.0, 28.0),
            ),
            6.0,
            Color32::from_rgba_unmultiplied(10, 22, 40, 180),
        );
        painter.text(
            Pos2::new(canvas.left() + 20.0, canvas.top() + 16.0),
            egui::Align2::LEFT_TOP,
            "流程画布",
            FontId::proportional(13.0),
            Color32::from_rgb(186, 200, 220),
        );
        if self.run_highlight.is_some() {
            painter.text(
                Pos2::new(canvas.left() + 130.0, canvas.top() + 16.0),
                egui::Align2::LEFT_TOP,
                "● 执行中",
                FontId::proportional(12.0),
                col().NODE_SEL,
            );
        }
    }

    /// Edit OR candidate list: 「＋ OR」adds another recorded image alternative.
    /// Keeps empty slots so newly added rows remain visible until filled.
    fn or_elements_editor(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        id_salt: &str,
        or_elements: &mut Vec<String>,
    ) -> bool {
        let mut changed = false;
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("OR 候选（任一匹配即可）")
                .size(12.0)
                .strong()
                .color(col().TEXT),
        );
        ui.label(
            egui::RichText::new("多张图任一张识别到都算成功")
                .size(10.0)
                .color(col().MUTED),
        );
        ui.add_space(4.0);
        let add = egui::Button::new(
            egui::RichText::new("＋ OR 添加图片")
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(col().ACCENT)
        .min_size(Vec2::new(ui.available_width(), 28.0));
        if ui
            .add(add)
            .on_hover_text("再增加一张可识别的图片；任意一张匹配即成功")
            .clicked()
        {
            or_elements.push(String::new());
            changed = true;
        }

        let mut remove_at: Option<usize> = None;
        for i in 0..or_elements.len() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("OR{}", i + 1))
                        .size(11.0)
                        .color(col().MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("删除")
                        .on_hover_text("移除此候选")
                        .clicked()
                    {
                        remove_at = Some(i);
                    }
                });
            });
            let salt = format!("{id_salt}_{i}");
            changed |= self.element_name_picker(ui, ctx, &salt, &mut or_elements[i], true);
        }
        if let Some(i) = remove_at {
            or_elements.remove(i);
            changed = true;
        }
        changed
    }

    /// Dropdown menu + text field + visual element library popup.
    fn element_name_picker(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        id_salt: &str,
        value: &mut String,
        allow_empty: bool,
    ) -> bool {
        let mut changed = false;
        let catalog = self.element_catalog.clone();

        let selected_label = if value.is_empty() {
            if catalog.is_empty() {
                "（暂无录制元素）".to_string()
            } else {
                "选择录制图片…".to_string()
            }
        } else {
            value.clone()
        };
        egui::ComboBox::from_id_salt(format!("combo_{id_salt}"))
            .selected_text(selected_label)
            .width(ui.available_width().max(160.0))
            .show_ui(ui, |ui| {
                if allow_empty {
                    if ui.selectable_label(value.is_empty(), "(无)").clicked() {
                        value.clear();
                        changed = true;
                    }
                }
                if catalog.is_empty() {
                    ui.label(
                        egui::RichText::new("请先在「录制」页添加模板")
                            .size(12.0)
                            .color(col().MUTED),
                    );
                } else {
                    for item in &catalog {
                        let selected = value == &item.name;
                        if ui.selectable_label(selected, &item.name).clicked() {
                            *value = item.name.clone();
                            changed = true;
                        }
                    }
                }
            });

        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(value)
                        .desired_width(120.0)
                        .hint_text("或手动输入名称"),
                )
                .changed();
            let open = self.picker_open.as_deref() == Some(id_salt);
            let btn = if open { "收起" } else { "图库" };
            if ui
                .button(btn)
                .on_hover_text("用缩略图从录制库中选择")
                .clicked()
            {
                self.picker_open = if open {
                    None
                } else {
                    Some(id_salt.to_string())
                };
            }
        });

        if !value.is_empty() {
            if let Some(item) = catalog.iter().find(|e| e.name == *value) {
                self.ensure_catalog_tex(ctx, &item.name, &item.preview_path);
            }
            if let Some(tex) = self.catalog_tex.get(value.as_str()) {
                let size = tex.size_vec2();
                let s = (72.0 / size.x).min(48.0 / size.y).min(1.0);
                ui.add(
                    egui::Image::new(tex)
                        .fit_to_exact_size(size * s)
                        .rounding(6.0),
                );
            }
        }

        if self.picker_open.as_deref() == Some(id_salt) {
            for item in &catalog {
                self.ensure_catalog_tex(ctx, &item.name, &item.preview_path);
            }
            egui::Frame::none()
                .fill(col().INSET)
                .rounding(egui::Rounding::same(10.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_max_height(220.0);
                    if allow_empty {
                        if ui.selectable_label(value.is_empty(), "(无)").clicked() {
                            value.clear();
                            changed = true;
                            self.picker_open = None;
                        }
                    }
                    if catalog.is_empty() {
                        ui.label(
                            egui::RichText::new("元素库为空，请先在「录制」页添加")
                                .size(12.0)
                                .color(col().MUTED),
                        );
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let cols = 2;
                        egui::Grid::new(format!("lib_{id_salt}"))
                            .num_columns(cols)
                            .spacing([8.0, 8.0])
                            .show(ui, |ui| {
                                for (i, item) in catalog.iter().enumerate() {
                                    let selected = value == &item.name;
                                    let tex = self.catalog_tex.get(&item.name);
                                    let cell = egui::Frame::none()
                                        .fill(if selected {
                                            Color32::from_rgb(224, 236, 255)
                                        } else {
                                            col().PANEL_ELEVATED
                                        })
                                        .stroke(Stroke::new(
                                            1.0,
                                            if selected {
                                                col().ACCENT
                                            } else {
                                                col().PANEL_EDGE
                                            },
                                        ))
                                        .rounding(egui::Rounding::same(8.0))
                                        .inner_margin(egui::Margin::same(6.0))
                                        .show(ui, |ui| {
                                            ui.set_min_width(88.0);
                                            let thumb = egui::vec2(72.0, 48.0);
                                            let (rect, _) =
                                                ui.allocate_exact_size(thumb, Sense::hover());
                                            ui.painter().rect_filled(rect, 6.0, col().INSET);
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
                                                    Color32::WHITE,
                                                );
                                            }
                                            ui.label(
                                                egui::RichText::new(&item.name)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(col().TEXT),
                                            );
                                        });
                                    if ui
                                        .interact(
                                            cell.response.rect,
                                            ui.id().with(("pick", id_salt, i)),
                                            Sense::click(),
                                        )
                                        .clicked()
                                    {
                                        *value = item.name.clone();
                                        changed = true;
                                        self.picker_open = None;
                                    }
                                    if (i + 1) % cols == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                });
        }
        changed
    }
}

fn draw_bezier(painter: &egui::Painter, p0: Pos2, p1: Pos2, color: Color32, width: f32) {
    let dx = (p1.x - p0.x).abs().max(40.0) * 0.5;
    let c0 = Pos2::new(p0.x + dx, p0.y);
    let c1 = Pos2::new(p1.x - dx, p1.y);
    let steps = 24;
    let mut prev = p0;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let pt = Pos2::new(
            u * u * u * p0.x + 3.0 * u * u * t * c0.x + 3.0 * u * t * t * c1.x + t * t * t * p1.x,
            u * u * u * p0.y + 3.0 * u * u * t * c0.y + 3.0 * u * t * t * c1.y + t * t * t * p1.y,
        );
        painter.line_segment([prev, pt], Stroke::new(width, color));
        prev = pt;
    }
    let dir = (p1 - c1).normalized();
    let orth = Vec2::new(-dir.y, dir.x);
    let tip = p1;
    let base = p1 - dir * 10.0;
    painter.line_segment([tip, base + orth * 5.0], Stroke::new(width, color));
    painter.line_segment([tip, base - orth * 5.0], Stroke::new(width, color));
}

fn load_flow_thumb(ctx: &egui::Context, path: &str, name: &str) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = (img.width(), img.height());
    let max_side = 128u32;
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
        format!("flow_elem_{name}"),
        color,
        egui::TextureOptions::LINEAR,
    ))
}
