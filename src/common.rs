use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn exe_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().expect("Failed to get exe path");
        exe.parent().expect("Failed to get exe dir").to_path_buf()
    })
}

/// Writable app data (never inside a macOS .app bundle).
///
/// - Windows portable: `{exe_dir}/data`
/// - macOS: `~/Library/Application Support/Mouse Suite`
/// - Linux: `$XDG_DATA_HOME/mouse-suite` or `~/.local/share/mouse-suite`
pub fn data_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = platform_data_dir();
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
}

fn platform_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Mouse Suite");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("mouse-suite");
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mouse-suite");
        }
    }
    // Windows (and fallback): portable next to the executable.
    exe_dir().join("data")
}

fn config_file_path() -> PathBuf {
    data_dir().join("config.toml")
}

/// Bundled / sideloaded default config locations (read-only OK).
fn bundled_config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // Next to exe (portable / Windows)
    paths.push(exe_dir().join("config.toml"));
    // macOS app Resources
    #[cfg(target_os = "macos")]
    {
        // .../Contents/MacOS -> .../Contents/Resources/config.toml
        if let Some(contents) = exe_dir().parent() {
            paths.push(contents.join("Resources").join("config.toml"));
        }
    }
    paths
}

/// Shared element library entry (recorder gallery + flow picker).
#[derive(Clone, Debug)]
pub struct ElementCatalogItem {
    pub name: String,
    /// Absolute path to primary template image (may be missing on disk).
    pub preview_path: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub db_path: Option<String>,
    pub image_dir: Option<String>,
    /// Milliseconds to wait after hiding UI before capturing (500–3000).
    #[serde(default = "default_hide_wait_ms")]
    pub hide_wait_ms: u64,
    /// UI language: "zh" | "en"
    #[serde(default = "default_language")]
    pub language: String,
    /// UI theme: "light" | "dark"
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_hide_wait_ms() -> u64 {
    1500
}

fn default_language() -> String {
    "zh".into()
}

fn default_theme() -> String {
    "light".into()
}

impl Config {
    pub fn load() -> Self {
        // Prefer writable user config; fall back to bundled defaults.
        let mut candidates = vec![config_file_path()];
        candidates.extend(bundled_config_candidates());
        for path in candidates {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = toml::from_str(&content) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        let config_path = config_file_path();
        let mut doc = String::from(
            "# Paths relative to the data directory, or absolute paths.\n\
             # db_path = \"mouse_recorder.db\"\n\
             # image_dir = \".\"\n\
             # language = \"zh\" | \"en\"\n\
             # theme = \"light\" | \"dark\"\n",
        );
        doc.push_str(&format!(
            "hide_wait_ms = {}\n",
            self.hide_wait_ms.clamp(500, 3000)
        ));
        doc.push_str(&format!("language = \"{}\"\n", self.language));
        doc.push_str(&format!("theme = \"{}\"\n", self.theme));
        if let Some(ref p) = self.db_path {
            doc.push_str(&format!("db_path = \"{}\"\n", p.replace('\\', "\\\\")));
        }
        if let Some(ref p) = self.image_dir {
            doc.push_str(&format!("image_dir = \"{}\"\n", p.replace('\\', "\\\\")));
        }
        let _ = std::fs::write(config_path, doc);
    }

    pub fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            // Relative paths resolve against writable data dir (not the .app bundle).
            data_dir().join(p)
        }
    }

    pub fn db_path(&self) -> String {
        self.db_path
            .as_ref()
            .map(|p| self.resolve(p).to_string_lossy().to_string())
            .unwrap_or_else(|| {
                data_dir()
                    .join("mouse_recorder.db")
                    .to_string_lossy()
                    .to_string()
            })
    }

    pub fn image_dir(&self) -> String {
        self.image_dir
            .as_ref()
            .map(|p| self.resolve(p).to_string_lossy().to_string())
            .unwrap_or_else(|| data_dir().to_string_lossy().to_string())
    }

    pub fn hide_wait_ms(&self) -> u64 {
        self.hide_wait_ms.clamp(500, 3000)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_path: None,
            image_dir: None,
            hide_wait_ms: 1500,
            language: default_language(),
            theme: default_theme(),
        }
    }
}

pub fn setup_chinese_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Prefer clean CJK + SF-like Latin pairing.
    let cn_candidates = [
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];
    for path in &cn_candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "cn_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "cn_font".to_owned());
            fonts
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .push("cn_font".to_owned());
            break;
        }
    }

    // Segoe UI Variable / Semibold — closest SF Pro stand-in on Windows.
    for path in [
        "C:\\Windows\\Fonts\\SegoeUI-VF.ttf",
        "C:\\Windows\\Fonts\\segoeuib.ttf",
        "C:\\Windows\\Fonts\\seguisb.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/SFNSText.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
    ] {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "ui_latin".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(data)),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .push("ui_latin".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}
