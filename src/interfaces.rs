#![allow(dead_code)]
//! Stable agent-facing interfaces for each major feature module.
//! Keep these traits backward-compatible so external automation can rely on them.

use crate::clicker::ClickerApp;
use crate::flow::{FlowEditor, NodeKind};
use crate::recorder::RecorderApp;
use crate::workflow::WorkflowStep;
use eframe::egui;

pub trait RecorderAgentInterface {
    fn agent_status(&self) -> &str;
    fn agent_element_count(&self) -> usize;
    fn agent_refresh_elements(&mut self);
    fn agent_start_new_element_capture(&mut self, ctx: &egui::Context);
    fn agent_start_add_state_capture(&mut self, ctx: &egui::Context, forced_state: Option<String>);
    fn agent_export_templates_csv(&mut self, name: &str) -> Result<String, String>;
}

impl RecorderAgentInterface for RecorderApp {
    fn agent_status(&self) -> &str {
        RecorderApp::status_text(self)
    }

    fn agent_element_count(&self) -> usize {
        RecorderApp::element_count(self)
    }

    fn agent_refresh_elements(&mut self) {
        RecorderApp::agent_refresh(self);
    }

    fn agent_start_new_element_capture(&mut self, ctx: &egui::Context) {
        RecorderApp::agent_start_new_capture(self, ctx);
    }

    fn agent_start_add_state_capture(&mut self, ctx: &egui::Context, forced_state: Option<String>) {
        RecorderApp::agent_start_add_state_capture(self, ctx, forced_state);
    }

    fn agent_export_templates_csv(&mut self, name: &str) -> Result<String, String> {
        RecorderApp::agent_export_csv(self, name)
    }
}

pub trait ClickerAgentInterface {
    fn agent_status(&self) -> &'static str;
    fn agent_logs(&self) -> Vec<String>;
    fn agent_set_delay_ms(&mut self, delay_ms: u64);
    fn agent_set_element_folder(&mut self, folder: String);
    fn agent_set_match_threshold(&mut self, threshold: f32);
    fn agent_set_pure_vision(&mut self, enabled: bool);
    fn agent_set_retries(&mut self, retries: u32);
    fn agent_set_retry_ms(&mut self, retry_ms: u64);
    fn agent_set_on_fail(&mut self, on_fail: &str) -> Result<(), String>;
    fn agent_set_save_match_debug(&mut self, enabled: bool);
    fn agent_stop(&mut self, ctx: &egui::Context);
    fn agent_load_workflow(&mut self, path: &str) -> Result<usize, String>;
    fn agent_set_workflow_steps(&mut self, steps: Vec<WorkflowStep>);
    fn agent_start_workflow(&mut self, ctx: &egui::Context) -> Result<(), String>;
    fn agent_load_csv(&mut self, path: &str) -> Result<usize, String>;
    fn agent_start_csv(&mut self, ctx: &egui::Context) -> Result<(), String>;
}

impl ClickerAgentInterface for ClickerApp {
    fn agent_status(&self) -> &'static str {
        ClickerApp::status_text(self)
    }

    fn agent_logs(&self) -> Vec<String> {
        ClickerApp::logs_snapshot(self)
    }

    fn agent_set_delay_ms(&mut self, delay_ms: u64) {
        ClickerApp::set_delay_ms(self, delay_ms);
    }

    fn agent_set_element_folder(&mut self, folder: String) {
        ClickerApp::set_element_folder(self, folder);
    }

    fn agent_set_match_threshold(&mut self, threshold: f32) {
        ClickerApp::set_match_threshold(self, threshold);
    }

    fn agent_set_pure_vision(&mut self, enabled: bool) {
        ClickerApp::set_pure_vision(self, enabled);
    }

    fn agent_set_retries(&mut self, retries: u32) {
        ClickerApp::set_retries(self, retries);
    }

    fn agent_set_retry_ms(&mut self, retry_ms: u64) {
        ClickerApp::set_retry_ms(self, retry_ms);
    }

    fn agent_set_on_fail(&mut self, on_fail: &str) -> Result<(), String> {
        let action = crate::workflow::ClickFailAction::parse(on_fail)
            .ok_or_else(|| "on_fail must be skip|abort".to_string())?;
        ClickerApp::set_on_fail(self, action);
        Ok(())
    }

    fn agent_set_save_match_debug(&mut self, enabled: bool) {
        ClickerApp::set_save_match_debug(self, enabled);
    }

    fn agent_stop(&mut self, ctx: &egui::Context) {
        ClickerApp::stop(self, ctx);
    }

    fn agent_load_workflow(&mut self, path: &str) -> Result<usize, String> {
        ClickerApp::agent_load_workflow_file(self, path)
    }

    fn agent_set_workflow_steps(&mut self, steps: Vec<WorkflowStep>) {
        ClickerApp::agent_set_workflow_steps(self, steps);
    }

    fn agent_start_workflow(&mut self, ctx: &egui::Context) -> Result<(), String> {
        ClickerApp::agent_start_workflow(self, ctx)
    }

    fn agent_load_csv(&mut self, path: &str) -> Result<usize, String> {
        ClickerApp::agent_load_csv_file(self, path)
    }

    fn agent_start_csv(&mut self, ctx: &egui::Context) -> Result<(), String> {
        ClickerApp::agent_start_csv(self, ctx)
    }
}

pub trait FlowEditorAgentInterface {
    fn agent_status(&self) -> &str;
    fn agent_reset_flow(&mut self);
    fn agent_add_flow_node(&mut self, kind: NodeKind) -> u32;
    fn agent_connect_nodes(&mut self, from: u32, to: u32);
    fn agent_nodes_overview(&self) -> Vec<(u32, NodeKind)>;
    fn agent_build_flow_from_steps(&mut self, steps: &[WorkflowStep]);
    fn agent_compile_steps(&self) -> Result<Vec<WorkflowStep>, String>;
    fn agent_load_flow(&mut self, path: &str) -> Result<(), String>;
    fn agent_save_flow(&mut self, path: &str) -> Result<(), String>;
    fn agent_auto_layout(&mut self);
    fn agent_duplicate_selection(&mut self) -> usize;
    fn agent_undo(&mut self) -> bool;
    fn agent_redo(&mut self) -> bool;
}

impl FlowEditorAgentInterface for FlowEditor {
    fn agent_status(&self) -> &str {
        FlowEditor::status_text(self)
    }

    fn agent_reset_flow(&mut self) {
        FlowEditor::agent_reset(self);
    }

    fn agent_add_flow_node(&mut self, kind: NodeKind) -> u32 {
        FlowEditor::agent_add_node(self, kind)
    }

    fn agent_connect_nodes(&mut self, from: u32, to: u32) {
        FlowEditor::agent_connect(self, from, to);
    }

    fn agent_nodes_overview(&self) -> Vec<(u32, NodeKind)> {
        FlowEditor::agent_nodes_overview(self)
    }

    fn agent_build_flow_from_steps(&mut self, steps: &[WorkflowStep]) {
        FlowEditor::agent_build_from_steps(self, steps);
    }

    fn agent_compile_steps(&self) -> Result<Vec<WorkflowStep>, String> {
        FlowEditor::compile_steps(self)
    }

    fn agent_load_flow(&mut self, path: &str) -> Result<(), String> {
        FlowEditor::agent_load(self, path)
    }

    fn agent_save_flow(&mut self, path: &str) -> Result<(), String> {
        FlowEditor::agent_save(self, path)
    }

    fn agent_auto_layout(&mut self) {
        FlowEditor::agent_auto_layout(self);
    }

    fn agent_duplicate_selection(&mut self) -> usize {
        FlowEditor::agent_duplicate_selection(self)
    }

    fn agent_undo(&mut self) -> bool {
        FlowEditor::agent_undo(self)
    }

    fn agent_redo(&mut self) -> bool {
        FlowEditor::agent_redo(self)
    }
}
