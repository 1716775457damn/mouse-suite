//! Visual flowchart editor: drag nodes, connect ports, edit props, save/load/run.

use crate::theme::colors;
use crate::workflow::{self, StepType, WorkflowStep};
use eframe::egui::{
    self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
}

impl NodeKind {
    fn color(self) -> Color32 {
        match self {
            NodeKind::Start => colors::NODE_START,
            NodeKind::End => colors::NODE_END,
            NodeKind::Click => colors::NODE_CLICK,
            NodeKind::Wait => colors::NODE_WAIT,
            NodeKind::Pause => colors::NODE_PAUSE,
            NodeKind::Manual => colors::NODE_MANUAL,
        }
    }

    fn title(self) -> &'static str {
        match self {
            NodeKind::Start => "开始",
            NodeKind::End => "结束",
            NodeKind::Click => "点击",
            NodeKind::Wait => "等待",
            NodeKind::Pause => "暂停",
            NodeKind::Manual => "人工",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct FlowNode {
    id: u32,
    kind: NodeKind,
    pos: [f32; 2],
    // Click
    element_name: String,
    fallback: String,
    // Wait
    seconds: u32,
    // Pause / Manual
    message: String,
    instruction: String,
}

impl FlowNode {
    fn new(id: u32, kind: NodeKind, pos: Pos2) -> Self {
        Self {
            id,
            kind,
            pos: [pos.x, pos.y],
            element_name: "element".into(),
            fallback: String::new(),
            seconds: 1,
            message: "请确认后继续".into(),
            instruction: String::new(),
        }
    }

    fn rect(&self) -> Rect {
        Rect::from_min_size(Pos2::new(self.pos[0], self.pos[1]), Vec2::new(NODE_W, NODE_H))
    }

    fn in_port(&self) -> Pos2 {
        let r = self.rect();
        Pos2::new(r.left(), r.center().y)
    }

    fn out_port(&self) -> Pos2 {
        let r = self.rect();
        Pos2::new(r.right(), r.center().y)
    }

    fn subtitle(&self) -> String {
        match self.kind {
            NodeKind::Start => "入口".into(),
            NodeKind::End => "出口".into(),
            NodeKind::Click => {
                if self.fallback.is_empty() {
                    self.element_name.clone()
                } else {
                    format!("{} / {}", self.element_name, self.fallback)
                }
            }
            NodeKind::Wait => format!("{} 秒", self.seconds),
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
            NodeKind::Start | NodeKind::End => None,
            NodeKind::Click => Some(StepType::Click {
                element_name: self.element_name.clone(),
                fallback_element: if self.fallback.trim().is_empty() {
                    None
                } else {
                    Some(self.fallback.trim().to_string())
                },
            }),
            NodeKind::Wait => Some(StepType::Wait {
                seconds: self.seconds.max(1),
            }),
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
struct FlowEdge {
    from: u32,
    to: u32,
}

#[derive(Serialize, Deserialize)]
struct FlowDocument {
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
    next_id: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragMode {
    None,
    Node(u32),
    Pan,
    Connect(u32),
}

pub struct FlowEditor {
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
    next_id: u32,
    selected: Option<u32>,
    drag: DragMode,
    drag_offset: Vec2,
    pan: Vec2,
    last_pointer: Pos2,
    path: String,
    status: String,
    /// When set, main should hand steps to clicker and clear.
    pub pending_run: Option<Vec<WorkflowStep>>,
}

impl FlowEditor {
    pub fn new() -> Self {
        let mut ed = Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 1,
            selected: None,
            drag: DragMode::None,
            drag_offset: Vec2::ZERO,
            pan: Vec2::ZERO,
            last_pointer: Pos2::ZERO,
            path: String::new(),
            status: "从左侧添加节点，拖动连线端口连接流程".into(),
            pending_run: None,
        };
        ed.reset_default_graph();
        ed
    }

    /// Agent entrypoint: reset to seed graph.
    pub fn agent_reset(&mut self) {
        self.reset_default_graph();
    }

    /// Agent entrypoint: add node and return id.
    pub fn agent_add_node(&mut self, kind: NodeKind) -> u32 {
        let pos = Pos2::new(200.0 - self.pan.x, 160.0 - self.pan.y);
        let id = self.alloc_id();
        self.nodes.push(FlowNode::new(id, kind, pos));
        self.selected = Some(id);
        self.status = format!("已添加「{}」节点", kind.title());
        id
    }

    /// Agent entrypoint: connect nodes (from out -> to in).
    pub fn agent_connect(&mut self, from: u32, to: u32) {
        self.connect(from, to);
    }

    /// Agent entrypoint: load flow document or workflow txt.
    pub fn agent_load(&mut self, path: &str) -> Result<(), String> {
        self.load_flow(path)
    }

    /// Agent entrypoint: save flow json (+ companion txt).
    pub fn agent_save(&mut self, path: &str) -> Result<(), String> {
        self.save_flow(path)
    }

    /// Agent entrypoint: status string.
    pub fn status_text(&self) -> &str {
        &self.status
    }

    /// Agent entrypoint: get selected node id.
    #[allow(dead_code)]
    pub fn selected_node_id(&self) -> Option<u32> {
        self.selected
    }

    /// Agent entrypoint: list node ids/kinds for graph planning.
    pub fn agent_nodes_overview(&self) -> Vec<(u32, NodeKind)> {
        self.nodes.iter().map(|n| (n.id, n.kind)).collect()
    }

    /// Agent entrypoint: rebuild the whole graph from linear workflow steps.
    pub fn agent_build_from_steps(&mut self, steps: &[WorkflowStep]) {
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
                    fallback_element,
                } => {
                    let mut n = FlowNode::new(
                        self.alloc_id(),
                        NodeKind::Click,
                        Pos2::new(x, 180.0),
                    );
                    n.element_name = element_name.clone();
                    n.fallback = fallback_element.clone().unwrap_or_default();
                    n
                }
                StepType::Wait { seconds } => {
                    let mut n = FlowNode::new(
                        self.alloc_id(),
                        NodeKind::Wait,
                        Pos2::new(x, 180.0),
                    );
                    n.seconds = *seconds;
                    n
                }
                StepType::Pause { message } => {
                    let mut n = FlowNode::new(
                        self.alloc_id(),
                        NodeKind::Pause,
                        Pos2::new(x, 180.0),
                    );
                    n.message = message.clone();
                    n
                }
                StepType::Manual {
                    message,
                    instruction,
                } => {
                    let mut n = FlowNode::new(
                        self.alloc_id(),
                        NodeKind::Manual,
                        Pos2::new(x, 180.0),
                    );
                    n.message = message.clone();
                    n.instruction = instruction.clone().unwrap_or_default();
                    n
                }
            };
            let id = node.id;
            self.nodes.push(node);
            self.edges.push(FlowEdge { from: prev, to: id });
            prev = id;
            x += 220.0;
        }

        let end = FlowNode::new(self.alloc_id(), NodeKind::End, Pos2::new(x, 180.0));
        let end_id = end.id;
        self.nodes.push(end);
        self.edges.push(FlowEdge {
            from: prev,
            to: end_id,
        });

        self.selected = self.nodes.get(1).map(|n| n.id);
        self.status = format!("已按 {} 步重建流程图", steps.len());
    }

    fn reset_default_graph(&mut self) {
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
        self.edges.push(FlowEdge { from: sid, to: cid });
        self.edges.push(FlowEdge { from: cid, to: eid });
        self.selected = Some(cid);
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
        let Some(id) = self.selected else { return };
        if let Some(n) = self.node(id) {
            if n.kind == NodeKind::Start {
                self.status = "不能删除「开始」节点".into();
                return;
            }
        }
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
        self.selected = None;
        self.status = "已删除节点".into();
    }

    fn connect(&mut self, from: u32, to: u32) {
        if from == to {
            return;
        }
        let Some(a) = self.node(from) else { return };
        let Some(b) = self.node(to) else { return };
        if a.kind == NodeKind::End || b.kind == NodeKind::Start {
            self.status = "连线方向无效（结束不能引出，开始不能接入）".into();
            return;
        }
        // One outgoing edge per node (linear flow)
        self.edges.retain(|e| e.from != from);
        // One incoming for simplicity of linear chain (except allow replace)
        self.edges.retain(|e| e.to != to);
        self.edges.push(FlowEdge { from, to });
        self.status = format!("已连接 #{} → #{}", from, to);
    }

    /// Walk Start → … → End and collect executable steps.
    pub fn compile_steps(&self) -> Result<Vec<WorkflowStep>, String> {
        let start = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Start)
            .ok_or_else(|| "缺少开始节点".to_string())?;

        let mut by_from: HashMap<u32, u32> = HashMap::new();
        for e in &self.edges {
            by_from.insert(e.from, e.to);
        }

        let mut steps = Vec::new();
        let mut cur = start.id;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(cur) {
                return Err("检测到环路，请检查连线".into());
            }
            let node = self
                .node(cur)
                .ok_or_else(|| format!("节点 #{} 不存在", cur))?;
            if node.kind == NodeKind::End {
                break;
            }
            if let Some(st) = node.to_step() {
                steps.push(WorkflowStep::new(st));
            }
            match by_from.get(&cur) {
                Some(&next) => cur = next,
                None => {
                    if node.kind != NodeKind::End {
                        return Err(format!("节点「{}」没有连出线", node.kind.title()));
                    }
                    break;
                }
            }
        }

        if steps.is_empty() {
            return Err("流程中没有可执行步骤".into());
        }
        Ok(steps)
    }

    fn save_flow(&mut self, path: &str) -> Result<(), String> {
        let doc = FlowDocument {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            next_id: self.next_id,
        };
        let json = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())?;
        // Also write companion .txt for clicker compatibility
        let steps = self.compile_steps()?;
        let txt_path = if path.ends_with(".flow.json") {
            path.trim_end_matches(".flow.json").to_string() + ".txt"
        } else {
            format!("{}.txt", path)
        };
        std::fs::write(&txt_path, workflow::steps_to_text(&steps)).map_err(|e| e.to_string())?;
        self.path = path.to_string();
        Ok(())
    }

    fn load_flow(&mut self, path: &str) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if path.ends_with(".json") {
            let doc: FlowDocument =
                serde_json::from_str(&content).map_err(|e| e.to_string())?;
            self.nodes = doc.nodes;
            self.edges = doc.edges;
            self.next_id = doc.next_id;
            self.path = path.to_string();
            self.selected = None;
            Ok(())
        } else {
            // Import linear .txt as vertical chain
            let steps = workflow::parse_workflow_text(&content)?;
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
                        fallback_element,
                    } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::Click, Pos2::new(80.0, y));
                        n.element_name = element_name;
                        n.fallback = fallback_element.unwrap_or_default();
                        n
                    }
                    StepType::Wait { seconds } => {
                        let mut n =
                            FlowNode::new(self.alloc_id(), NodeKind::Wait, Pos2::new(80.0, y));
                        n.seconds = seconds;
                        n
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
                node.pos = [80.0 + (y - 80.0) * 0.0 + (self.nodes.len() as f32 - 1.0) * 180.0, 180.0];
                self.edges.push(FlowEdge { from: prev, to: id });
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
            self.edges.push(FlowEdge { from: prev, to: eid });
            self.nodes.push(end);
            self.path = path.to_string();
            Ok(())
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        // Left toolbox
        egui::SidePanel::left("flow_toolbox")
            .exact_width(150.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("节点库")
                        .strong()
                        .color(colors::TEXT),
                );
                ui.add_space(8.0);
                for (kind, hint) in [
                    (NodeKind::Click, "模板元素点击"),
                    (NodeKind::Wait, "延时等待"),
                    (NodeKind::Pause, "确认后继续"),
                    (NodeKind::Manual, "人工介入"),
                    (NodeKind::End, "流程结束"),
                ] {
                    let btn = egui::Button::new(
                        egui::RichText::new(kind.title()).color(Color32::WHITE),
                    )
                    .fill(kind.color())
                    .min_size(Vec2::new(130.0, 28.0));
                    if ui.add(btn).on_hover_text(hint).clicked() {
                        // Only one End recommended — still allow
                        self.add_node(kind);
                    }
                    ui.add_space(4.0);
                }
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                if ui.button("重置示例图").clicked() {
                    self.reset_default_graph();
                }
                if ui.button("删除选中  Del").clicked() {
                    self.delete_selected();
                }
            });

        // Right inspector
        egui::SidePanel::right("flow_inspector")
            .exact_width(220.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("属性")
                        .strong()
                        .color(colors::TEXT),
                );
                ui.add_space(8.0);
                if let Some(id) = self.selected {
                    if let Some(node) = self.node_mut(id) {
                        ui.label(format!("#{}  {}", node.id, node.kind.title()));
                        ui.add_space(6.0);
                        match node.kind {
                            NodeKind::Click => {
                                ui.label("元素名");
                                ui.text_edit_singleline(&mut node.element_name);
                                ui.add_space(4.0);
                                ui.label("回退元素（可选）");
                                ui.text_edit_singleline(&mut node.fallback);
                            }
                            NodeKind::Wait => {
                                ui.label("秒数");
                                ui.add(egui::DragValue::new(&mut node.seconds).range(1..=3600));
                            }
                            NodeKind::Pause => {
                                ui.label("提示消息");
                                ui.text_edit_multiline(&mut node.message);
                            }
                            NodeKind::Manual => {
                                ui.label("提示消息");
                                ui.text_edit_multiline(&mut node.message);
                                ui.label("操作说明");
                                ui.text_edit_multiline(&mut node.instruction);
                            }
                            NodeKind::Start | NodeKind::End => {
                                ui.label(
                                    egui::RichText::new("系统节点，无需配置")
                                        .color(colors::MUTED),
                                );
                            }
                        }
                    }
                } else {
                    ui.label(
                        egui::RichText::new("选中节点以编辑属性")
                            .color(colors::MUTED),
                    );
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(egui::RichText::new("文件").strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("打开").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Flow", &["flow.json", "json", "txt"])
                            .pick_file()
                        {
                            let p = path.to_string_lossy().to_string();
                            match self.load_flow(&p) {
                                Ok(()) => self.status = format!("已打开 {}", p),
                                Err(e) => self.status = format!("打开失败: {}", e),
                            }
                        }
                    }
                    if ui.button("保存").clicked() {
                        let default = if self.path.is_empty() {
                            "workflow.flow.json".to_string()
                        } else {
                            self.path.clone()
                        };
                        if let Some(path) = rfd::FileDialog::new()
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
                ui.add_space(8.0);
                let run = egui::Button::new(
                    egui::RichText::new("▶  运行流程").color(Color32::WHITE),
                )
                .fill(colors::ACCENT)
                .min_size(Vec2::new(190.0, 32.0));
                if ui.add(run).clicked() {
                    match self.compile_steps() {
                        Ok(steps) => {
                            self.status = format!("开始执行 {} 步…", steps.len());
                            self.pending_run = Some(steps);
                        }
                        Err(e) => self.status = format!("无法运行: {}", e),
                    }
                }
            });

        egui::TopBottomPanel::bottom("flow_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&self.status)
                        .size(12.0)
                        .color(colors::MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("拖动画布空白处平移 · 拖端口连线 · Delete 删除")
                            .size(11.0)
                            .color(colors::MUTED),
                    );
                });
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(colors::CANVAS))
            .show(ctx, |ui| {
                self.draw_canvas(ui);
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            self.delete_selected();
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
                Stroke::new(1.0, colors::GRID),
            );
            x += grid;
        }
        let mut y = canvas.top() + oy;
        while y < canvas.bottom() {
            painter.line_segment(
                [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
                Stroke::new(1.0, colors::GRID),
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
            let p0 = to_screen(a.out_port(), self.pan);
            let p1 = to_screen(b.in_port(), self.pan);
            draw_bezier(&painter, p0, p1, colors::WIRE, 2.0);
        }

        // Connecting preview
        if let DragMode::Connect(from) = self.drag {
            if let Some(a) = self.nodes.iter().find(|n| n.id == from) {
                let p0 = to_screen(a.out_port(), self.pan);
                let p1 = self.last_pointer;
                draw_bezier(&painter, p0, p1, colors::ACCENT_DIM, 2.0);
            }
        }

        // Pointer
        if let Some(pos) = response.interact_pointer_pos() {
            self.last_pointer = pos;
        }

        // Nodes hit-test (topmost first)
        let mut hit_node: Option<u32> = None;
        let mut hit_out: Option<u32> = None;
        let mut hit_in: Option<u32> = None;
        if let Some(pos) = response.interact_pointer_pos() {
            for n in self.nodes.iter().rev() {
                let r = n.rect().translate(self.pan + canvas.min.to_vec2());
                let out_c = to_screen(n.out_port(), self.pan);
                let in_c = to_screen(n.in_port(), self.pan);
                if out_c.distance(pos) <= PORT_R + 4.0 && n.kind != NodeKind::End {
                    hit_out = Some(n.id);
                    break;
                }
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
        if response.drag_started() {
            if let Some(id) = hit_out {
                self.drag = DragMode::Connect(id);
            } else if let Some(id) = hit_node {
                self.selected = Some(id);
                if let Some(n) = self.node(id) {
                    let origin = n.rect().min;
                    if let Some(pos) = response.interact_pointer_pos() {
                        let world = from_screen(pos, self.pan, canvas.min);
                        self.drag_offset = world - origin;
                    }
                }
                self.drag = DragMode::Node(id);
            } else {
                self.drag = DragMode::Pan;
                self.selected = None;
            }
        }

        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            match self.drag {
                DragMode::Node(id) => {
                    if let Some(n) = self.node_mut(id) {
                        n.pos[0] += delta.x;
                        n.pos[1] += delta.y;
                    }
                }
                DragMode::Pan => {
                    self.pan += delta;
                }
                DragMode::Connect(_) | DragMode::None => {}
            }
        }

        if response.drag_stopped() {
            if let DragMode::Connect(from) = self.drag {
                if let Some(to) = hit_in {
                    self.connect(from, to);
                } else if let Some(to) = hit_node {
                    if to != from {
                        self.connect(from, to);
                    }
                }
            }
            self.drag = DragMode::None;
        }

        if response.clicked() && !response.dragged() {
            if let Some(id) = hit_node.or(hit_out).or(hit_in) {
                self.selected = Some(id);
            } else {
                self.selected = None;
            }
        }

        // Draw nodes
        let selected = self.selected;
        for n in &self.nodes {
            let r = n.rect().translate(self.pan + canvas.min.to_vec2());
            let accent = n.kind.color();
            let selected_here = selected == Some(n.id);

            painter.rect_filled(r, 8.0, colors::NODE_BG);
            painter.rect_stroke(
                r,
                8.0,
                Stroke::new(
                    if selected_here { 2.5 } else { 1.0 },
                    if selected_here {
                        colors::NODE_SEL
                    } else {
                        accent
                    },
                ),
            );
            // Top accent bar
            let bar = Rect::from_min_max(
                r.min,
                Pos2::new(r.max.x, r.min.y + 6.0),
            );
            painter.rect_filled(bar, 8.0, accent);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(r.min.x, r.min.y + 3.0), Pos2::new(r.max.x, r.min.y + 6.0)),
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
                painter.circle_filled(c, PORT_R, colors::CANVAS);
                painter.circle_stroke(c, PORT_R, Stroke::new(2.0, accent));
            }
            if n.kind != NodeKind::End {
                let c = to_screen(n.out_port(), self.pan);
                painter.circle_filled(c, PORT_R, accent);
                painter.circle_stroke(c, PORT_R, Stroke::new(1.5, Color32::WHITE));
            }
        }

        // Title overlay
        painter.text(
            Pos2::new(canvas.left() + 16.0, canvas.top() + 16.0),
            egui::Align2::LEFT_TOP,
            "流程画布",
            FontId::proportional(13.0),
            Color32::from_rgb(148, 163, 184),
        );
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
    // Arrow head
    let dir = (p1 - c1).normalized();
    let orth = Vec2::new(-dir.y, dir.x);
    let tip = p1;
    let base = p1 - dir * 10.0;
    painter.line_segment([tip, base + orth * 5.0], Stroke::new(width, color));
    painter.line_segment([tip, base - orth * 5.0], Stroke::new(width, color));
}
