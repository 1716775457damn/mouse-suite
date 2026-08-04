//! 文档页 AI 步骤说明（OpenAI 兼容 / 智谱 GLM / 本地代理）。

use crate::common::data_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::path::Path;

/// App-level AI config (clickscribe-compatible schema).
const CONFIG_NAME: &str = "ai_config.json";
/// Legacy path — migrated once on load.
const LEGACY_CONFIG_NAME: &str = "scribe_ai.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Ccswitch,
    Glm,
    Custom,
}

impl Default for AiProvider {
    fn default() -> Self {
        Self::Ccswitch
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub provider: AiProvider,
    #[serde(default)]
    pub glm_key: String,
    #[serde(default)]
    pub custom_base: String,
    #[serde(default)]
    pub custom_key: String,
    #[serde(default)]
    pub custom_model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: AiProvider::Ccswitch,
            glm_key: String::new(),
            custom_base: String::new(),
            custom_key: String::new(),
            custom_model: String::new(),
        }
    }
}

impl AiConfig {
    pub fn path() -> std::path::PathBuf {
        data_dir().join(CONFIG_NAME)
    }

    pub fn load() -> Self {
        let _ = fs::create_dir_all(data_dir());
        let p = Self::path();
        if let Ok(s) = fs::read_to_string(&p) {
            if let Ok(cfg) = serde_json::from_str(&s) {
                return cfg;
            }
        }
        // Migrate legacy scribe_ai.json → ai_config.json (once).
        let legacy = data_dir().join(LEGACY_CONFIG_NAME);
        if let Ok(s) = fs::read_to_string(&legacy) {
            if let Ok(cfg) = serde_json::from_str::<Self>(&s) {
                let _ = cfg.save();
                return cfg;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let _ = fs::create_dir_all(data_dir());
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(Self::path(), json).map_err(|e| e.to_string())
    }

    /// Masked view for Agent / UI (never returns raw keys).
    pub fn public_view(&self) -> serde_json::Value {
        serde_json::json!({
            "provider": match self.provider {
                AiProvider::Ccswitch => "ccswitch",
                AiProvider::Glm => "glm",
                AiProvider::Custom => "custom",
            },
            "glm_key_set": !self.glm_key.trim().is_empty(),
            "glm_key_mask": mask_key(&self.glm_key),
            "custom_base": self.custom_base,
            "custom_key_set": !self.custom_key.trim().is_empty(),
            "custom_key_mask": mask_key(&self.custom_key),
            "custom_model": self.custom_model,
            "ccswitch_url": "http://127.0.0.1:15721/v1/messages",
        })
    }

    /// Merge patch like clickscribe: empty keys do not overwrite existing.
    pub fn apply_patch(&mut self, patch: &serde_json::Value) {
        if let Some(p) = patch.get("provider").and_then(|v| v.as_str()) {
            self.provider = match p.to_ascii_lowercase().as_str() {
                "glm" => AiProvider::Glm,
                "custom" => AiProvider::Custom,
                _ => AiProvider::Ccswitch,
            };
        }
        if let Some(k) = patch.get("glm_key").and_then(|v| v.as_str()) {
            if !k.is_empty() {
                self.glm_key = k.to_string();
            }
        }
        if let Some(b) = patch.get("custom_base").and_then(|v| v.as_str()) {
            self.custom_base = b.to_string();
        }
        if let Some(k) = patch.get("custom_key").and_then(|v| v.as_str()) {
            if !k.is_empty() {
                self.custom_key = k.to_string();
            }
        }
        if let Some(m) = patch.get("custom_model").and_then(|v| v.as_str()) {
            self.custom_model = m.to_string();
        }
    }

    pub fn endpoint(&self) -> Result<(String, String, String), String> {
        match self.provider {
            AiProvider::Glm => {
                if self.glm_key.trim().is_empty() {
                    return Err("请填写智谱 API Key".into());
                }
                Ok((
                    "https://open.bigmodel.cn/api/paas/v4/chat/completions".into(),
                    self.glm_key.trim().to_string(),
                    "glm-4v-plus".into(),
                ))
            }
            AiProvider::Custom => {
                let url = normalize_chat_url(&self.custom_base);
                if url.is_empty() {
                    return Err("请填写自定义 Base URL".into());
                }
                let model = if self.custom_model.trim().is_empty() {
                    "gpt-4o".into()
                } else {
                    self.custom_model.trim().to_string()
                };
                Ok((url, self.custom_key.trim().to_string(), model))
            }
            // CC Switch local proxy (Anthropic Messages). OpenAI /v1/chat on this port is Codex path.
            AiProvider::Ccswitch => Ok((
                "http://127.0.0.1:15721/v1/messages".into(),
                String::new(),
                "claude-haiku-4-5".into(),
            )),
        }
    }

    pub fn is_ccswitch(&self) -> bool {
        matches!(self.provider, AiProvider::Ccswitch)
    }
}

fn mask_key(key: &str) -> String {
    let t = key.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() <= 8 {
        return "****".into();
    }
    format!("{}…{}", &t[..4], &t[t.len() - 4..])
}

fn normalize_chat_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return String::new();
    }
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }
    if base.ends_with("/v1") {
        return format!("{base}/chat/completions");
    }
    if base.contains("/v1/") {
        return format!("{base}/chat/completions");
    }
    format!("{base}/v1/chat/completions")
}

fn encode_jpeg_b64(path: &Path, max_side: u32) -> Result<String, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let mut rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let m = w.max(h);
    if m > max_side {
        let s = max_side as f32 / m as f32;
        let nw = ((w as f32) * s).round().max(1.0) as u32;
        let nh = ((h as f32) * s).round().max(1.0) as u32;
        rgb = image::imageops::resize(&rgb, nw, nh, image::imageops::FilterType::Triangle);
    }
    let mut buf = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        buf,
    ))
}

fn parse_descriptions(text: &str, n: usize) -> Vec<String> {
    let text = text.trim();
    let json_slice = if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            &text[start..=end]
        } else {
            text
        }
    } else {
        text
    };
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(json_slice) {
        let mut out: Vec<String> = arr
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();
        while out.len() < n {
            out.push(String::new());
        }
        return out.into_iter().take(n).collect();
    }
    let mut lines: Vec<String> = text
        .lines()
        .map(|ln| {
            let t = ln.trim();
            let t = t.trim_start_matches(|c: char| c.is_ascii_digit() || "、.）) ".contains(c));
            t.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    while lines.len() < n {
        lines.push(String::new());
    }
    lines.truncate(n);
    lines
}

/// Text chat completion. CC Switch uses Anthropic Messages; GLM/custom use OpenAI chat.
pub fn chat_completion(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    if cfg.is_ccswitch() {
        return chat_completion_anthropic(cfg, system, user);
    }
    chat_completion_openai(cfg, system, user)
}

fn chat_completion_anthropic(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    let (url, _key, model) = cfg.endpoint()?;
    let mut payload = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "temperature": 0.2,
        "messages": [{ "role": "user", "content": user }],
    });
    if !system.trim().is_empty() {
        payload["system"] = serde_json::json!(system);
    }

    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(180))
        .send_json(payload)
        .map_err(|e| {
            format!("AI 请求失败: {e}（请确认 CC Switch 本地代理已开 :15721，顶部 Proxy 为绿色）")
        })?;
    let status = resp.status();
    let body = resp.into_string().map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            body.chars().take(240).collect::<String>()
        ));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let text = anthropic_text(&v);
    if text.trim().is_empty() {
        return Err("模型返回空内容".into());
    }
    Ok(text)
}

fn anthropic_text(v: &serde_json::Value) -> String {
    if let Some(arr) = v["content"].as_array() {
        let mut out = String::new();
        for block in arr {
            if block["type"].as_str() == Some("text") {
                if let Some(t) = block["text"].as_str() {
                    out.push_str(t);
                }
            }
        }
        return out;
    }
    v["content"].as_str().unwrap_or("").to_string()
}

fn chat_completion_openai(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    let (url, key, model) = cfg.endpoint()?;
    let mut messages = Vec::new();
    if !system.trim().is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": system
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": user
    }));

    let payload = serde_json::json!({
        "model": model,
        "stream": false,
        "messages": messages,
        "max_tokens": 4096,
        "temperature": 0.2,
    });

    let mut req = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(180));
    if !key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }

    let resp = req
        .send_json(payload)
        .map_err(|e| format!("AI 请求失败: {e}（请检查 AI 设置 / 网络）"))?;
    let status = resp.status();
    let body = resp.into_string().map_err(|e| e.to_string())?;
    if status == 429 || status >= 500 {
        return Err(format!(
            "上游错误 {status}: {}",
            body.chars().take(160).collect::<String>()
        ));
    }
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }

    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if text.trim().is_empty() {
        return Err("模型返回空内容".into());
    }
    Ok(text)
}

/// Extract JSON object/array from model text (raw or fenced).
pub fn extract_json_payload(text: &str) -> &str {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("```") {
        let rest = rest
            .strip_prefix("json")
            .or_else(|| rest.strip_prefix("mouse-suite-flow"))
            .unwrap_or(rest);
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(end) = rest.find("```") {
            return rest[..end].trim();
        }
    }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                return &text[start..=end];
            }
        }
    }
    text
}

/// 一次把全部步骤截图发给视觉模型，返回每步说明。
pub fn describe_all(paths: &[std::path::PathBuf], cfg: &AiConfig) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let prompt = format!(
        "这是按顺序的一组电脑操作截图，共 {} 步，每张图的红圈（若有）标注了该步的点击位置。\
         请结合整个流程的上下文，为每一步用一句简洁中文说明「这步做了什么」\
         （不超过 30 字，不要序号、不要引号）。\
         只返回一个 JSON 字符串数组，例如 [\"第一步说明\",\"第二步说明\"]，不要任何其他文字。",
        paths.len()
    );
    let text = if cfg.is_ccswitch() {
        describe_all_anthropic(paths, cfg, &prompt)?
    } else {
        describe_all_openai(paths, cfg, &prompt)?
    };
    if text.trim().is_empty() {
        return Err("模型返回空内容".into());
    }
    Ok(parse_descriptions(&text, paths.len()))
}

fn describe_all_anthropic(
    paths: &[std::path::PathBuf],
    cfg: &AiConfig,
    prompt: &str,
) -> Result<String, String> {
    let (url, _key, model) = cfg.endpoint()?;
    let mut content = vec![serde_json::json!({"type": "text", "text": prompt})];
    for p in paths {
        let b64 = encode_jpeg_b64(p, 1024)?;
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/jpeg",
                "data": b64
            }
        }));
    }
    let payload = serde_json::json!({
        "model": model,
        "max_tokens": (600i64).max(paths.len() as i64 * 80),
        "temperature": 0.3,
        "messages": [{ "role": "user", "content": content }],
    });
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(180))
        .send_json(payload)
        .map_err(|e| {
            format!("AI 请求失败: {e}（请确认 CC Switch 本地代理已开 :15721，顶部 Proxy 为绿色）")
        })?;
    let status = resp.status();
    let body = resp.into_string().map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            body.chars().take(240).collect::<String>()
        ));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    // Prefer Anthropic `content[]`; also accept OpenAI-shaped proxy replies.
    let text = anthropic_text(&v);
    if !text.trim().is_empty() {
        return Ok(text);
    }
    let openai = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if openai.trim().is_empty() {
        return Err(format!(
            "模型返回空内容: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    Ok(openai)
}

fn describe_all_openai(
    paths: &[std::path::PathBuf],
    cfg: &AiConfig,
    prompt: &str,
) -> Result<String, String> {
    let (url, key, model) = cfg.endpoint()?;
    let mut content = vec![serde_json::json!({"type":"text","text": prompt})];
    for p in paths {
        let b64 = encode_jpeg_b64(p, 1024)?;
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/jpeg;base64,{b64}") }
        }));
    }

    let payload = serde_json::json!({
        "model": model,
        "stream": false,
        "messages": [{ "role": "user", "content": content }],
        "max_tokens": (600i64).max(paths.len() as i64 * 80),
        "temperature": 0.3,
    });

    let mut req = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(180));
    if !key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }

    let resp = req
        .send_json(payload)
        .map_err(|e| format!("AI 请求失败: {e}"))?;
    let status = resp.status();
    let body = resp.into_string().map_err(|e| e.to_string())?;
    if status == 429 || status >= 500 {
        return Err(format!(
            "上游错误 {status}: {}",
            body.chars().take(160).collect::<String>()
        ));
    }
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }

    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if text.trim().is_empty() {
        // Some proxies wrap Anthropic responses even on OpenAI URLs.
        let alt = anthropic_text(&v);
        if !alt.trim().is_empty() {
            return Ok(alt);
        }
        return Err(format!(
            "模型返回空内容: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    Ok(text)
}
