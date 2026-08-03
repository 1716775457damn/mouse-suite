#![cfg_attr(windows, windows_subsystem = "windows")]

mod agent_bridge;
mod clicker;
mod common;
mod flow;
mod flow_ai;
mod flow_md;
mod hotkeys;
mod i18n;
mod interfaces;
mod mouse_hook;
mod recorder;
mod screen;
mod scribe;
mod scribe_ai;
mod theme;
mod workflow;

use agent_bridge::{AgentBridge, AgentCommand, AgentResponse};
use clicker::ClickerApp;
use common::{setup_chinese_fonts, Config};
use eframe::egui;
use flow::FlowEditor;
use flow::NodeKind;
use hotkeys::{HotkeyBus, HotkeyEvent};
use interfaces::{ClickerAgentInterface, FlowEditorAgentInterface, RecorderAgentInterface};
use mouse_hook::MouseHook;
use recorder::{setup_panic_hook, RecorderApp};
use scribe::ScribeApp;
use serde_json::{json, Value};
use std::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Recorder,
    Clicker,
    Flow,
    Scribe,
}

struct SuiteApp {
    tab: Tab,
    recorder: RecorderApp,
    clicker: ClickerApp,
    flow: FlowEditor,
    scribe: ScribeApp,
    bridge: AgentBridge,
    hotkeys: HotkeyBus,
}

impl SuiteApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_chinese_fonts(&cc.egui_ctx);
        let config = Config::load();
        i18n::set_lang(i18n::Lang::from_str(&config.language));
        theme::apply_theme_mode(
            &cc.egui_ctx,
            theme::ThemeMode::from_str(&config.theme),
        );
        let image_dir = config.image_dir();
        // One global grab for both element marquee + document click recording.
        let (rec_tx, rec_rx) = mpsc::channel();
        let (scribe_tx, scribe_rx) = mpsc::channel();
        let hook = MouseHook::start(rec_tx, scribe_tx);
        let recorder = RecorderApp::new(config, hook.recorder_flag.clone(), rec_rx);
        let clicker = ClickerApp::new(image_dir);
        let scribe = ScribeApp::new(hook.scribe_flag.clone(), hook.scribe_ignore.clone(), scribe_rx);
        Self {
            tab: Tab::Recorder,
            recorder,
            clicker,
            flow: FlowEditor::new(),
            scribe,
            bridge: AgentBridge::new(),
            hotkeys: HotkeyBus::spawn(),
        }
    }

    fn persist_prefs(&self) {
        let mut cfg = Config::load();
        cfg.language = i18n::lang().as_str().into();
        cfg.theme = theme::theme_mode().as_str().into();
        cfg.save();
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
            "loop" | "loop_start" => Some(NodeKind::LoopStart),
            "loop_end" | "endloop" => Some(NodeKind::LoopEnd),
            "if_vision" | "ifvision" => Some(NodeKind::IfVision),
            "loop_while" | "while_match" => Some(NodeKind::LoopWhile),
            "type" | "type_text" | "keys" | "keyboard" => Some(NodeKind::TypeText),
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
                    "tab": match self.tab {
                        Tab::Recorder => "recorder",
                        Tab::Clicker => "clicker",
                        Tab::Flow => "flow",
                        Tab::Scribe => "scribe",
                    },
                    "recorder_status": self.recorder.agent_status(),
                    "recorder_elements": self.recorder.agent_element_count(),
                    "clicker_status": self.clicker.agent_status(),
                    "clicker_threshold": self.clicker.match_threshold(),
                    "clicker_pure_vision": self.clicker.pure_vision(),
                    "clicker_retries": self.clicker.retries(),
                    "clicker_retry_ms": self.clicker.retry_ms(),
                    "clicker_on_fail": self.clicker.on_fail().as_str(),
                    "clicker_save_match_debug": self.clicker.save_match_debug(),
                    "recorder_hide_wait_ms": self.recorder.hide_wait_ms(),
                    "flow_status": self.flow.agent_status(),
                    "scribe_recording": self.scribe.is_recording(),
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
                    "scribe" | "docs" | "document" => Tab::Scribe,
                    _ => return make_err("tab must be recorder|clicker|flow|scribe".into()),
                };
                make_ok(format!("switched to {}", tab), None)
            }
            "ai_get_config" => {
                let cfg = scribe_ai::AiConfig::load();
                make_ok("ai config".into(), Some(cfg.public_view()))
            }
            "ai_set_config" => {
                let mut cfg = scribe_ai::AiConfig::load();
                cfg.apply_patch(&args);
                match cfg.save() {
                    Ok(()) => {
                        // Keep scribe UI in sync if it holds a copy.
                        self.scribe.reload_ai_config();
                        make_ok("ai config saved".into(), Some(cfg.public_view()))
                    }
                    Err(e) => make_err(e),
                }
            }
            "scribe_start" => match self.scribe.agent_start(ctx) {
                Ok(()) => {
                    self.tab = Tab::Scribe;
                    make_ok("scribe recording started".into(), None)
                }
                Err(e) => make_err(e),
            },
            "scribe_stop" => match self.scribe.agent_stop(ctx) {
                Ok(n) => {
                    self.tab = Tab::Scribe;
                    make_ok(
                        "scribe recording stopped".into(),
                        Some(json!({ "steps": n, "session": self.scribe.agent_session_id() })),
                    )
                }
                Err(e) => make_err(e),
            },
            "scribe_export_html" => {
                let Some(path) = Self::value_str(args, "path") else {
                    return make_err("missing args.path".into());
                };
                match self.scribe.agent_export_html(path) {
                    Ok(()) => make_ok("html exported".into(), Some(json!({ "path": path }))),
                    Err(e) => make_err(e),
                }
            }
            "scribe_to_flow" => {
                self.scribe.build_flow_draft();
                if let Some(steps) = self.scribe.take_pending_flow() {
                    self.flow.agent_build_from_steps(&steps);
                    self.flow.agent_auto_layout();
                    self.tab = Tab::Flow;
                    make_ok(
                        "flow draft built".into(),
                        Some(json!({ "nodes": steps.len() + 2 })),
                    )
                } else {
                    make_err("no session or empty steps".into())
                }
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
            "clicker_set_threshold" => {
                let Some(th) = args.get("threshold").and_then(|v| v.as_f64()) else {
                    return make_err("missing args.threshold".into());
                };
                self.clicker.agent_set_match_threshold(th as f32);
                make_ok("threshold set".into(), Some(json!({ "threshold": th })))
            }
            "clicker_set_pure_vision" => {
                let enabled = args
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                self.clicker.agent_set_pure_vision(enabled);
                make_ok(
                    "pure vision updated".into(),
                    Some(json!({ "pure_vision": enabled })),
                )
            }
            "clicker_set_retries" => {
                let Some(n) = Self::value_u64(args, "retries") else {
                    return make_err("missing args.retries".into());
                };
                self.clicker.agent_set_retries(n as u32);
                make_ok("retries set".into(), Some(json!({ "retries": n })))
            }
            "clicker_set_retry_ms" => {
                let Some(ms) = Self::value_u64(args, "ms") else {
                    return make_err("missing args.ms".into());
                };
                self.clicker.agent_set_retry_ms(ms);
                make_ok("retry_ms set".into(), Some(json!({ "retry_ms": ms })))
            }
            "clicker_set_on_fail" => {
                let Some(action) = Self::value_str(args, "action") else {
                    return make_err("missing args.action (skip|abort)".into());
                };
                match self.clicker.agent_set_on_fail(action) {
                    Ok(()) => make_ok("on_fail set".into(), Some(json!({ "on_fail": action }))),
                    Err(e) => make_err(e),
                }
            }
            "clicker_set_save_match_debug" => {
                let enabled = args
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                self.clicker.agent_set_save_match_debug(enabled);
                make_ok(
                    "save_match_debug updated".into(),
                    Some(json!({ "save_match_debug": enabled })),
                )
            }
            "recorder_set_hide_wait_ms" => {
                let Some(ms) = Self::value_u64(args, "ms") else {
                    return make_err("missing args.ms".into());
                };
                self.recorder.set_hide_wait_ms(ms);
                make_ok(
                    "hide_wait_ms set".into(),
                    Some(json!({ "hide_wait_ms": self.recorder.hide_wait_ms() })),
                )
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
                    return make_err(
                        "kind must be start|end|click|wait|type|pause|manual|loop|loop_end|if_vision|loop_while".into(),
                    );
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
                if let Some(branch) = Self::value_str(args, "branch") {
                    self.flow
                        .agent_connect_branch(from as u32, to as u32, branch);
                } else {
                    self.flow.agent_connect_nodes(from as u32, to as u32);
                }
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
                        make_ok(
                            "flow graph rebuilt from steps".into(),
                            Some(json!({ "steps": n })),
                        )
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
                        make_ok(
                            "flow graph rebuilt from text".into(),
                            Some(json!({ "steps": n })),
                        )
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
                    Ok(()) => {
                        self.tab = Tab::Flow;
                        make_ok("flow loaded".into(), None)
                    }
                    Err(e) => make_err(e),
                }
            }
            "flow_load_example" => {
                let name = Self::value_str(args, "name").unwrap_or("vision-click-10");
                let result = match name {
                    "vision-click-10" | "vision_click_10" | "default" => {
                        self.flow.load_example_vision_click_10()
                    }
                    other => Err(format!(
                        "unknown example: {other} (supported: vision-click-10)"
                    )),
                };
                match result {
                    Ok(()) => {
                        self.tab = Tab::Flow;
                        make_ok(format!("example loaded: {name}"), None)
                    }
                    Err(e) => make_err(e),
                }
            }
            "flow_ai_generate" => {
                let Some(prompt) = Self::value_str(args, "prompt") else {
                    return make_err("missing args.prompt".into());
                };
                let replace = args
                    .get("replace")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                match self.flow.agent_ai_generate(prompt, replace) {
                    Ok((title, nodes, edges)) => {
                        self.tab = Tab::Flow;
                        make_ok(
                            "flow generated".into(),
                            Some(json!({
                                "title": title,
                                "nodes": nodes,
                                "edges": edges,
                            })),
                        )
                    }
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
                            NodeKind::LoopStart => "loop",
                            NodeKind::LoopEnd => "loop_end",
                            NodeKind::IfVision => "if_vision",
                            NodeKind::LoopWhile => "loop_while",
                            NodeKind::TypeText => "type",
                        };
                        json!({ "id": id_num, "kind": kind_s })
                    })
                    .collect();
                make_ok("ok".into(), Some(json!({ "nodes": nodes })))
            }
            "flow_auto_layout" => {
                self.flow.agent_auto_layout();
                make_ok("flow auto-laid out".into(), None)
            }
            "flow_duplicate" => {
                let n = self.flow.agent_duplicate_selection();
                make_ok("selection duplicated".into(), Some(json!({ "pasted": n })))
            }
            "flow_undo" => {
                if self.flow.agent_undo() {
                    make_ok("undone".into(), None)
                } else {
                    make_err("nothing to undo".into())
                }
            }
            "flow_redo" => {
                if self.flow.agent_redo() {
                    make_ok("redone".into(), None)
                } else {
                    make_err("nothing to redo".into())
                }
            }
            _ => make_err(format!("unknown action: {}", action)),
        }
    }

    fn traffic_light(ui: &mut egui::Ui, color: egui::Color32, id: &str) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
        let center = rect.center();
        let edge = if response.hovered() {
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 74)
        } else {
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 36)
        };
        ui.painter().circle_filled(center, 6.0, color);
        ui.painter().circle_stroke(center, 6.0, egui::Stroke::new(0.8, edge));
        response.on_hover_text(id)
    }

    fn window_chrome(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("window_chrome")
            .exact_height(82.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::col().CHROME)
                    .inner_margin(egui::Margin::symmetric(16.0, 0.0)),
            )
            .show(ctx, |ui| {
                let full_rect = ui.max_rect();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let close =
                        Self::traffic_light(ui, theme::col().DANGER, i18n::t("app.chrome.close"));
                    ui.add_space(2.0);
                    let minimize = Self::traffic_light(
                        ui,
                        theme::col().WARN,
                        i18n::t("app.chrome.minimize"),
                    );
                    ui.add_space(2.0);
                    let maximize = Self::traffic_light(
                        ui,
                        theme::col().SUCCESS,
                        i18n::t("app.chrome.maximize"),
                    );
                    ui.add_space(14.0);
                    theme::brand_title(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.clicker.is_busy() {
                            theme::status_pill(
                                ui,
                                i18n::t("app.status.running"),
                                theme::StatusTone::Run,
                            );
                        } else if self.scribe.is_recording() {
                            theme::status_pill(
                                ui,
                                i18n::t("app.status.recording"),
                                theme::StatusTone::Danger,
                            );
                        } else {
                            theme::status_pill(
                                ui,
                                i18n::t("app.status.ready"),
                                theme::StatusTone::Idle,
                            );
                        }

                        ui.add_space(8.0);
                        let theme_label = match theme::theme_mode() {
                            theme::ThemeMode::Light => i18n::t("app.pref.theme_dark"),
                            theme::ThemeMode::Dark => i18n::t("app.pref.theme_light"),
                        };
                        if theme::quiet_button(ui, theme_label)
                            .on_hover_text(i18n::t("app.pref.theme"))
                            .clicked()
                        {
                            let next = theme::theme_mode().toggle();
                            theme::apply_theme_mode(ctx, next);
                            self.persist_prefs();
                        }
                        ui.add_space(4.0);
                        let lang_label = i18n::lang().toggle().label();
                        if theme::quiet_button(ui, lang_label)
                            .on_hover_text(i18n::t("app.pref.lang"))
                            .clicked()
                        {
                            i18n::set_lang(i18n::lang().toggle());
                            self.persist_prefs();
                        }
                    });

                    if close.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if minimize.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    if maximize.clicked() {
                        let maximized =
                            ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                });

                ui.add_space(5.0);
                ui.horizontal_centered(|ui| {
                    let selected = match self.tab {
                        Tab::Recorder => 0,
                        Tab::Clicker => 1,
                        Tab::Flow => 2,
                        Tab::Scribe => 3,
                    };
                    let tabs = [
                        i18n::t("app.tab.recorder"),
                        i18n::t("app.tab.clicker"),
                        i18n::t("app.tab.flow"),
                        i18n::t("app.tab.scribe"),
                    ];
                    if let Some(index) = theme::segmented_control(ui, &tabs, selected) {
                        self.tab = match index {
                            0 => Tab::Recorder,
                            1 => Tab::Clicker,
                            2 => Tab::Flow,
                            _ => Tab::Scribe,
                        };
                    }
                });

                // Leave room for traffic lights + brand (left) and status / prefs (right).
                let drag_rect = egui::Rect::from_min_max(
                    egui::pos2(full_rect.left() + 250.0, full_rect.top() + 4.0),
                    egui::pos2(full_rect.right() - 280.0, full_rect.top() + 38.0),
                );
                let drag = ui.interact(drag_rect, ui.id().with("window_drag"), egui::Sense::click_and_drag());
                if drag.double_clicked() {
                    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.painter().hline(
                    full_rect.x_range(),
                    full_rect.bottom() - 0.5,
                    egui::Stroke::new(1.0, theme::col().PANEL_EDGE),
                );
            });
    }
}

impl eframe::App for SuiteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Scale chrome / fonts / controls with window size (and system DPI via egui).
        theme::sync_ui_scale(ctx);

        if let Some(cmd) = self.bridge.poll() {
            let resp = self.handle_agent_command(ctx, cmd);
            let _ = self.bridge.write_response(&resp);
        }

        // Drain global hotkeys (Ctrl+Alt+F9 start / F10 stop / F8 scribe)
        while let Some(ev) = self.hotkeys.try_recv() {
            match ev {
                HotkeyEvent::Stop => {
                    self.clicker.agent_stop(ctx);
                    if self.scribe.is_recording() {
                        self.scribe.stop_recording(ctx);
                        self.tab = Tab::Scribe;
                    }
                }
                HotkeyEvent::Start => {
                    if !self.clicker.is_busy() {
                        match self.flow.agent_compile_steps() {
                            Ok(steps) => {
                                self.clicker.agent_set_workflow_steps(steps);
                                if let Err(e) = self.clicker.agent_start_workflow(ctx) {
                                    self.flow.set_run_highlight(None);
                                    let _ = e;
                                } else {
                                    self.tab = Tab::Flow;
                                }
                            }
                            Err(_) => {}
                        }
                    }
                }
                HotkeyEvent::ScribeToggle => {
                    self.tab = Tab::Scribe;
                    self.scribe.toggle_recording(ctx);
                }
            }
        }

        if let Some(steps) = self.scribe.take_pending_flow() {
            self.flow.agent_build_from_steps(&steps);
            self.flow.agent_auto_layout();
            self.tab = Tab::Flow;
        }

        self.recorder.tick_hide_then_capture(ctx);

        self.flow
            .set_element_catalog(self.recorder.element_catalog());
        self.flow
            .set_run_highlight(self.clicker.current_workflow_node());
        if self.clicker.is_busy() || self.clicker.should_show_run_hud() {
            ctx.request_repaint();
        }

        // Top-of-screen run HUD (main window stays minimized)
        if self.clicker.should_show_run_hud() {
            self.clicker.paint_run_hud(ctx);
        }

        if let Some(name) = self.flow.pending_screenshot.take() {
            self.recorder.start_named_capture_after_hide(ctx, name);
            self.tab = Tab::Recorder;
        }

        let capturing = self.recorder.is_capturing();

        if !capturing {
            self.window_chrome(ctx);
        }

        if let Some(steps) = self.flow.pending_run.take() {
            self.clicker.run_workflow_steps(ctx, steps);
            self.tab = Tab::Flow;
        }

        if capturing || self.tab == Tab::Recorder {
            self.recorder.ui(ctx);
        } else if self.tab == Tab::Clicker {
            self.clicker.ui(ctx);
        } else if self.tab == Tab::Scribe {
            self.scribe.ui(ctx);
        } else {
            self.flow.ui(ctx);
        }
    }
}

fn main() -> eframe::Result {
    setup_panic_hook();
    screen::enable_dpi_awareness();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            .with_decorations(false)
            .with_title("Mouse Suite"),
        ..Default::default()
    };
    eframe::run_native(
        "Mouse Suite",
        opts,
        Box::new(|cc| Ok(Box::new(SuiteApp::new(cc)))),
    )
}
