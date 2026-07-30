use eframe::egui;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn exe_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = std::env::current_exe().expect("Failed to get exe path");
        exe.parent().expect("Failed to get exe dir").to_path_buf()
    })
}

pub fn data_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| exe_dir().join("data"))
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub db_path: Option<String>,
    pub image_dir: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let config_path = exe_dir().join("config.toml");
        std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            exe_dir().join(p)
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_path: None,
            image_dir: None,
        }
    }
}

pub fn setup_chinese_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_paths = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];
    for path in &font_paths {
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
    ctx.set_fonts(fonts);
}
