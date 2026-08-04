//! Shared workflow step types used by clicker execution and the flow editor.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickFailAction {
    /// Continue workflow without clicking (default for pure-vision miss).
    Skip,
    /// Stop the whole workflow.
    Abort,
}

impl ClickFailAction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "skip" | "continue" => Some(Self::Skip),
            "abort" | "stop" | "fail" => Some(Self::Abort),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Abort => "abort",
        }
    }
}

/// Primary + OR candidates (any match wins). Dedupes and skips empties.
pub fn merge_or_names(primary: &str, or_elements: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let p = primary.trim();
    if !p.is_empty() {
        out.push(p.to_string());
    }
    for e in or_elements {
        let t = e.trim();
        if !t.is_empty() && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// Split `a or b or c` into primary + extras.
fn split_or_names(main: &str) -> (String, Vec<String>) {
    let parts: Vec<String> = main
        .split(" or ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        (String::new(), Vec::new())
    } else {
        (parts[0].clone(), parts[1..].to_vec())
    }
}

fn format_or_names(primary: &str, or_elements: &[String]) -> String {
    let names = merge_or_names(primary, or_elements);
    names.join(" or ")
}

#[derive(Clone, Debug)]
pub enum StepType {
    Click {
        element_name: String,
        /// Additional templates: match **any** → click that one (true OR).
        or_elements: Vec<String>,
        /// Per-step match threshold; `None` uses clicker global.
        threshold: Option<f32>,
        /// Per-step pure-vision override; `None` uses clicker global.
        pure_vision: Option<bool>,
        /// Extra attempts after the first try; `None` uses clicker global.
        retries: Option<u32>,
        /// Delay between retries in ms; `None` uses clicker global.
        retry_ms: Option<u64>,
        /// What to do after all attempts (+ OR candidates) fail.
        on_fail: Option<ClickFailAction>,
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
    /// Type text / special keys into the focused control.
    /// Special tokens: `{Enter}` `{Tab}` `{Esc}` `{Backspace}` `{Delete}`
    /// `{Up}` `{Down}` `{Left}` `{Right}` `{Home}` `{End}` `{Space}`
    /// `{Ctrl+A}` `{Ctrl+C}` `{Ctrl+V}` `{Ctrl+X}` `{Ctrl+Z}` — use `{{` `}}` for literal braces.
    TypeText {
        text: String,
        /// Delay between keystrokes in ms; default 30.
        interval_ms: Option<u64>,
    },
    /// Marks the start of a repeatable body; pairs with `LoopEnd`.
    LoopStart {
        times: u32,
    },
    /// Conditional loop head: continue body while template matches (and under max_times).
    LoopWhileStart {
        element_name: String,
        or_elements: Vec<String>,
        threshold: Option<f32>,
        retries: Option<u32>,
        retry_ms: Option<u64>,
        max_times: u32,
    },
    LoopEnd,
    /// Vision-only branch (no click). Absolute PCs filled by graph compiler / text jumps.
    IfVision {
        element_name: String,
        or_elements: Vec<String>,
        threshold: Option<f32>,
        retries: Option<u32>,
        retry_ms: Option<u64>,
        then_jump: usize,
        else_jump: usize,
    },
    /// OCR text condition: true if needle is found on screen.
    IfText {
        needle: String,
        /// `contains` (default) or `exact`.
        match_exact: bool,
        case_sensitive: bool,
        retries: Option<u32>,
        retry_ms: Option<u64>,
        then_jump: usize,
        else_jump: usize,
    },
    /// OCR: find needle and click its bounding-box center.
    ClickText {
        needle: String,
        match_exact: bool,
        case_sensitive: bool,
        retries: Option<u32>,
        retry_ms: Option<u64>,
        on_fail: Option<ClickFailAction>,
    },
    /// Unconditional jump to absolute step index (compile artifact for branch joins).
    Goto {
        jump: usize,
    },
}

#[derive(Clone)]
pub struct WorkflowStep {
    pub step_type: StepType,
    pub executed: bool,
    /// Flow-graph node id this step was compiled from (for canvas highlight).
    pub source_node: Option<u32>,
}

impl WorkflowStep {
    pub fn new(step_type: StepType) -> Self {
        Self {
            step_type,
            executed: false,
            source_node: None,
        }
    }

    pub fn with_node(step_type: StepType, source_node: Option<u32>) -> Self {
        Self {
            step_type,
            executed: false,
            source_node,
        }
    }
}

pub fn parse_workflow_file(path: &str) -> Result<Vec<WorkflowStep>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read workflow file: {}", e))?;
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
            let step_type_str = step_type_str.trim().to_ascii_lowercase();
            let rest = rest.trim();

            let step_type = match step_type_str.as_str() {
                "click" => parse_click_line(rest),
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
                "type" | "type_text" | "keys" | "keyboard" => parse_type_line(rest),
                "loop" | "loop_start" => {
                    let times = rest.parse::<u32>().unwrap_or(2).max(1);
                    StepType::LoopStart { times }
                }
                "loop_while" | "while_match" => parse_loop_while_line(rest),
                "loop_end" | "endloop" => StepType::LoopEnd,
                "if_vision" | "ifvision" => parse_if_vision_line(rest),
                "if_text" | "iftext" | "ocr_if" => parse_if_text_line(rest),
                "click_text" | "clicktext" | "ocr_click" => parse_click_text_line(rest),
                "goto" | "jump" => {
                    let jump = rest.parse::<usize>().unwrap_or(0);
                    StepType::Goto { jump }
                }
                _ => continue,
            };

            steps.push(WorkflowStep::new(step_type));
        } else if line.eq_ignore_ascii_case("loop_end") || line.eq_ignore_ascii_case("endloop") {
            steps.push(WorkflowStep::new(StepType::LoopEnd));
        }
    }

    if steps.is_empty() {
        return Err("Workflow file is empty or invalid".to_string());
    }
    Ok(steps)
}

fn parse_type_line(rest: &str) -> StepType {
    // type: hello{Enter} | interval_ms=30
    let (main, params) = rest
        .split_once('|')
        .map(|(a, b)| (a.trim(), Some(b)))
        .unwrap_or((rest, None));
    let interval_ms = params
        .and_then(|p| extract_param(p, "interval_ms"))
        .and_then(|s| s.parse::<u64>().ok());
    StepType::TypeText {
        text: main.to_string(),
        interval_ms,
    }
}

fn parse_click_line(rest: &str) -> StepType {
    // click: a or b or c | threshold=0.85 | pure_vision=true | retries=2 | retry_ms=500 | on_fail=abort
    let (main, params) = rest
        .split_once('|')
        .map(|(a, b)| (a.trim(), Some(b)))
        .unwrap_or((rest, None));
    let (element_name, or_elements) = split_or_names(main);
    let threshold = params
        .and_then(|p| extract_param(p, "threshold"))
        .and_then(|s| s.parse::<f32>().ok());
    let pure_vision = params
        .and_then(|p| extract_param(p, "pure_vision"))
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"));
    let retries = params
        .and_then(|p| extract_param(p, "retries"))
        .and_then(|s| s.parse::<u32>().ok());
    let retry_ms = params
        .and_then(|p| extract_param(p, "retry_ms"))
        .and_then(|s| s.parse::<u64>().ok());
    let on_fail = params
        .and_then(|p| extract_param(p, "on_fail"))
        .and_then(|s| ClickFailAction::parse(&s));
    StepType::Click {
        element_name,
        or_elements,
        threshold,
        pure_vision,
        retries,
        retry_ms,
        on_fail,
    }
}

fn parse_if_vision_line(rest: &str) -> StepType {
    // if_vision: a or b | threshold=0.8 | retries=1 | retry_ms=500 | then=3 | else=5
    let (main, params) = rest
        .split_once('|')
        .map(|(a, b)| (a.trim(), Some(b)))
        .unwrap_or((rest, None));
    let (element_name, or_elements) = split_or_names(main);
    let threshold = params
        .and_then(|p| extract_param(p, "threshold"))
        .and_then(|s| s.parse::<f32>().ok());
    let retries = params
        .and_then(|p| extract_param(p, "retries"))
        .and_then(|s| s.parse::<u32>().ok());
    let retry_ms = params
        .and_then(|p| extract_param(p, "retry_ms"))
        .and_then(|s| s.parse::<u64>().ok());
    let then_jump = params
        .and_then(|p| extract_param(p, "then"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let else_jump = params
        .and_then(|p| extract_param(p, "else"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    StepType::IfVision {
        element_name,
        or_elements,
        threshold,
        retries,
        retry_ms,
        then_jump,
        else_jump,
    }
}

fn parse_text_match_flags(params: Option<&str>) -> (bool, bool) {
    let match_exact = params
        .and_then(|p| extract_param(p, "match"))
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "exact" | "eq" | "equals"))
        .or_else(|| {
            params
                .and_then(|p| extract_param(p, "exact"))
                .map(|s| matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        })
        .unwrap_or(false);
    let case_sensitive = params
        .and_then(|p| extract_param(p, "case_sensitive"))
        .or_else(|| params.and_then(|p| extract_param(p, "case")))
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "sensitive"))
        .unwrap_or(false);
    (match_exact, case_sensitive)
}

fn parse_if_text_line(rest: &str) -> StepType {
    // if_text: 确定 | match=contains | retries=1 | retry_ms=500 | then=3 | else=5
    let (main, params) = rest
        .split_once('|')
        .map(|(a, b)| (a.trim(), Some(b)))
        .unwrap_or((rest, None));
    let (match_exact, case_sensitive) = parse_text_match_flags(params);
    let retries = params
        .and_then(|p| extract_param(p, "retries"))
        .and_then(|s| s.parse::<u32>().ok());
    let retry_ms = params
        .and_then(|p| extract_param(p, "retry_ms"))
        .and_then(|s| s.parse::<u64>().ok());
    let then_jump = params
        .and_then(|p| extract_param(p, "then"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let else_jump = params
        .and_then(|p| extract_param(p, "else"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    StepType::IfText {
        needle: main.to_string(),
        match_exact,
        case_sensitive,
        retries,
        retry_ms,
        then_jump,
        else_jump,
    }
}

fn parse_click_text_line(rest: &str) -> StepType {
    // click_text: 确定 | match=exact | retries=1 | on_fail=skip
    let (main, params) = rest
        .split_once('|')
        .map(|(a, b)| (a.trim(), Some(b)))
        .unwrap_or((rest, None));
    let (match_exact, case_sensitive) = parse_text_match_flags(params);
    let retries = params
        .and_then(|p| extract_param(p, "retries"))
        .and_then(|s| s.parse::<u32>().ok());
    let retry_ms = params
        .and_then(|p| extract_param(p, "retry_ms"))
        .and_then(|s| s.parse::<u64>().ok());
    let on_fail = params
        .and_then(|p| extract_param(p, "on_fail"))
        .and_then(|s| ClickFailAction::parse(&s));
    StepType::ClickText {
        needle: main.to_string(),
        match_exact,
        case_sensitive,
        retries,
        retry_ms,
        on_fail,
    }
}

fn parse_loop_while_line(rest: &str) -> StepType {
    // loop_while: a or b | threshold=0.8 | max_times=50 | retries=0 | retry_ms=500
    let (main, params) = rest
        .split_once('|')
        .map(|(a, b)| (a.trim(), Some(b)))
        .unwrap_or((rest, None));
    let (element_name, or_elements) = split_or_names(main);
    let threshold = params
        .and_then(|p| extract_param(p, "threshold"))
        .and_then(|s| s.parse::<f32>().ok());
    let retries = params
        .and_then(|p| extract_param(p, "retries"))
        .and_then(|s| s.parse::<u32>().ok());
    let retry_ms = params
        .and_then(|p| extract_param(p, "retry_ms"))
        .and_then(|s| s.parse::<u64>().ok());
    let max_times = params
        .and_then(|p| extract_param(p, "max_times"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(50)
        .max(1);
    StepType::LoopWhileStart {
        element_name,
        or_elements,
        threshold,
        retries,
        retry_ms,
        max_times,
    }
}

pub fn steps_to_text(steps: &[WorkflowStep]) -> String {
    let mut out = String::from("# Mouse Suite workflow\n");
    for step in steps {
        match &step.step_type {
            StepType::Click {
                element_name,
                or_elements,
                threshold,
                pure_vision,
                retries,
                retry_ms,
                on_fail,
            } => {
                let mut line = format!("click: {}", format_or_names(element_name, or_elements));
                if let Some(t) = threshold {
                    line.push_str(&format!(" | threshold={}", t));
                }
                if let Some(pv) = pure_vision {
                    line.push_str(&format!(" | pure_vision={}", pv));
                }
                if let Some(r) = retries {
                    line.push_str(&format!(" | retries={}", r));
                }
                if let Some(ms) = retry_ms {
                    line.push_str(&format!(" | retry_ms={}", ms));
                }
                if let Some(f) = on_fail {
                    line.push_str(&format!(" | on_fail={}", f.as_str()));
                }
                out.push_str(&line);
                out.push('\n');
            }
            StepType::Wait { seconds } => {
                out.push_str(&format!("wait: {}\n", seconds));
            }
            StepType::TypeText { text, interval_ms } => {
                let mut line = format!("type: {}", text);
                if let Some(ms) = interval_ms {
                    line.push_str(&format!(" | interval_ms={}", ms));
                }
                out.push_str(&line);
                out.push('\n');
            }
            StepType::Pause { message } => {
                out.push_str(&format!("pause: {}\n", message));
            }
            StepType::Manual {
                message,
                instruction,
            } => {
                if let Some(inst) = instruction {
                    out.push_str(&format!("manual: {} | instruction={}\n", message, inst));
                } else {
                    out.push_str(&format!("manual: {}\n", message));
                }
            }
            StepType::LoopStart { times } => {
                out.push_str(&format!("loop: {}\n", times));
            }
            StepType::LoopWhileStart {
                element_name,
                or_elements,
                threshold,
                retries,
                retry_ms,
                max_times,
            } => {
                let mut line =
                    format!("loop_while: {}", format_or_names(element_name, or_elements));
                if let Some(t) = threshold {
                    line.push_str(&format!(" | threshold={}", t));
                }
                line.push_str(&format!(" | max_times={}", max_times));
                if let Some(r) = retries {
                    line.push_str(&format!(" | retries={}", r));
                }
                if let Some(ms) = retry_ms {
                    line.push_str(&format!(" | retry_ms={}", ms));
                }
                out.push_str(&line);
                out.push('\n');
            }
            StepType::LoopEnd => {
                out.push_str("loop_end:\n");
            }
            StepType::IfVision {
                element_name,
                or_elements,
                threshold,
                retries,
                retry_ms,
                then_jump,
                else_jump,
            } => {
                let mut line = format!("if_vision: {}", format_or_names(element_name, or_elements));
                if let Some(t) = threshold {
                    line.push_str(&format!(" | threshold={}", t));
                }
                if let Some(r) = retries {
                    line.push_str(&format!(" | retries={}", r));
                }
                if let Some(ms) = retry_ms {
                    line.push_str(&format!(" | retry_ms={}", ms));
                }
                line.push_str(&format!(" | then={} | else={}", then_jump, else_jump));
                out.push_str(&line);
                out.push('\n');
            }
            StepType::IfText {
                needle,
                match_exact,
                case_sensitive,
                retries,
                retry_ms,
                then_jump,
                else_jump,
            } => {
                let mut line = format!("if_text: {}", needle);
                if *match_exact {
                    line.push_str(" | match=exact");
                }
                if *case_sensitive {
                    line.push_str(" | case_sensitive=true");
                }
                if let Some(r) = retries {
                    line.push_str(&format!(" | retries={}", r));
                }
                if let Some(ms) = retry_ms {
                    line.push_str(&format!(" | retry_ms={}", ms));
                }
                line.push_str(&format!(" | then={} | else={}", then_jump, else_jump));
                out.push_str(&line);
                out.push('\n');
            }
            StepType::ClickText {
                needle,
                match_exact,
                case_sensitive,
                retries,
                retry_ms,
                on_fail,
            } => {
                let mut line = format!("click_text: {}", needle);
                if *match_exact {
                    line.push_str(" | match=exact");
                }
                if *case_sensitive {
                    line.push_str(" | case_sensitive=true");
                }
                if let Some(r) = retries {
                    line.push_str(&format!(" | retries={}", r));
                }
                if let Some(ms) = retry_ms {
                    line.push_str(&format!(" | retry_ms={}", ms));
                }
                if let Some(f) = on_fail {
                    line.push_str(&format!(" | on_fail={}", f.as_str()));
                }
                out.push_str(&line);
                out.push('\n');
            }
            StepType::Goto { jump } => {
                out.push_str(&format!("goto: {}\n", jump));
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
                let mut or_elements = json_or_elements(obj);
                if let Some(fb) = obj.get("fallback").and_then(|v| v.as_str()) {
                    let fb = fb.trim();
                    if !fb.is_empty() && !or_elements.iter().any(|x| x == fb) {
                        or_elements.insert(0, fb.to_string());
                    }
                }
                let threshold = obj
                    .get("threshold")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let pure_vision = obj.get("pure_vision").and_then(|v| v.as_bool());
                let retries = obj
                    .get("retries")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let retry_ms = obj.get("retry_ms").and_then(|v| v.as_u64());
                let on_fail = obj
                    .get("on_fail")
                    .and_then(|v| v.as_str())
                    .and_then(ClickFailAction::parse);
                WorkflowStep::new(StepType::Click {
                    element_name: element.to_string(),
                    or_elements,
                    threshold,
                    pure_vision,
                    retries,
                    retry_ms,
                    on_fail,
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
            "type" | "type_text" | "keys" | "keyboard" => {
                let text = obj
                    .get("text")
                    .or_else(|| obj.get("keys"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let interval_ms = obj.get("interval_ms").and_then(|v| v.as_u64());
                WorkflowStep::new(StepType::TypeText { text, interval_ms })
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
            "loop" | "loop_start" => {
                let times = obj
                    .get("times")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(2)
                    .max(1) as u32;
                WorkflowStep::new(StepType::LoopStart { times })
            }
            "loop_while" | "while_match" => {
                let element = obj
                    .get("element")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        format!("steps[{}].element is required for loop_while", idx)
                    })?;
                let threshold = obj
                    .get("threshold")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let retries = obj
                    .get("retries")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let retry_ms = obj.get("retry_ms").and_then(|v| v.as_u64());
                let max_times = obj
                    .get("max_times")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .max(1) as u32;
                WorkflowStep::new(StepType::LoopWhileStart {
                    element_name: element.to_string(),
                    or_elements: json_or_elements(obj),
                    threshold,
                    retries,
                    retry_ms,
                    max_times,
                })
            }
            "loop_end" | "endloop" => WorkflowStep::new(StepType::LoopEnd),
            "if_vision" | "ifvision" => {
                let element = obj
                    .get("element")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        format!("steps[{}].element is required for if_vision", idx)
                    })?;
                let threshold = obj
                    .get("threshold")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let retries = obj
                    .get("retries")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let retry_ms = obj.get("retry_ms").and_then(|v| v.as_u64());
                let then_jump = obj
                    .get("then")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let else_jump = obj
                    .get("else")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                WorkflowStep::new(StepType::IfVision {
                    element_name: element.to_string(),
                    or_elements: json_or_elements(obj),
                    threshold,
                    retries,
                    retry_ms,
                    then_jump,
                    else_jump,
                })
            }
            "if_text" | "iftext" | "ocr_if" => {
                let needle = obj
                    .get("needle")
                    .or_else(|| obj.get("text"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("steps[{}].needle is required for if_text", idx))?;
                let match_exact = obj
                    .get("match")
                    .and_then(|v| v.as_str())
                    .map(|s| matches!(s.to_ascii_lowercase().as_str(), "exact" | "eq" | "equals"))
                    .or_else(|| obj.get("match_exact").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
                let case_sensitive = obj
                    .get("case_sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let retries = obj
                    .get("retries")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let retry_ms = obj.get("retry_ms").and_then(|v| v.as_u64());
                let then_jump = obj.get("then").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let else_jump = obj.get("else").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                WorkflowStep::new(StepType::IfText {
                    needle: needle.to_string(),
                    match_exact,
                    case_sensitive,
                    retries,
                    retry_ms,
                    then_jump,
                    else_jump,
                })
            }
            "click_text" | "clicktext" | "ocr_click" => {
                let needle = obj
                    .get("needle")
                    .or_else(|| obj.get("text"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("steps[{}].needle is required for click_text", idx))?;
                let match_exact = obj
                    .get("match")
                    .and_then(|v| v.as_str())
                    .map(|s| matches!(s.to_ascii_lowercase().as_str(), "exact" | "eq" | "equals"))
                    .or_else(|| obj.get("match_exact").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
                let case_sensitive = obj
                    .get("case_sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let retries = obj
                    .get("retries")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let retry_ms = obj.get("retry_ms").and_then(|v| v.as_u64());
                let on_fail = obj
                    .get("on_fail")
                    .and_then(|v| v.as_str())
                    .and_then(ClickFailAction::parse);
                WorkflowStep::new(StepType::ClickText {
                    needle: needle.to_string(),
                    match_exact,
                    case_sensitive,
                    retries,
                    retry_ms,
                    on_fail,
                })
            }
            "goto" | "jump" => {
                let jump = obj.get("jump").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                WorkflowStep::new(StepType::Goto { jump })
            }
            _ => {
                return Err(format!(
                    "steps[{}].type unsupported: {} (use click|wait|type|pause|manual|loop|loop_while|loop_end|if_vision|if_text|click_text|goto)",
                    idx, t
                ))
            }
        };
        out.push(step);
    }

    Ok(out)
}

fn json_or_elements(obj: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = obj
        .get("or")
        .or_else(|| obj.get("or_elements"))
        .and_then(|v| v.as_array())
    {
        for v in arr {
            if let Some(s) = v.as_str() {
                let t = s.trim();
                if !t.is_empty() && !out.iter().any(|x| x == t) {
                    out.push(t.to_string());
                }
            }
        }
    } else if let Some(s) = obj.get("or").and_then(|v| v.as_str()) {
        for part in s.split(" or ") {
            let t = part.trim();
            if !t.is_empty() && !out.iter().any(|x| x == t) {
                out.push(t.to_string());
            }
        }
    }
    out
}
