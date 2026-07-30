#![cfg_attr(windows, windows_subsystem = "windows")]

mod agent_bridge;
mod clicker;
mod common;
mod flow;
mod interfaces;
mod recorder;
mod theme;
mod workflow;

use agent_bridge::{AgentBridge, AgentCommand, AgentResponse};
use clicker::ClickerApp;
use common::{setup_chinese_fonts, Config};
use eframe::egui;
use flow::FlowEditor;
use flow::NodeKind;
use interfaces::{ClickerAgentInterface, FlowEditorAgentInterface, RecorderAgentInterface};
use recorder::{setup_panic_hook, RecorderApp};
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Recorder,
    Clicker,
    Flow,
}

struct SuiteApp {
    tab: Tab,
    recorder: RecorderApp,
    clicker: ClickerApp,
    flow: FlowEditor,
    bridge: AgentBridge,
}

impl SuiteApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_chinese_fonts(&cc.egui_ctx);
        theme::apply_theme(&cc.egui_ctx);
        let config = Config::load();
        let image_dir = config.image_dir();
        let recorder = RecorderApp::new(config);
        let clicker = ClickerApp::new(image_dir);
        Self {
            tab: Tab::Recorder,
            recorder,
            clicker,
            flow: FlowEditor::new(),
            bridge: AgentBridge::new(),
        }
    }

    fn value_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
        args.get(key).and_then(|v| v.as_str())
    }

    fn value_u64(args: &Value, key: &str) -> Option<u64> {
        args.get(key).and_then(|v| v.as_u64())
    }

    fn parse_node_kind(s: &str) -> Option<NodeKind> {
        match s.to_ascii_lowercase().as_str() {
            "start" => Some(NodeKind::Start),
            "end" => Some(NodeKind::End),
            "click" => Some(NodeKind::Click),
            "wait" => Some(NodeKind::Wait),
            "pause" => Some(NodeKind::Pause),
            "manual" => Some(NodeKind::Manual),
            _ => None,
        }
    }

    fn handle_agent_command(&mut self, ctx: &egui::Context, cmd: AgentCommand) -> AgentResponse {
        let id = cmd.id.clone();
        let action = cmd.action.as_str();
        let args = &cmd.args;

        let make_ok = |message: String, data: Option<Value>| AgentResponse {
            id: id.clone(),
            ok: true,
            message,
            data,
        };
        let make_err = |message: String| AgentResponse {
            id: id.clone(),
            ok: false,
            message,
            data: None,
        };

        match action {
            "status" => make_ok(
                "ok".into(),
                Some(json!({
                    "tab": match self.tab { Tab::Recorder => "recorder", Tab::Clicker => "clicker", Tab::Flow => "flow" },
                    "recorder_status": self.recorder.agent_status(),
                    "recorder_elements": self.recorder.agent_element_count(),
                    "clicker_status": self.clicker.agent_status(),
                    "flow_status": self.flow.agent_status(),
                    "command_file": self.bridge.command_path(),
                })),
            ),
            "switch_tab" => {
                let Some(tab) = Self::value_str(args, "tab") else {
                    return make_err("missing args.tab".into());
                };
                self.tab = match tab {
                    "recorder" => Tab::Recorder,
                    "clicker" => Tab::Clicker,
                    "flow" => Tab::Flow,
                    _ => return make_err("tab must be recorder|clicker|flow".into()),
                };
                make_ok(format!("switched to {}", tab), None)
            }
            "recorder_refresh" => {
                self.recorder.agent_refresh_elements();
                make_ok("recorder refreshed".into(), None)
            }
            "recorder_export_csv" => {
                let name = Self::value_str(args, "name").unwrap_or("agent_export");
                match self.recorder.agent_export_templates_csv(name) {
                    Ok(path) => make_ok("csv exported".into(), Some(json!({ "path": path }))),
                    Err(e) => make_err(e),
                }
            }
            "clicker_set_delay" => {
                let Some(delay) = Self::value_u64(args, "ms") else {
                    return make_err("missing args.ms".into());
                };
                self.clicker.agent_set_delay_ms(delay);
                make_ok(format!("delay set to {}ms", delay), None)
            }
            "clicker_set_element_folder" => {
                let Some(folder) = Self::value_str(args, "path") else {
                    return make_err("missing args.path".into());
                };
                self.clicker.agent_set_element_folder(folder.to_string());
                make_ok("element folder updated".into(), None)
            }
            "clicker_load_workflow" => {
                let Some(path) = Self::value_str(args, "path") else {
                    return make_err("missing args.path".into());
                };
                match self.clicker.agent_load_workflow(path) {
                    Ok(n) => make_ok("workflow loaded".into(), Some(json!({ "steps": n }))),
                    Err(e) => make_err(e),
                }
            }
            "clicker_start_workflow" => match self.clicker.agent_start_workflow(ctx) {
                Ok(()) => make_ok("workflow started".into(), None),
                Err(e) => make_err(e),
            },
            "clicker_stop" => {
                self.clicker.agent_stop(ctx);
                make_ok("clicker stopped".into(), None)
            }
            "flow_reset" => {
                self.flow.agent_reset_flow();
                make_ok("flow reset".into(), None)
            }
            "flow_add_node" => {
                let Some(kind_s) = Self::value_str(args, "kind") else {
                    return make_err("missing args.kind".into());
                };
                let Some(kind) = Self::parse_node_kind(kind_s) else {
                    return make_err("kind must be start|end|click|wait|pause|manual".into());
                };
                let id_num = self.flow.agent_add_flow_node(kind);
                make_ok("node added".into(), Some(json!({ "id": id_num })))
            }
            "flow_connect" => {
                let Some(from) = Self::value_u64(args, "from") else {
                    return make_err("missing args.from".into());
                };
                let Some(to) = Self::value_u64(args, "to") else {
                    return make_err("missing args.to".into());
                };
                self.flow.agent_connect_nodes(from as u32, to as u32);
                make_ok("nodes connected".into(), None)
            }
            "flow_compile" => match self.flow.agent_compile_steps() {
                Ok(steps) => make_ok(
                    "flow compiled".into(),
                    Some(json!({ "steps": steps.len() })),
                ),
                Err(e) => make_err(e),
            },
            "flow_build_from_steps" => {
                let Some(steps_value) = args.get("steps") else {
                    return make_err("missing args.steps".into());
                };
                match workflow::parse_steps_json(steps_value) {
                    Ok(steps) => {
                        let n = steps.len();
                        self.flow.agent_build_flow_from_steps(&steps);
                        self.tab = Tab::Flow;
                        make_ok("flow graph rebuilt from steps".into(), Some(json!({ "steps": n })))
                    }
                    Err(e) => make_err(e),
                }
            }
            "flow_build_from_text" => {
                let Some(text) = Self::value_str(args, "text") else {
                    return make_err("missing args.text".into());
                };
                match workflow::parse_workflow_text(text) {
                    Ok(steps) => {
                        let n = steps.len();
                        self.flow.agent_build_flow_from_steps(&steps);
                        self.tab = Tab::Flow;
                        make_ok("flow graph rebuilt from text".into(), Some(json!({ "steps": n })))
                    }
                    Err(e) => make_err(e),
                }
            }
            "flow_run" => match self.flow.agent_compile_steps() {
                Ok(steps) => {
                    let n = steps.len();
                    self.clicker.agent_set_workflow_steps(steps);
                    match self.clicker.agent_start_workflow(ctx) {
                        Ok(()) => make_ok(
                            "flow compiled and started".into(),
                            Some(json!({ "steps": n })),
                        ),
                        Err(e) => make_err(e),
                    }
                }
                Err(e) => make_err(e),
            },
            "flow_load" => {
                let Some(path) = Self::value_str(args, "path") else {
                    return make_err("missing args.path".into());
                };
                match self.flow.agent_load_flow(path) {
                    Ok(()) => make_ok("flow loaded".into(), None),
                    Err(e) => make_err(e),
                }
            }
            "flow_save" => {
                let Some(path) = Self::value_str(args, "path") else {
                    return make_err("missing args.path".into());
                };
                match self.flow.agent_save_flow(path) {
                    Ok(()) => make_ok("flow saved".into(), None),
                    Err(e) => make_err(e),
                }
            }
            "flow_nodes" => {
                let nodes: Vec<Value> = self
                    .flow
                    .agent_nodes_overview()
                    .into_iter()
                    .map(|(id_num, kind)| {
                        let kind_s = match kind {
                            NodeKind::Start => "start",
                            NodeKind::End => "end",
                            NodeKind::Click => "click",
                            NodeKind::Wait => "wait",
                            NodeKind::Pause => "pause",
                            NodeKind::Manual => "manual",
                        };
                        json!({ "id": id_num, "kind": kind_s })
                    })
                    .collect();
                make_ok("ok".into(), Some(json!({ "nodes": nodes })))
            }
            _ => make_err(format!("unknown action: {}", action)),
        }
    }
}

impl eframe::App for SuiteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(cmd) = self.bridge.poll() {
            let resp = self.handle_agent_command(ctx, cmd);
            let _ = self.bridge.write_response(&resp);
        }

        let capturing = self.recorder.is_capturing();

        if !capturing {
            egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Recorder, "录制");
                    ui.selectable_value(&mut self.tab, Tab::Clicker, "点击");
                    ui.selectable_value(&mut self.tab, Tab::Flow, "流程图");
                });
            });
        }

        if let Some(steps) = self.flow.pending_run.take() {
            self.clicker.run_workflow_steps(ctx, steps);
            self.tab = Tab::Clicker;
        }

        if capturing || self.tab == Tab::Recorder {
            self.recorder.ui(ctx);
        } else if self.tab == Tab::Clicker {
            self.clicker.ui(ctx);
        } else {
            self.flow.ui(ctx);
        }
    }
}

fn main() -> eframe::Result {
    setup_panic_hook();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 550.0])
            .with_title("Mouse Suite"),
        ..Default::default()
    };
    eframe::run_native(
        "Mouse Suite",
        opts,
        Box::new(|cc| Ok(Box::new(SuiteApp::new(cc)))),
    )
}
