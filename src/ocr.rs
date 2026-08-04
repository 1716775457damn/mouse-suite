//! Screen OCR: find text on the current monitor and return its screen bbox.
//!
//! - Windows: `Windows.Media.Ocr` (offline, system language packs)
//! - macOS / Linux (and Windows fallback): multimodal AI via [`crate::scribe_ai`]

use crate::screen::{self, CapturedMonitor};
use crate::scribe_ai::{self, AiConfig};
use image::RgbaImage;
use std::io::Cursor;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct OcrHit {
    pub text: String,
    /// Absolute screen pixels (top-left of match).
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl OcrHit {
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatchMode {
    #[default]
    Contains,
    Exact,
}

#[derive(Clone, Debug)]
pub struct OcrMatchOpts {
    pub mode: MatchMode,
    pub case_sensitive: bool,
}

impl Default for OcrMatchOpts {
    fn default() -> Self {
        Self {
            mode: MatchMode::Contains,
            case_sensitive: false,
        }
    }
}

fn text_matches(hay: &str, needle: &str, opts: &OcrMatchOpts) -> bool {
    let n = needle.trim();
    if n.is_empty() {
        return false;
    }
    if opts.case_sensitive {
        match opts.mode {
            MatchMode::Contains => hay.contains(n),
            MatchMode::Exact => hay.trim() == n,
        }
    } else {
        let hay_l = hay.to_lowercase();
        let n_l = n.to_lowercase();
        match opts.mode {
            MatchMode::Contains => hay_l.contains(&n_l),
            MatchMode::Exact => hay_l.trim() == n_l.trim(),
        }
    }
}

/// Capture the monitor under the cursor and find `needle`.
pub fn find_text_on_screen(needle: &str, opts: &OcrMatchOpts) -> Result<Option<OcrHit>, String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Err("目标文字为空".into());
    }
    let cap = screen::capture_under_cursor()?;
    find_text_in_capture(&cap, needle, opts)
}

pub fn find_text_in_capture(
    cap: &CapturedMonitor,
    needle: &str,
    opts: &OcrMatchOpts,
) -> Result<Option<OcrHit>, String> {
    let hits = recognize_all(cap)?;
    Ok(pick_hit(&hits, needle, opts))
}

fn pick_hit(hits: &[OcrHit], needle: &str, opts: &OcrMatchOpts) -> Option<OcrHit> {
    // Prefer the smallest matching line (usually the button label itself).
    let mut best: Option<&OcrHit> = None;
    for h in hits {
        if !text_matches(&h.text, needle, opts) {
            continue;
        }
        let area = (h.w.max(1) as i64) * (h.h.max(1) as i64);
        let better = match best {
            None => true,
            Some(b) => {
                let ba = (b.w.max(1) as i64) * (b.h.max(1) as i64);
                area < ba
            }
        };
        if better {
            best = Some(h);
        }
    }
    best.cloned()
}

fn recognize_all(cap: &CapturedMonitor) -> Result<Vec<OcrHit>, String> {
    #[cfg(windows)]
    {
        match recognize_windows(&cap.image, cap.x, cap.y) {
            Ok(hits) if !hits.is_empty() => return Ok(hits),
            Ok(_) => { /* fall through to AI */ }
            Err(e) => {
                // Local OCR unavailable — try AI before failing hard.
                eprintln!("[ocr] Windows OCR: {e}; trying AI fallback");
            }
        }
    }
    recognize_ai(&cap.image, cap.x, cap.y)
}

#[cfg(windows)]
fn recognize_windows(img: &RgbaImage, origin_x: i32, origin_y: i32) -> Result<Vec<OcrHit>, String> {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    // Worker threads need WinRT apartment init (ignore already-initialized).
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let tmp: PathBuf = std::env::temp_dir().join(format!(
        "mouse_suite_ocr_{}.png",
        std::process::id()
    ));
    img.save(&tmp)
        .map_err(|e| format!("OCR 写临时图失败: {e}"))?;
    let path_str = tmp
        .to_str()
        .ok_or_else(|| "OCR 临时路径无效".to_string())?;

    let result = (|| -> Result<Vec<OcrHit>, String> {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path_str))
            .map_err(|e| format!("StorageFile: {e}"))?
            .get()
            .map_err(|e| format!("StorageFile.get: {e}"))?;
        let stream = file
            .OpenAsync(FileAccessMode::Read)
            .map_err(|e| format!("OpenAsync: {e}"))?
            .get()
            .map_err(|e| format!("OpenAsync.get: {e}"))?;
        let decoder = BitmapDecoder::CreateAsync(&stream)
            .map_err(|e| format!("CreateAsync: {e}"))?
            .get()
            .map_err(|e| format!("decoder.get: {e}"))?;
        let bitmap = decoder
            .GetSoftwareBitmapAsync()
            .map_err(|e| format!("GetSoftwareBitmapAsync: {e}"))?
            .get()
            .map_err(|e| format!("bitmap.get: {e}"))?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|e| format!("OcrEngine: {e}"))?;
        let ocr = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| format!("RecognizeAsync: {e}"))?
            .get()
            .map_err(|e| format!("Recognize.get: {e}"))?;
        let lines = ocr.Lines().map_err(|e| format!("Lines: {e}"))?;
        let count = lines.Size().map_err(|e| format!("Size: {e}"))?;
        let mut hits = Vec::new();
        for i in 0..count {
            let line = lines.GetAt(i).map_err(|e| format!("GetAt: {e}"))?;
            let text = line
                .Text()
                .map(|t| t.to_string())
                .unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            let (x, y, w, h) = match line.Words() {
                Ok(words) => {
                    let n = words.Size().unwrap_or(0);
                    let mut min_x = f32::MAX;
                    let mut min_y = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut max_y = f32::MIN;
                    let mut any = false;
                    for wi in 0..n {
                        if let Ok(word) = words.GetAt(wi) {
                            if let Ok(rect) = word.BoundingRect() {
                                any = true;
                                min_x = min_x.min(rect.X);
                                min_y = min_y.min(rect.Y);
                                max_x = max_x.max(rect.X + rect.Width);
                                max_y = max_y.max(rect.Y + rect.Height);
                            }
                        }
                    }
                    if any {
                        (
                            min_x.round() as i32,
                            min_y.round() as i32,
                            (max_x - min_x).round().max(1.0) as i32,
                            (max_y - min_y).round().max(1.0) as i32,
                        )
                    } else {
                        (0, 0, img.width() as i32, 20)
                    }
                }
                Err(_) => (0, 0, img.width() as i32, 20),
            };
            hits.push(OcrHit {
                text,
                x: origin_x + x,
                y: origin_y + y,
                w,
                h,
            });
        }
        Ok(hits)
    })();

    let _ = std::fs::remove_file(&tmp);
    result
}

fn encode_jpeg_b64_rgba(img: &RgbaImage, max_side: u32) -> Result<(String, u32, u32), String> {
    let mut rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let (mut w, mut h) = rgb.dimensions();
    let m = w.max(h);
    if m > max_side {
        let s = max_side as f32 / m as f32;
        w = ((w as f32) * s).round().max(1.0) as u32;
        h = ((h as f32) * s).round().max(1.0) as u32;
        rgb = image::imageops::resize(&rgb, w, h, image::imageops::FilterType::Triangle);
    }
    let mut buf = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, buf);
    Ok((b64, w, h))
}

fn recognize_ai(img: &RgbaImage, origin_x: i32, origin_y: i32) -> Result<Vec<OcrHit>, String> {
    let cfg = AiConfig::load();
    let (b64, w, h) = encode_jpeg_b64_rgba(img, 1280)?;
    let prompt = format!(
        "你是屏幕 OCR。图像宽 {w} 高 {h}（像素，左上为原点）。\
         列出图中所有可见文字行。只返回 JSON 数组，每项:\
         {{\"text\":\"...\",\"x\":0,\"y\":0,\"w\":10,\"h\":10}}。\
         x,y,w,h 为该行在图像内的像素包围盒。不要 Markdown，不要解释。"
    );
    let text = vision_locate(&cfg, &b64, &prompt)?;
    let payload = scribe_ai::extract_json_payload(&text);
    let arr: Vec<serde_json::Value> = serde_json::from_str(payload).map_err(|e| {
        format!(
            "AI OCR JSON 解析失败: {e}; raw={}",
            text.chars().take(180).collect::<String>()
        )
    })?;
    let mut hits = Vec::new();
    for v in arr {
        let t = v["text"].as_str().unwrap_or("").to_string();
        if t.trim().is_empty() {
            continue;
        }
        let x = v["x"].as_f64().unwrap_or(0.0).round() as i32;
        let y = v["y"].as_f64().unwrap_or(0.0).round() as i32;
        let bw = v["w"].as_f64().unwrap_or(1.0).round().max(1.0) as i32;
        let bh = v["h"].as_f64().unwrap_or(1.0).round().max(1.0) as i32;
        // Scale from possibly downscaled JPEG coords back to full capture.
        let sx = img.width() as f64 / w as f64;
        let sy = img.height() as f64 / h as f64;
        hits.push(OcrHit {
            text: t,
            x: origin_x + (x as f64 * sx).round() as i32,
            y: origin_y + (y as f64 * sy).round() as i32,
            w: (bw as f64 * sx).round().max(1.0) as i32,
            h: (bh as f64 * sy).round().max(1.0) as i32,
        });
    }
    if hits.is_empty() {
        return Err(
            "OCR 未识别到文字（Windows 系统 OCR 不可用且 AI 未返回结果；请安装 OCR 语言包或配置 AI）"
                .into(),
        );
    }
    Ok(hits)
}

fn vision_locate(cfg: &AiConfig, jpeg_b64: &str, prompt: &str) -> Result<String, String> {
    if cfg.is_ccswitch() {
        let (url, _key, model) = cfg.endpoint()?;
        let payload = serde_json::json!({
            "model": model,
            "max_tokens": 2048,
            "temperature": 0.1,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/jpeg",
                            "data": jpeg_b64
                        }
                    }
                ]
            }],
        });
        let resp = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("anthropic-version", "2023-06-01")
            .timeout(std::time::Duration::from_secs(120))
            .send_json(payload)
            .map_err(|e| {
                format!("AI OCR 请求失败: {e}（请确认 CC Switch 代理 :15721 或改用 GLM/自定义）")
            })?;
        let status = resp.status();
        let body = resp.into_string().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            ));
        }
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let mut text = String::new();
        if let Some(arr) = v["content"].as_array() {
            for block in arr {
                if block["type"].as_str() == Some("text") {
                    if let Some(t) = block["text"].as_str() {
                        text.push_str(t);
                    }
                }
            }
        }
        if text.trim().is_empty() {
            text = v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
        }
        if text.trim().is_empty() {
            return Err("AI OCR 返回空内容".into());
        }
        return Ok(text);
    }

    let (url, key, model) = cfg.endpoint()?;
    let payload = serde_json::json!({
        "model": model,
        "stream": false,
        "temperature": 0.1,
        "max_tokens": 2048,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": prompt },
                {
                    "type": "image_url",
                    "image_url": { "url": format!("data:image/jpeg;base64,{jpeg_b64}") }
                }
            ]
        }],
    });
    let mut req = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(120));
    if !key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let resp = req
        .send_json(payload)
        .map_err(|e| format!("AI OCR 请求失败: {e}"))?;
    let status = resp.status();
    let body = resp.into_string().map_err(|e| e.to_string())?;
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
        return Err("AI OCR 返回空内容".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_contains_case_insensitive() {
        let opts = OcrMatchOpts::default();
        assert!(text_matches("确定 OK", "确定", &opts));
        assert!(text_matches("Hello", "hello", &opts));
        assert!(!text_matches("取消", "确定", &opts));
    }

    #[test]
    fn match_exact() {
        let opts = OcrMatchOpts {
            mode: MatchMode::Exact,
            case_sensitive: false,
        };
        assert!(text_matches("  Ok  ", "ok", &opts));
        assert!(!text_matches("Ok please", "ok", &opts));
    }
}
