//! Shared workflow step types used by clicker execution and the flow editor.
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum StepType {
    Click {
        element_name: String,
        fallback_element: Option<String>,
    },
    Pause {
        message: String,
    },
    Manual {
        message: String,
        instruction: Option<String>,
    },
    Wait {
        seconds: u32,
    },
}

#[derive(Clone)]
pub struct WorkflowStep {
    pub step_type: StepType,
    pub executed: bool,
}

impl WorkflowStep {
    pub fn new(step_type: StepType) -> Self {
        Self {
            step_type,
            executed: false,
        }
    }
}

pub fn parse_workflow_file(path: &str) -> Result<Vec<WorkflowStep>, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read workflow file: {}", e))?;
    parse_workflow_text(&content)
}

pub fn parse_workflow_text(content: &str) -> Result<Vec<WorkflowStep>, String> {
    let mut steps = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((step_type_str, rest)) = line.split_once(':') {
            let step_type_str = step_type_str.trim();
            let rest = rest.trim();

            let step_type = match step_type_str {
                "click" => {
                    let parts: Vec<&str> = rest.splitn(2, " or ").collect();
                    StepType::Click {
                        element_name: parts[0].trim().to_string(),
                        fallback_element: if parts.len() > 1 {
                            Some(parts[1].trim().to_string())
                        } else {
                            None
                        },
                    }
                }
                "pause" => StepType::Pause {
                    message: parse_pause_params(rest),
                },
                "manual" => {
                    let (message, instruction) = parse_manual_params(rest);
                    StepType::Manual {
                        message,
                        instruction,
                    }
                }
                "wait" => {
                    let seconds = rest.parse::<u32>().unwrap_or(1);
                    StepType::Wait { seconds }
                }
                _ => continue,
            };

            steps.push(WorkflowStep::new(step_type));
        }
    }

    if steps.is_empty() {
        return Err("Workflow file is empty or invalid".to_string());
    }
    Ok(steps)
}

pub fn steps_to_text(steps: &[WorkflowStep]) -> String {
    let mut out = String::from("# Mouse Suite workflow\n");
    for step in steps {
        match &step.step_type {
            StepType::Click {
                element_name,
                fallback_element,
            } => {
                if let Some(fb) = fallback_element {
                    out.push_str(&format!("click: {} or {}\n", element_name, fb));
                } else {
                    out.push_str(&format!("click: {}\n", element_name));
                }
            }
            StepType::Wait { seconds } => {
                out.push_str(&format!("wait: {}\n", seconds));
            }
            StepType::Pause { message } => {
                out.push_str(&format!("pause: {}\n", message));
            }
            StepType::Manual {
                message,
                instruction,
            } => {
                if let Some(inst) = instruction {
                    out.push_str(&format!(
                        "manual: {} | instruction={}\n",
                        message, inst
                    ));
                } else {
                    out.push_str(&format!("manual: {}\n", message));
                }
            }
        }
    }
    out
}

fn parse_pause_params(text: &str) -> String {
    if let Some((message, _params)) = text.split_once('|') {
        message.trim().to_string()
    } else {
        text.to_string()
    }
}

fn parse_manual_params(text: &str) -> (String, Option<String>) {
    if let Some((message, params)) = text.split_once('|') {
        let message = message.trim().to_string();
        let instruction = extract_param(params, "instruction");
        (message, instruction)
    } else {
        (text.to_string(), None)
    }
}

fn extract_param(params: &str, key: &str) -> Option<String> {
    for param in params.split('|') {
        let param = param.trim();
        if let Some((k, v)) = param.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Parse workflow steps from JSON array payload.
///
/// Supported schema:
/// [
///   {"type":"click","element":"btn_login","fallback":"btn_login_alt"},
///   {"type":"wait","seconds":2},
///   {"type":"pause","message":"确认后继续"},
///   {"type":"manual","message":"输入验证码","instruction":"在弹窗中输入6位码"}
/// ]
pub fn parse_steps_json(value: &Value) -> Result<Vec<WorkflowStep>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "steps must be an array".to_string())?;
    if arr.is_empty() {
        return Err("steps array is empty".to_string());
    }

    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("steps[{}] must be an object", idx))?;
        let t = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("steps[{}].type is required", idx))?
            .to_ascii_lowercase();

        let step = match t.as_str() {
            "click" => {
                let element = obj
                    .get("element")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("steps[{}].element is required for click", idx))?;
                let fallback = obj
                    .get("fallback")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                WorkflowStep::new(StepType::Click {
                    element_name: element.to_string(),
                    fallback_element: fallback,
                })
            }
            "wait" => {
                let seconds = obj
                    .get("seconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;
                WorkflowStep::new(StepType::Wait {
                    seconds: seconds.max(1),
                })
            }
            "pause" => {
                let message = obj
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("暂停")
                    .to_string();
                WorkflowStep::new(StepType::Pause { message })
            }
            "manual" => {
                let message = obj
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("人工操作")
                    .to_string();
                let instruction = obj
                    .get("instruction")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                WorkflowStep::new(StepType::Manual {
                    message,
                    instruction,
                })
            }
            _ => {
                return Err(format!(
                    "steps[{}].type unsupported: {} (use click|wait|pause|manual)",
                    idx, t
                ))
            }
        };
        out.push(step);
    }

    Ok(out)
}

