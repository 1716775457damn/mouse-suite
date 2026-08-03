//! Markdown flowchart import/export for Agent-friendly round-trips.
//!
//! Format:
//! - `# Title` + blockquote description
//! - ```mermaid``` diagram (human-readable)
//! - ```mouse-suite-flow``` JSON (exact graph: nodes/edges/next_id)

use crate::flow::{EdgeBranch, FlowDocument, FlowEdge, FlowNode, NodeKind};
use eframe::egui::Pos2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct MdFlowFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub title: String,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub next_id: u32,
}

fn default_version() -> u32 {
    1
}

impl MdFlowFile {
    pub fn from_document(title: &str, doc: FlowDocument) -> Self {
        Self {
            version: 1,
            title: title.to_string(),
            nodes: doc.nodes,
            edges: doc.edges,
            next_id: doc.next_id,
        }
    }

    pub fn into_document(self) -> FlowDocument {
        FlowDocument {
            nodes: self.nodes,
            edges: self.edges,
            next_id: self.next_id,
        }
    }
}

/// Export graph to Markdown (Mermaid + mouse-suite-flow JSON).
pub fn export_markdown(
    title: &str,
    description: &str,
    doc: &FlowDocument,
) -> Result<String, String> {
    let title = if title.trim().is_empty() {
        "未命名流程"
    } else {
        title.trim()
    };
    let desc = if description.trim().is_empty() {
        "mouse-suite-flow v1"
    } else {
        description.trim()
    };

    let mermaid = to_mermaid(doc);
    let payload = MdFlowFile::from_document(title, doc.clone());
    let json = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;

    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    for line in desc.lines() {
        out.push_str(&format!("> {line}\n"));
    }
    out.push('\n');
    out.push_str("```mermaid\n");
    out.push_str(&mermaid);
    if !mermaid.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n\n");
    out.push_str("```mouse-suite-flow\n");
    out.push_str(&json);
    if !json.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
    Ok(out)
}

/// Parse Markdown: prefer ```mouse-suite-flow``` JSON; else best-effort Mermaid.
pub fn import_markdown(md: &str) -> Result<(String, FlowDocument), String> {
    if let Some(json) = extract_fence(md, "mouse-suite-flow") {
        let file: MdFlowFile = serde_json::from_str(json.trim())
            .map_err(|e| format!("mouse-suite-flow JSON 解析失败: {e}"))?;
        let title = if file.title.trim().is_empty() {
            extract_title(md).unwrap_or_else(|| "未命名流程".into())
        } else {
            file.title.clone()
        };
        return Ok((title, file.into_document()));
    }
    if let Some(mermaid) = extract_fence(md, "mermaid") {
        let doc = parse_mermaid_flowchart(mermaid)?;
        let title = extract_title(md).unwrap_or_else(|| "未命名流程".into());
        return Ok((title, doc));
    }
    Err("未找到 ```mouse-suite-flow``` 或 ```mermaid``` 代码块".into())
}

fn extract_title(md: &str) -> Option<String> {
    for line in md.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

fn extract_fence<'a>(md: &'a str, lang: &str) -> Option<&'a str> {
    let open = format!("```{lang}");
    let start = md.find(&open)?;
    let after = &md[start + open.len()..];
    let after = after.strip_prefix('\r').unwrap_or(after);
    let after = after.strip_prefix('\n').unwrap_or(after);
    let end = after.find("```")?;
    Some(&after[..end])
}

fn to_mermaid(doc: &FlowDocument) -> String {
    let mut s = String::from("flowchart LR\n");
    for n in &doc.nodes {
        let label = mermaid_label(n);
        let shape = match n.kind {
            NodeKind::IfVision => format!("  n{}{{{}}}", n.id, label),
            NodeKind::Start | NodeKind::End => format!("  n{}([{}])", n.id, label),
            _ => format!("  n{}[\"{}\"]", n.id, label),
        };
        s.push_str(&shape);
        s.push('\n');
    }
    for e in &doc.edges {
        let edge = match e.branch {
            EdgeBranch::True => format!("  n{} -->|是| n{}\n", e.from, e.to),
            EdgeBranch::False => format!("  n{} -->|否| n{}\n", e.from, e.to),
            EdgeBranch::Main => format!("  n{} --> n{}\n", e.from, e.to),
        };
        s.push_str(&edge);
    }
    s
}

fn mermaid_label(n: &FlowNode) -> String {
    let raw = match n.kind {
        NodeKind::Start => "开始".into(),
        NodeKind::End => "结束".into(),
        NodeKind::LoopStart => format!("循环 x{}", n.seconds.max(1)),
        NodeKind::LoopEnd => "循环结束".into(),
        NodeKind::IfVision => format!("{}?", n.element_name),
        NodeKind::Click => format!("点击 {}", n.element_name),
        NodeKind::TypeText => {
            let t: String = n.type_text.chars().take(12).collect();
            format!("输入 {t}")
        }
        NodeKind::Wait => format!("等待 {}s", n.seconds),
        NodeKind::Pause => "暂停".into(),
        NodeKind::Manual => "人工".into(),
        NodeKind::LoopWhile => format!("条件循环 {}", n.element_name),
    };
    escape_mermaid(&raw)
}

fn escape_mermaid(s: &str) -> String {
    s.replace('\\', "/")
        .replace('"', "'")
        .replace('[', "(")
        .replace(']', ")")
        .replace('{', "(")
        .replace('}', ")")
        .replace('|', "/")
        .replace('\n', " ")
}

/// Best-effort Mermaid flowchart parser (simple nodes + edges).
fn parse_mermaid_flowchart(src: &str) -> Result<FlowDocument, String> {
    let mut labels: HashMap<String, String> = HashMap::new();
    let mut edge_list: Vec<(String, String, EdgeBranch)> = Vec::new();

    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("flowchart")
            || line.starts_with("graph")
            || line.starts_with("%%")
        {
            continue;
        }
        if let Some((id, label)) = parse_node_decl(line) {
            labels.insert(id, label);
            continue;
        }
        if let Some((from, to, branch)) = parse_edge(line) {
            labels.entry(from.clone()).or_default();
            labels.entry(to.clone()).or_default();
            edge_list.push((from, to, branch));
        }
    }

    if labels.is_empty() {
        return Err("Mermaid 中未解析到节点".into());
    }

    let mut ids: Vec<String> = labels.keys().cloned().collect();
    ids.sort();

    let mut id_map: HashMap<String, u32> = HashMap::new();
    let mut nodes = Vec::new();
    let mut next_id = 1u32;
    let mut x = 60.0f32;
    for key in &ids {
        let label = labels.get(key).cloned().unwrap_or_default();
        let kind = kind_from_label(&label);
        let mut n = FlowNode::new(next_id, kind, Pos2::new(x, 180.0));
        apply_label_props(&mut n, &label);
        id_map.insert(key.clone(), next_id);
        nodes.push(n);
        next_id += 1;
        x += 200.0;
    }

    let mut flow_edges = Vec::new();
    for (from, to, branch) in edge_list {
        let Some(&f) = id_map.get(&from) else {
            continue;
        };
        let Some(&t) = id_map.get(&to) else {
            continue;
        };
        flow_edges.push(FlowEdge {
            from: f,
            to: t,
            branch,
        });
    }

    Ok(FlowDocument {
        nodes,
        edges: flow_edges,
        next_id,
    })
}

fn parse_node_decl(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.contains("-->") {
        return None;
    }
    let id_end = line.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')?;
    if id_end == 0 {
        return None;
    }
    let id = line[..id_end].to_string();
    let rest = &line[id_end..];
    let label = if let Some(inner) = rest.strip_prefix("[\"") {
        let end = inner.find("\"]")?;
        inner[..end].to_string()
    } else if let Some(inner) = rest.strip_prefix("([") {
        let end = inner.find("])")?;
        inner[..end].to_string()
    } else if let Some(inner) = rest.strip_prefix('[') {
        let end = inner.find(']')?;
        inner[..end].trim_matches('"').to_string()
    } else if let Some(inner) = rest.strip_prefix('{') {
        let end = inner.find('}')?;
        inner[..end].to_string()
    } else if let Some(inner) = rest.strip_prefix('(') {
        let end = inner.find(')')?;
        inner[..end].to_string()
    } else {
        return None;
    };
    Some((id, label))
}

fn parse_edge(line: &str) -> Option<(String, String, EdgeBranch)> {
    let line = line.trim();
    if !line.contains("-->") {
        return None;
    }
    let (left, right) = line.split_once("-->")?;
    let from = left
        .trim()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()?
        .to_string();
    if from.is_empty() {
        return None;
    }
    let right = right.trim();
    let (branch, rest) = if let Some(rest) = right.strip_prefix('|') {
        let end = rest.find('|')?;
        let label = rest[..end].trim();
        let branch = match label {
            "是" | "true" | "yes" | "Y" | "y" => EdgeBranch::True,
            "否" | "false" | "no" | "N" | "n" => EdgeBranch::False,
            _ => EdgeBranch::Main,
        };
        (branch, rest[end + 1..].trim())
    } else {
        (EdgeBranch::Main, right)
    };
    let to = rest
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()?
        .to_string();
    if to.is_empty() {
        return None;
    }
    Some((from, to, branch))
}

fn kind_from_label(label: &str) -> NodeKind {
    let l = label.to_ascii_lowercase();
    let c = label;
    if c.contains("开始") && !c.contains("循环") {
        NodeKind::Start
    } else if c.contains("结束") && !c.contains("循环") {
        NodeKind::End
    } else if c.contains("循环结束") || c.contains("回到循环") {
        NodeKind::LoopEnd
    } else if c.contains("循环开始")
        || c.contains("循环 x")
        || c.contains("循环×")
        || c.contains("循环 X")
    {
        NodeKind::LoopStart
    } else if c.contains("条件循环") {
        NodeKind::LoopWhile
    } else if c.contains('?') || c.contains("视觉") {
        NodeKind::IfVision
    } else if c.contains("点击") || l.contains("click") {
        NodeKind::Click
    } else if c.contains("输入") || c.contains("键盘") || l.contains("type") {
        NodeKind::TypeText
    } else if c.contains("等待") || l.contains("wait") {
        NodeKind::Wait
    } else if c.contains("暂停") {
        NodeKind::Pause
    } else if c.contains("人工") {
        NodeKind::Manual
    } else if c.contains("循环") {
        NodeKind::LoopStart
    } else {
        NodeKind::Manual
    }
}

fn apply_label_props(n: &mut FlowNode, label: &str) {
    match n.kind {
        NodeKind::LoopStart => {
            if let Some(num) = label
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
            {
                n.seconds = num.max(1);
            }
        }
        NodeKind::IfVision | NodeKind::Click | NodeKind::LoopWhile => {
            let name = label
                .replace('?', "")
                .replace("点击", "")
                .replace("视觉条件", "")
                .replace("条件循环", "")
                .trim()
                .to_string();
            if !name.is_empty() {
                n.element_name = name;
            }
        }
        NodeKind::TypeText => {
            let t = label.replace("输入", "").trim().to_string();
            if !t.is_empty() {
                n.type_text = t;
            }
        }
        NodeKind::Wait => {
            if let Some(num) = label
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
            {
                n.seconds = num.max(1);
            }
        }
        _ => {}
    }
}

/// Embedded example: vision match then click, 10 successful iterations.
pub const EXAMPLE_VISION_CLICK_10_MD: &str =
    include_str!("../workflows/examples/vision-click-10.md");
