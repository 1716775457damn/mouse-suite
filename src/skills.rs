//! Built-in agent skills for more accurate AI flow generation.
//!
//! Defaults are embedded in the binary. Optional overrides:
//! `{exe_dir}/data/skills/<id>/SKILL.md`

use crate::common::data_dir;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Skill {
    pub id: String,
    pub title: String,
    pub body: String,
}

const EMBEDDED: &[(&str, &str, &str)] = &[
    (
        "flow-generator",
        "Flow Generator",
        include_str!("../skills/flow-generator/SKILL.md"),
    ),
    (
        "flow-patterns",
        "Flow Patterns",
        include_str!("../skills/flow-patterns/SKILL.md"),
    ),
];

fn skills_root() -> PathBuf {
    data_dir().join("skills")
}

/// Ensure default skill files exist under `data/skills/` (user-editable overlay).
pub fn ensure_bundled_skills_on_disk() {
    let root = skills_root();
    for (id, _title, body) in EMBEDDED {
        let dir = root.join(id);
        let path = dir.join("SKILL.md");
        if path.is_file() {
            continue;
        }
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(&path, body);
    }
}

fn parse_title(body: &str, fallback: &str) -> String {
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return title.to_string();
            }
        }
    }
    fallback.to_string()
}

fn strip_md_fences_for_prompt(body: &str) -> String {
    // Keep fenced JSON examples — models need them. Just normalize line endings.
    body.replace("\r\n", "\n")
}

/// Load skills: disk overlay wins when present, else embedded.
pub fn load_flow_skills() -> Vec<Skill> {
    ensure_bundled_skills_on_disk();
    let mut out = Vec::with_capacity(EMBEDDED.len());
    for (id, title, embedded) in EMBEDDED {
        let disk = skills_root().join(id).join("SKILL.md");
        let body = fs::read_to_string(&disk).unwrap_or_else(|_| (*embedded).to_string());
        let title = parse_title(&body, title);
        out.push(Skill {
            id: (*id).to_string(),
            title,
            body: strip_md_fences_for_prompt(&body),
        });
    }
    out
}

/// Compact schema + role header prepended before skills.
const BASE_HEADER: &str = r#"你是 Mouse Suite 流程图生成器。根据用户需求，输出**一个** JSON 对象。
禁止输出 Markdown 代码围栏包裹之外的解释（若示例里出现围栏，最终答案仍必须是纯 JSON 对象本身）。
禁止 mermaid。只输出纯 JSON。

严格 schema（字段名必须一致；数字字段不要用字符串）：
{
  "version": 1,
  "title": "短标题",
  "nodes": [
    {
      "id": 1,
      "kind": "Start",
      "pos": [60, 180],
      "element_name": "",
      "or_elements": [],
      "fallback": "",
      "threshold": 0.85,
      "pure_vision": false,
      "retries": 0,
      "retry_ms": 300,
      "on_fail": "skip",
      "seconds": 1,
      "max_times": 50,
      "message": "",
      "instruction": "",
      "type_text": "",
      "type_interval_ms": 30
    }
  ],
  "edges": [ { "from": 1, "to": 2, "branch": "main" } ],
  "next_id": 3
}

kind 只能是:
"Start"|"End"|"Click"|"Wait"|"TypeText"|"Pause"|"Manual"|"LoopStart"|"LoopEnd"|"IfVision"|"LoopWhile"
"#;

/// Full system prompt for flow AI (header + built-in skills).
pub fn flow_ai_system_prompt() -> String {
    let skills = load_flow_skills();
    let mut out = String::with_capacity(12_000);
    out.push_str(BASE_HEADER);
    out.push_str("\n\n# 内置 Skills（生成时必须遵守）\n");
    out.push_str("下列 skills 已内置在软件中；按用户意图选择正确 Pattern，并遵守字段语义。\n");
    for (i, skill) in skills.iter().enumerate() {
        out.push_str(&format!(
            "\n---------- Skill {} · {} ({}) ----------\n",
            i + 1,
            skill.title,
            skill.id
        ));
        out.push_str(skill.body.trim());
        out.push('\n');
    }
    out.push_str(
        "\n最终提醒：只输出 JSON 对象本身；IfVision 必须有 true/false 两边；Wait.seconds 是秒；LoopStart.seconds 是次数。\n",
    );
    out
}

/// Short label for UI status (how many skills loaded).
pub fn flow_skills_status_line() -> String {
    let n = load_flow_skills().len();
    format!("内置 Skills ×{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skills_non_empty() {
        let skills = load_flow_skills();
        assert!(skills.len() >= 2);
        let prompt = flow_ai_system_prompt();
        assert!(prompt.contains("Pattern B"));
        assert!(prompt.contains("LoopWhile"));
        assert!(prompt.contains("mouse-suite") || prompt.contains("Mouse Suite"));
    }
}
