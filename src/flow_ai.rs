//! Generate a flow graph from natural language via configured AI (ccswitch/GLM/custom).

use crate::flow::FlowDocument;
use crate::flow_md::{self, MdFlowFile};
use crate::scribe_ai::{self, AiConfig};
use crate::skills;
use serde_json::{json, Value};

/// Generate a flow document from a natural-language prompt.
pub fn generate_flow_document(
    prompt: &str,
    element_names: &[String],
    cfg: &AiConfig,
) -> Result<(String, String, FlowDocument), String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("请先输入流程描述".into());
    }

    let mut user = String::from("用户需求：\n");
    user.push_str(prompt);
    user.push_str("\n\n");
    if element_names.is_empty() {
        user.push_str(
            "元素库：暂无已录制元素，请自行起合理的 element_name（英文或拼音）。\n",
        );
    } else {
        user.push_str("可用元素库名称（优先原样使用，不要改写）：\n");
        for n in element_names {
            user.push_str("- ");
            user.push_str(n);
            user.push('\n');
        }
    }
    user.push_str("\n先根据内置 Skills 选择 Pattern（A–F），再输出 mouse-suite-flow JSON 对象，不要其它文字。");

    let system = skills::flow_ai_system_prompt();
    let raw = scribe_ai::chat_completion(cfg, &system, &user)?;
    let (title, description, doc) = parse_model_flow(&raw)?;
    Ok((title, description, doc))
}

fn parse_model_flow(raw: &str) -> Result<(String, String, FlowDocument), String> {
    let trimmed = raw.trim();
    // Full markdown export
    if trimmed.contains("```mouse-suite-flow") || trimmed.contains("```mermaid") {
        if let Ok((title, doc)) = flow_md::import_markdown(trimmed) {
            return Ok((title, "由 AI 根据自然语言生成".into(), doc));
        }
    }

    let json = scribe_ai::extract_json_payload(trimmed);
    if let Ok((title, doc)) = try_parse_flow_json(json) {
        return Ok((title, "由 AI 根据自然语言生成".into(), doc));
    }

    // Normalize common model drift, then retry.
    if let Ok(normalized) = normalize_loose_json(json) {
        if let Ok((title, doc)) = try_parse_flow_json(&normalized) {
            return Ok((title, "由 AI 根据自然语言生成".into(), doc));
        }
        let wrapped = format!("# AI 生成\n\n```mouse-suite-flow\n{normalized}\n```\n");
        if let Ok((title, doc)) = flow_md::import_markdown(&wrapped) {
            return Ok((title, "由 AI 根据自然语言生成".into(), doc));
        }
    }

    let wrapped = format!("# AI 生成\n\n```mouse-suite-flow\n{json}\n```\n");
    let (title, doc) = flow_md::import_markdown(&wrapped).map_err(|e| {
        format!(
            "无法解析模型输出为流程图: {e}\n---\n{}",
            trimmed.chars().take(400).collect::<String>()
        )
    })?;
    Ok((title, "由 AI 根据自然语言生成".into(), doc))
}

fn try_parse_flow_json(json: &str) -> Result<(String, FlowDocument), String> {
    if let Ok(file) = serde_json::from_str::<MdFlowFile>(json) {
        let title = if file.title.trim().is_empty() {
            "AI 生成流程".into()
        } else {
            file.title.clone()
        };
        return Ok((title, file.into_document()));
    }
    if let Ok(doc) = serde_json::from_str::<FlowDocument>(json) {
        return Ok(("AI 生成流程".into(), doc));
    }
    Err("schema mismatch".into())
}

/// Coerce common LLM mistakes into mouse-suite-flow v1 JSON text.
fn normalize_loose_json(raw: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let obj = v
        .as_object_mut()
        .ok_or_else(|| "root is not object".to_string())?;

    // version may be "1" / "1.0"
    if let Some(ver) = obj.get("version").cloned() {
        obj.insert("version".into(), json!(as_u32(&ver).unwrap_or(1)));
    } else {
        obj.insert("version".into(), json!(1));
    }

    if !obj.contains_key("title") {
        if let Some(n) = obj.get("flow_name").and_then(|x| x.as_str()) {
            obj.insert("title".into(), json!(n));
        } else {
            obj.insert("title".into(), json!("AI 生成流程"));
        }
    }

    let nodes_in = obj.get("nodes").cloned().unwrap_or_else(|| json!([]));
    let nodes_arr = nodes_in
        .as_array()
        .ok_or_else(|| "nodes is not array".to_string())?;
    let mut nodes_out = Vec::with_capacity(nodes_arr.len());
    let mut max_id = 0u32;
    for (i, n) in nodes_arr.iter().enumerate() {
        let mut node = n.as_object().cloned().unwrap_or_default();
        let id = node.get("id").and_then(as_u32).unwrap_or((i as u32) + 1);
        max_id = max_id.max(id);
        node.insert("id".into(), json!(id));

        // kind may be lowercase
        if let Some(k) = node.get("kind").and_then(|x| x.as_str()) {
            let canon = canonicalize_kind(k);
            node.insert("kind".into(), json!(canon));
        }

        // pos may be {x,y} or missing
        if let Some(pos) = node.get("pos").cloned() {
            if let Some(arr) = pos.as_array() {
                if arr.len() >= 2 {
                    let x = as_f64(&arr[0]).unwrap_or(60.0 + (i as f64) * 200.0);
                    let y = as_f64(&arr[1]).unwrap_or(180.0);
                    node.insert("pos".into(), json!([x, y]));
                }
            } else if let Some(pobj) = pos.as_object() {
                let x = pobj.get("x").and_then(as_f64).unwrap_or(60.0);
                let y = pobj.get("y").and_then(as_f64).unwrap_or(180.0);
                node.insert("pos".into(), json!([x, y]));
            }
        } else {
            node.insert("pos".into(), json!([60.0 + (i as f64) * 200.0, 180.0]));
        }

        // payload → element_name / seconds / type_text
        if let Some(payload) = node.get("payload").cloned() {
            if let Some(p) = payload.as_object() {
                if !node.contains_key("element_name") || node["element_name"] == json!("") {
                    if let Some(name) = p
                        .get("element_name")
                        .or_else(|| p.get("label"))
                        .or_else(|| p.get("name"))
                        .or_else(|| p.get("selector"))
                        .and_then(|x| x.as_str())
                    {
                        if !name.is_empty() && name != "Start" && name != "End" {
                            node.insert("element_name".into(), json!(name));
                        }
                    }
                }
                if let Some(dur) = p.get("duration").or_else(|| p.get("ms")).and_then(as_f64) {
                    let secs = if dur >= 20.0 {
                        (dur / 1000.0).round().max(1.0) as u32
                    } else {
                        dur.max(1.0) as u32
                    };
                    node.insert("seconds".into(), json!(secs));
                }
                if let Some(t) = p
                    .get("text")
                    .or_else(|| p.get("type_text"))
                    .and_then(|x| x.as_str())
                {
                    node.insert("type_text".into(), json!(t));
                }
            }
            node.remove("payload");
        }

        if !node.contains_key("element_name") {
            node.insert("element_name".into(), json!(""));
        }
        nodes_out.push(Value::Object(node));
    }
    obj.insert("nodes".into(), Value::Array(nodes_out));

    // edges: [{from,to,branch}] or [[from,to]] or [{source,target}]
    let edges_in = obj.get("edges").cloned().unwrap_or_else(|| json!([]));
    let edges_arr = edges_in
        .as_array()
        .ok_or_else(|| "edges is not array".to_string())?;
    let mut edges_out = Vec::with_capacity(edges_arr.len());
    for e in edges_arr {
        if let Some(arr) = e.as_array() {
            if arr.len() >= 2 {
                let mut edge = serde_json::Map::new();
                edge.insert("from".into(), json!(as_u32(&arr[0]).unwrap_or(0)));
                edge.insert("to".into(), json!(as_u32(&arr[1]).unwrap_or(0)));
                let branch = arr.get(2).and_then(|x| x.as_str()).unwrap_or("main");
                edge.insert("branch".into(), json!(branch));
                edges_out.push(Value::Object(edge));
                continue;
            }
        }
        if let Some(eo) = e.as_object() {
            let mut edge = serde_json::Map::new();
            let from = eo
                .get("from")
                .or_else(|| eo.get("source"))
                .and_then(as_u32)
                .unwrap_or(0);
            let to = eo
                .get("to")
                .or_else(|| eo.get("target"))
                .and_then(as_u32)
                .unwrap_or(0);
            let branch = eo
                .get("branch")
                .or_else(|| eo.get("label"))
                .and_then(|x| x.as_str())
                .unwrap_or("main");
            edge.insert("from".into(), json!(from));
            edge.insert("to".into(), json!(to));
            edge.insert("branch".into(), json!(canonicalize_branch(branch)));
            edges_out.push(Value::Object(edge));
        }
    }
    obj.insert("edges".into(), Value::Array(edges_out));

    let next = obj
        .get("next_id")
        .and_then(as_u32)
        .unwrap_or(max_id.saturating_add(1));
    obj.insert("next_id".into(), json!(next.max(max_id.saturating_add(1))));

    serde_json::to_string(&v).map_err(|e| e.to_string())
}

fn canonicalize_kind(k: &str) -> String {
    match k.trim().to_ascii_lowercase().as_str() {
        "start" => "Start".into(),
        "end" => "End".into(),
        "click" => "Click".into(),
        "wait" | "delay" | "sleep" => "Wait".into(),
        "typetext" | "type" | "input" | "keyboard" => "TypeText".into(),
        "pause" => "Pause".into(),
        "manual" => "Manual".into(),
        "loopstart" | "loop_start" | "for" => "LoopStart".into(),
        "loopend" | "loop_end" => "LoopEnd".into(),
        "ifvision" | "if_vision" | "if" | "condition" => "IfVision".into(),
        "iftext" | "if_text" | "ocr_if" | "text_if" => "IfText".into(),
        "clicktext" | "click_text" | "ocr_click" | "text_click" => "ClickText".into(),
        "loopwhile" | "loop_while" | "while" => "LoopWhile".into(),
        _ => {
            // Preserve original casing if already canonical-ish
            let mut c = k.chars();
            match c.next() {
                None => "Click".into(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}

fn canonicalize_branch(b: &str) -> &'static str {
    match b.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "是" | "success" | "ok" => "true",
        "false" | "no" | "否" | "fail" | "failed" => "false",
        _ => "main",
    }
}

fn as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .map(|x| x as u32)
            .or_else(|| n.as_f64().map(|f| f.round() as u32)),
        Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f.round() as u32),
        _ => None,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_glm_style_payload_edges() {
        let raw = r#"{
          "version": "1.0",
          "title": "登录提交流程",
          "nodes": [
            {"id": "1", "kind": "Start", "pos": [100, 50]},
            {"id": "2", "kind": "Click", "pos": [100, 150], "payload": {"label": "btn_login"}},
            {"id": "3", "kind": "Wait", "pos": [100, 250], "payload": {"duration": 2000}},
            {"id": "4", "kind": "Click", "pos": [100, 350], "payload": {"label": "btn_submit"}},
            {"id": "5", "kind": "End", "pos": [100, 450]}
          ],
          "edges": [["1", "2"], ["2", "3"], ["3", "4"], ["4", "5"]],
          "next_id": 6
        }"#;
        let norm = normalize_loose_json(raw).unwrap();
        let (title, doc) = try_parse_flow_json(&norm).unwrap();
        assert!(title.contains("登录") || title.contains("流程") || !title.is_empty());
        assert_eq!(doc.nodes.len(), 5);
        assert_eq!(doc.edges.len(), 4);
        assert!(doc.nodes.iter().any(|n| n.element_name == "btn_login"));
        let wait = doc
            .nodes
            .iter()
            .find(|n| matches!(n.kind, crate::flow::NodeKind::Wait));
        assert!(wait.is_some());
        assert_eq!(wait.unwrap().seconds, 2);
    }
}
