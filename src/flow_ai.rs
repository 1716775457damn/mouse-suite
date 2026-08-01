//! Generate a flow graph from natural language via configured AI (ccswitch/GLM/custom).

use crate::flow::FlowDocument;
use crate::flow_md::{self, MdFlowFile};
use crate::scribe_ai::{self, AiConfig};

const SYSTEM_PROMPT: &str = r#"你是 Mouse Suite 流程图生成器。根据用户的中文需求，输出**一个** JSON 对象（不要 Markdown 说明文字，不要 mermaid）。

JSON schema（mouse-suite-flow v1）:
{
  "version": 1,
  "title": "短标题",
  "nodes": [ FlowNode, ... ],
  "edges": [ { "from": id, "to": id, "branch": "main"|"true"|"false" }, ... ],
  "next_id": <最大id+1>
}

FlowNode 必填字段:
- id: number (从 1 起)
- kind: "Start"|"End"|"Click"|"Wait"|"TypeText"|"Pause"|"Manual"|"LoopStart"|"LoopEnd"|"IfVision"|"LoopWhile"
- pos: [x, y]  (可先给横向坐标，如 [60,180],[260,180],...)
- element_name: string  (Click/IfVision/LoopWhile 用；优先用用户提供的元素库名称)
- or_elements: []  (可选，额外 OR 模板名；任一匹配即成功/可点)
- fallback: ""  (旧字段，等同于 or_elements 里一项)
- threshold: 0.85
- pure_vision: false
- retries: 0
- retry_ms: 300
- on_fail: "skip"
- seconds: number  (Wait 秒数；LoopStart 表示循环次数)
- max_times: 50
- message / instruction: string
- type_text: string  (TypeText 的输入内容，可用 {Enter}{Tab} 等)
- type_interval_ms: 30

规则:
1. 必须有且仅有一个 Start、一个 End；边要连通。
2. 「成功点击 N 次 / 匹配到才算一次」: LoopStart(seconds=N) → IfVision —true→ Click → LoopEnd → End；IfVision 的 false **接回自身**（不占次数）。
3. 普通「循环 N 次每次都点」: LoopStart → Click → LoopEnd（无需 IfVision）。
4. 键盘输入用 TypeText；延时用 Wait。
5. IfVision 必须有 true 与 false 两条出边。
6. 只输出 JSON 对象本身。"#;

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
        user.push_str("元素库：暂无已录制元素，请自行起合理的 element_name（英文或拼音）。\n");
    } else {
        user.push_str("可用元素库名称（优先使用）：\n");
        for n in element_names {
            user.push_str("- ");
            user.push_str(n);
            user.push('\n');
        }
    }
    user.push_str("\n请输出 mouse-suite-flow JSON。");

    let raw = scribe_ai::chat_completion(cfg, SYSTEM_PROMPT, &user)?;
    let (title, description, doc) = parse_model_flow(&raw)?;
    Ok((title, description, doc))
}

fn parse_model_flow(raw: &str) -> Result<(String, String, FlowDocument), String> {
    let trimmed = raw.trim();
    // Full markdown export
    if trimmed.contains("```mouse-suite-flow") || trimmed.contains("```mermaid") {
        let (title, doc) = flow_md::import_markdown(trimmed)?;
        return Ok((title, "由 AI 根据自然语言生成".into(), doc));
    }

    let json = scribe_ai::extract_json_payload(trimmed);
    if let Ok(file) = serde_json::from_str::<MdFlowFile>(json) {
        let title = if file.title.trim().is_empty() {
            "AI 生成流程".into()
        } else {
            file.title.clone()
        };
        return Ok((title, "由 AI 根据自然语言生成".into(), file.into_document()));
    }
    if let Ok(doc) = serde_json::from_str::<FlowDocument>(json) {
        return Ok(("AI 生成流程".into(), "由 AI 根据自然语言生成".into(), doc));
    }

    // Last resort: wrap as markdown fence and import
    let wrapped = format!("# AI 生成\n\n```mouse-suite-flow\n{json}\n```\n");
    let (title, doc) = flow_md::import_markdown(&wrapped).map_err(|e| {
        format!("无法解析模型输出为流程图: {e}\n---\n{}", trimmed.chars().take(400).collect::<String>())
    })?;
    Ok((title, "由 AI 根据自然语言生成".into(), doc))
}
