//! Visual system — light & dark teal brand palettes (MASTER + desktop chrome).

use eframe::egui::{
    self, Color32, FontFamily, FontId, Frame, Margin, RichText, Rounding, Sense, Stroke, Ui, Vec2,
    Visuals,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering, AtomicU32};

/// Design reference size (logical points at zoom = 1).
const DESIGN_W: f32 = 960.0;
const DESIGN_H: f32 = 640.0;
const SCALE_MIN: f32 = 0.82;
const SCALE_MAX: f32 = 1.45;

/// Last applied zoom × 1000 (avoids float churn / feedback).
static LAST_ZOOM_MILLI: AtomicU32 = AtomicU32::new(1000);

/// Desired UI zoom from current window size (undoes current zoom to avoid feedback).
pub fn desired_ui_scale(ctx: &egui::Context) -> f32 {
    let zoom = ctx.zoom_factor().max(0.01);
    let rect = ctx.screen_rect();
    let unzoomed_w = rect.width() * zoom;
    let unzoomed_h = rect.height() * zoom;
    let s = (unzoomed_w / DESIGN_W).min(unzoomed_h / DESIGN_H);
    // Quantize to 0.05 to avoid zoom thrashing every frame.
    ((s * 20.0).round() / 20.0).clamp(SCALE_MIN, SCALE_MAX)
}

/// Apply window-adaptive zoom so chrome, fonts and controls scale together.
pub fn sync_ui_scale(ctx: &egui::Context) {
    let target = desired_ui_scale(ctx);
    let milli = (target * 1000.0).round() as u32;
    if LAST_ZOOM_MILLI.swap(milli, Ordering::Relaxed) != milli {
        ctx.set_zoom_factor(target);
    }
}

/// Clamp a fraction of available width into [min, max].
pub fn frac_width(avail: f32, frac: f32, min: f32, max: f32) -> f32 {
    (avail * frac).clamp(min, max)
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::Light,
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        }
    }
}

static THEME_MODE: AtomicU8 = AtomicU8::new(0); // 0 light, 1 dark

pub fn set_theme_mode(mode: ThemeMode) {
    THEME_MODE.store(
        match mode {
            ThemeMode::Light => 0,
            ThemeMode::Dark => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn theme_mode() -> ThemeMode {
    if THEME_MODE.load(Ordering::Relaxed) == 1 {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

/// Active palette (Copy). Field names match legacy `colors::NAME` for easy migration.
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
pub struct Palette {
    pub BG: Color32,
    pub CHROME: Color32,
    pub PANEL: Color32,
    pub PANEL_ELEVATED: Color32,
    pub PANEL_EDGE: Color32,
    pub INSET: Color32,
    pub SEGMENT: Color32,
    pub TEXT: Color32,
    pub MUTED: Color32,
    pub FAINT: Color32,
    pub ACCENT: Color32,
    pub ACCENT_HOT: Color32,
    pub ACCENT_DIM: Color32,
    pub ACCENT_SOFT: Color32,
    pub STEEL: Color32,
    pub SUCCESS: Color32,
    pub DANGER: Color32,
    pub WARN: Color32,
    pub CANVAS: Color32,
    pub GRID: Color32,
    pub WIRE: Color32,
    pub NODE_START: Color32,
    pub NODE_END: Color32,
    pub NODE_CLICK: Color32,
    pub NODE_WAIT: Color32,
    pub NODE_PAUSE: Color32,
    pub NODE_MANUAL: Color32,
    pub NODE_LOOP: Color32,
    pub NODE_IF: Color32,
    pub NODE_TYPE: Color32,
    pub NODE_BG: Color32,
    pub NODE_SEL: Color32,
    pub HUD_BG: Color32,
    pub HUD_TEXT: Color32,
    pub HUD_MUTED: Color32,
    pub HUD_EDGE: Color32,
    pub HOVER_BG: Color32,
    pub HOVER_EDGE: Color32,
    pub CHIP_SELECTED: Color32,
}

const LIGHT: Palette = Palette {
    BG: Color32::from_rgb(246, 247, 248),
    CHROME: Color32::from_rgb(250, 251, 251),
    PANEL: Color32::from_rgb(255, 255, 255),
    PANEL_ELEVATED: Color32::from_rgb(255, 255, 255),
    PANEL_EDGE: Color32::from_rgb(229, 231, 235),
    INSET: Color32::from_rgb(240, 241, 243),
    SEGMENT: Color32::from_rgb(232, 235, 234),
    TEXT: Color32::from_rgb(19, 78, 74),
    MUTED: Color32::from_rgb(75, 85, 99),
    FAINT: Color32::from_rgb(107, 114, 128),
    ACCENT: Color32::from_rgb(13, 148, 136),
    ACCENT_HOT: Color32::from_rgb(234, 88, 12),
    ACCENT_DIM: Color32::from_rgb(15, 118, 110),
    ACCENT_SOFT: Color32::from_rgb(230, 247, 245),
    STEEL: Color32::from_rgb(20, 184, 166),
    SUCCESS: Color32::from_rgb(22, 163, 74),
    DANGER: Color32::from_rgb(220, 38, 38),
    WARN: Color32::from_rgb(217, 119, 6),
    CANVAS: Color32::from_rgb(28, 28, 30),
    GRID: Color32::from_rgb(44, 44, 46),
    WIRE: Color32::from_rgb(94, 234, 212),
    NODE_START: Color32::from_rgb(48, 209, 88),
    NODE_END: Color32::from_rgb(142, 142, 147),
    NODE_CLICK: Color32::from_rgb(13, 148, 136),
    NODE_WAIT: Color32::from_rgb(217, 119, 6),
    NODE_PAUSE: Color32::from_rgb(220, 38, 38),
    NODE_MANUAL: Color32::from_rgb(94, 92, 230),
    NODE_LOOP: Color32::from_rgb(20, 184, 166),
    NODE_IF: Color32::from_rgb(234, 88, 12),
    NODE_TYPE: Color32::from_rgb(45, 212, 191),
    NODE_BG: Color32::from_rgb(44, 44, 46),
    NODE_SEL: Color32::from_rgb(13, 148, 136),
    HUD_BG: Color32::from_rgb(28, 28, 30),
    HUD_TEXT: Color32::from_rgb(240, 244, 242),
    HUD_MUTED: Color32::from_rgb(156, 168, 163),
    HUD_EDGE: Color32::from_rgb(58, 66, 62),
    HOVER_BG: Color32::from_rgb(238, 249, 247),
    HOVER_EDGE: Color32::from_rgb(153, 230, 216),
    CHIP_SELECTED: Color32::from_rgb(255, 255, 255),
};

const DARK: Palette = Palette {
    BG: Color32::from_rgb(20, 24, 22),
    CHROME: Color32::from_rgb(26, 31, 29),
    PANEL: Color32::from_rgb(35, 40, 38),
    PANEL_ELEVATED: Color32::from_rgb(44, 50, 47),
    PANEL_EDGE: Color32::from_rgb(58, 66, 62),
    INSET: Color32::from_rgb(28, 33, 31),
    SEGMENT: Color32::from_rgb(46, 53, 50),
    TEXT: Color32::from_rgb(240, 244, 242),
    MUTED: Color32::from_rgb(156, 168, 163),
    FAINT: Color32::from_rgb(107, 117, 111),
    ACCENT: Color32::from_rgb(45, 212, 191),
    ACCENT_HOT: Color32::from_rgb(251, 146, 60),
    ACCENT_DIM: Color32::from_rgb(153, 246, 228),
    ACCENT_SOFT: Color32::from_rgb(22, 53, 50),
    STEEL: Color32::from_rgb(94, 234, 212),
    SUCCESS: Color32::from_rgb(74, 222, 128),
    DANGER: Color32::from_rgb(248, 113, 113),
    WARN: Color32::from_rgb(251, 191, 36),
    CANVAS: Color32::from_rgb(14, 16, 16),
    GRID: Color32::from_rgb(30, 36, 34),
    WIRE: Color32::from_rgb(94, 234, 212),
    NODE_START: Color32::from_rgb(48, 209, 88),
    NODE_END: Color32::from_rgb(142, 142, 147),
    NODE_CLICK: Color32::from_rgb(45, 212, 191),
    NODE_WAIT: Color32::from_rgb(251, 191, 36),
    NODE_PAUSE: Color32::from_rgb(248, 113, 113),
    NODE_MANUAL: Color32::from_rgb(129, 140, 248),
    NODE_LOOP: Color32::from_rgb(45, 212, 191),
    NODE_IF: Color32::from_rgb(251, 146, 60),
    NODE_TYPE: Color32::from_rgb(94, 234, 212),
    NODE_BG: Color32::from_rgb(54, 54, 58),
    NODE_SEL: Color32::from_rgb(45, 212, 191),
    HUD_BG: Color32::from_rgb(18, 22, 20),
    HUD_TEXT: Color32::from_rgb(240, 244, 242),
    HUD_MUTED: Color32::from_rgb(156, 168, 163),
    HUD_EDGE: Color32::from_rgb(58, 66, 62),
    HOVER_BG: Color32::from_rgb(30, 56, 52),
    HOVER_EDGE: Color32::from_rgb(42, 92, 84),
    CHIP_SELECTED: Color32::from_rgb(44, 50, 47),
};

#[inline]
pub fn col() -> Palette {
    match theme_mode() {
        ThemeMode::Light => LIGHT,
        ThemeMode::Dark => DARK,
    }
}

/// Backward-compatible alias module (prefer `col()`).
pub mod colors {
    pub use super::col as get;
}

pub fn apply_theme(ctx: &egui::Context) {
    apply_theme_mode(ctx, theme_mode());
}

pub fn apply_theme_mode(ctx: &egui::Context, mode: ThemeMode) {
    set_theme_mode(mode);
    let c = col();
    let mut visuals = match mode {
        ThemeMode::Light => Visuals::light(),
        ThemeMode::Dark => Visuals::dark(),
    };
    visuals.dark_mode = matches!(mode, ThemeMode::Dark);
    visuals.window_fill = c.PANEL;
    visuals.panel_fill = c.BG;
    visuals.extreme_bg_color = c.PANEL;
    visuals.faint_bg_color = c.INSET;
    visuals.override_text_color = Some(c.TEXT);

    let r = Rounding::same(8.0);

    visuals.widgets.noninteractive.bg_fill = c.PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.PANEL_EDGE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.TEXT);
    visuals.widgets.noninteractive.rounding = r;

    visuals.widgets.inactive.bg_fill = c.PANEL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, c.PANEL_EDGE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, c.TEXT);
    visuals.widgets.inactive.rounding = r;

    visuals.widgets.hovered.bg_fill = c.HOVER_BG;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, c.HOVER_EDGE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, c.ACCENT_DIM);
    visuals.widgets.hovered.rounding = r;

    visuals.widgets.active.bg_fill = c.ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, on_accent());
    visuals.widgets.active.rounding = r;

    visuals.widgets.open.bg_fill = c.PANEL_ELEVATED;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, c.PANEL_EDGE);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, c.TEXT);
    visuals.widgets.open.rounding = r;

    visuals.selection.bg_fill = match mode {
        ThemeMode::Light => Color32::from_rgba_unmultiplied(13, 148, 136, 40),
        ThemeMode::Dark => Color32::from_rgba_unmultiplied(45, 212, 191, 48),
    };
    visuals.selection.stroke = Stroke::new(1.0, c.ACCENT);
    visuals.hyperlink_color = c.ACCENT;
    visuals.window_rounding = Rounding::same(8.0);
    visuals.menu_rounding = Rounding::same(8.0);
    visuals.window_stroke = Stroke::new(1.0, c.PANEL_EDGE);
    let shadow_a = if matches!(mode, ThemeMode::Dark) {
        60
    } else {
        20
    };
    visuals.window_shadow = egui::Shadow {
        offset: [0.0, 6.0].into(),
        blur: 20.0,
        spread: 0.0,
        color: Color32::from_black_alpha(shadow_a),
    };
    visuals.popup_shadow = egui::Shadow {
        offset: [0.0, 4.0].into(),
        blur: 14.0,
        spread: 0.0,
        color: Color32::from_black_alpha(shadow_a.saturating_sub(2)),
    };
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.slider_trailing_fill = true;

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.window_margin = Margin::same(16.0);
    style.spacing.menu_margin = Margin::same(8.0);
    style.spacing.indent = 16.0;
    style.spacing.interact_size = egui::vec2(40.0, 30.0);
    style.spacing.slider_width = 156.0;
    style.spacing.slider_rail_height = 3.0;
    style.spacing.combo_width = 140.0;
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    style.spacing.scroll.floating_width = 4.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.dormant_handle_opacity = 0.25;
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(24.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    ctx.set_style(style);
}

/// Soft wash: light gray highlight (light) or subtle lift (dark).
pub fn paint_atmosphere(ui: &Ui) {
    let rect = ui.ctx().screen_rect();
    let painter = ui.painter();
    let c = col();
    painter.rect_filled(rect, 0.0, c.BG);

    let band_h = 180.0_f32.min(rect.height() * 0.35);
    let steps = 24;
    let dark = matches!(theme_mode(), ThemeMode::Dark);
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let y0 = rect.top() + band_h * t0;
        let y1 = rect.top() + band_h * ((i + 1) as f32 / steps as f32);
        let alpha = if dark {
            ((1.0 - t0) * 28.0) as u8
        } else {
            ((1.0 - t0) * 40.0) as u8
        };
        let wash = if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
        } else {
            // Soft teal lift at the top of the window
            Color32::from_rgba_unmultiplied(13, 148, 136, (alpha / 5).max(1))
        };
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left(), y0), egui::pos2(rect.right(), y1)),
            0.0,
            wash,
        );
    }
}

pub fn inset_frame() -> Frame {
    Frame::none()
        .fill(col().INSET)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::same(12.0))
}

pub fn section_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).size(26.0).color(col().TEXT).strong());
        if !subtitle.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(subtitle).size(13.0).color(col().MUTED));
        }
    });
    ui.add_space(14.0);
}

pub fn brand_title(ui: &mut Ui) {
    ui.allocate_ui_with_layout(
        Vec2::new(168.0, 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
            let c = mark.center();
            let p = ui.painter();
            p.rect_filled(mark, 8.0, col().ACCENT);
            p.circle_filled(c, 5.0, Color32::from_rgba_unmultiplied(255, 255, 255, 230));
            p.circle_stroke(c, 5.0, Stroke::new(1.5, Color32::from_white_alpha(180)));
            ui.add_space(9.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    RichText::new("Mouse Suite")
                        .size(16.0)
                        .strong()
                        .color(col().TEXT),
                );
                ui.label(
                    RichText::new(crate::i18n::t("brand.subtitle"))
                        .size(11.0)
                        .color(col().MUTED),
                );
            });
        },
    );
}

/// Apple-style segmented control chip.
pub fn tab_chip(ui: &mut Ui, selected: bool, label: &str) -> bool {
    let pad = Vec2::new(18.0, 6.0);
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            FontId::new(13.0, FontFamily::Proportional),
            if selected {
                col().TEXT
            } else {
                col().MUTED
            },
        )
    });
    let size = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());

    if selected {
        // Selected pill with soft shadow feel
        ui.painter().rect_filled(
            rect.translate(egui::vec2(0.0, 0.5)),
            8.0,
            Color32::from_black_alpha(18),
        );
        ui.painter().rect_filled(rect, 8.0, col().CHIP_SELECTED);
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            8.0,
            if matches!(theme_mode(), ThemeMode::Dark) {
                Color32::from_white_alpha(20)
            } else {
                Color32::from_white_alpha(120)
            },
        );
    }

    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        if selected {
            col().TEXT
        } else {
            col().MUTED
        },
    );
    resp.clicked()
}

pub fn paint_segment_backdrop(ui: &Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 10.0, col().SEGMENT);
}

/// Unified control height for toolbars / form rows (scales with `zoom_factor`).
pub const CTRL_H: f32 = 34.0;

pub fn on_accent() -> Color32 {
    match theme_mode() {
        ThemeMode::Light => Color32::WHITE,
        ThemeMode::Dark => Color32::from_rgb(4, 47, 46),
    }
}

pub fn primary_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(on_accent()).strong())
        .fill(col().ACCENT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

/// Orange CTA for capture / run / record actions.
pub fn cta_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(col().ACCENT_HOT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

pub fn secondary_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(col().TEXT))
        .fill(col().INSET)
        .stroke(Stroke::new(1.0, col().PANEL_EDGE))
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

pub fn danger_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(col().DANGER)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

pub fn quiet_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).size(12.0).color(col().MUTED))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, 28.0));
    ui.add(btn)
}

/// Full-width toolbox / sidebar action button.
pub fn fill_button(ui: &mut Ui, label: &str, fill: Color32) -> egui::Response {
    let w = ui.available_width().max(80.0);
    let on = if fill == col().ACCENT {
        on_accent()
    } else {
        Color32::WHITE
    };
    let btn = egui::Button::new(RichText::new(label).color(on).strong())
        .fill(fill)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(w, CTRL_H - 2.0));
    ui.add(btn)
}

/// Card without outer margin (for grid columns that share spacing).
pub fn panel_frame() -> Frame {
    Frame::none()
        .fill(col().PANEL_ELEVATED)
        .stroke(Stroke::new(1.0, col().PANEL_EDGE))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(14.0))
        .shadow(egui::Shadow {
            offset: [0.0, 2.0].into(),
            blur: 12.0,
            spread: 0.0,
            color: Color32::from_black_alpha(10),
        })
}

pub fn hairline(ui: &mut Ui) {
    let rect = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(1.0, col().PANEL_EDGE));
    ui.add_space(1.0);
    ui.add_space(10.0);
}

/// Compact divider for dense toolbars / HUD rows (less vertical gap than `hairline`).
pub fn soft_separator(ui: &mut Ui) {
    let rect = ui.max_rect();
    let y = ui.cursor().top() + 2.0;
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(1.0, col().PANEL_EDGE));
    ui.add_space(8.0);
}

/// Centered empty-state block for galleries / editors / log panels.
pub fn empty_state(ui: &mut Ui, title: &str, hint: &str) {
    let tall = ui.available_height() > 160.0;
    ui.vertical_centered(|ui| {
        if tall {
            ui.add_space(28.0);
        } else {
            ui.add_space(8.0);
        }
        Frame::none()
            .fill(col().INSET)
            .stroke(Stroke::new(1.0, col().PANEL_EDGE))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(18.0, if tall { 18.0 } else { 10.0 }))
            .show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(title)
                            .size(if tall { 15.0 } else { 13.0 })
                            .strong()
                            .color(col().TEXT),
                    );
                    if !hint.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(hint).size(12.0).color(col().MUTED));
                    }
                });
            });
    });
}

pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).strong().color(col().MUTED));
}

/// Full-width selectable list row with fixed height.
pub fn list_row(ui: &mut Ui, selected: bool, label: &str) -> egui::Response {
    let w = ui.available_width().max(40.0);
    let h = 32.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());

    let bg = if selected {
        col().ACCENT_SOFT
    } else if resp.hovered() {
        col().INSET
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, bg);
    if selected {
        ui.painter()
            .rect_stroke(rect, 8.0, Stroke::new(1.0, col().HOVER_EDGE));
    }

    let color = if selected {
        col().ACCENT_DIM
    } else {
        col().TEXT
    };
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            FontId::new(13.0, FontFamily::Proportional),
            color,
        )
    });
    let text_pos = egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, color);
    resp
}

/// Apple-style segmented tabs. Returns newly selected index on click.
pub fn segmented_control(ui: &mut Ui, labels: &[&str], selected: usize) -> Option<usize> {
    let pad = Vec2::new(16.0, 6.0);
    let gap = 2.0_f32;
    let mut widths = Vec::with_capacity(labels.len());
    let mut total = 8.0_f32; // inner pad
    for lab in labels {
        let g = ui.fonts(|f| {
            f.layout_no_wrap(
                (*lab).to_owned(),
                FontId::new(13.0, FontFamily::Proportional),
                col().TEXT,
            )
        });
        let w = g.size().x + pad.x * 2.0;
        widths.push(w);
        total += w + gap;
    }
    total -= gap;
    total += 8.0;

    let track_h = 32.0;
    let (track, _) = ui.allocate_exact_size(Vec2::new(total, track_h), Sense::hover());
    ui.painter().rect_filled(track, 10.0, col().SEGMENT);

    let mut clicked = None;
    let mut x = track.left() + 4.0;
    let chip_h = track_h - 8.0;
    let y = track.top() + 4.0;

    for (i, lab) in labels.iter().enumerate() {
        let w = widths[i];
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), Vec2::new(w, chip_h));
        let resp = ui.interact(rect, ui.id().with(("seg", i)), Sense::click());

        if i == selected {
            ui.painter().rect_filled(
                rect.translate(egui::vec2(0.0, 0.5)),
                8.0,
                Color32::from_black_alpha(if matches!(theme_mode(), ThemeMode::Dark) {
                    40
                } else {
                    16
                }),
            );
            ui.painter().rect_filled(rect, 8.0, col().CHIP_SELECTED);
        } else if resp.hovered() {
            ui.painter().rect_filled(
                rect,
                8.0,
                if matches!(theme_mode(), ThemeMode::Dark) {
                    Color32::from_white_alpha(20)
                } else {
                    Color32::from_white_alpha(140)
                },
            );
        }

        let color = if i == selected {
            col().TEXT
        } else {
            col().MUTED
        };
        let galley = ui.fonts(|f| {
            f.layout_no_wrap(
                (*lab).to_owned(),
                FontId::new(13.0, FontFamily::Proportional),
                color,
            )
        });
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x * 0.5,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            color,
        );

        if resp.clicked() {
            clicked = Some(i);
        }
        x += w + gap;
    }
    clicked
}

/// Left-to-right toolbar row, vertically centered, fixed height.
pub fn toolbar_row(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(w, CTRL_H + 4.0),
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    );
}

pub fn status_pill(ui: &mut Ui, text: &str, tone: StatusTone) {
    let dark = matches!(theme_mode(), ThemeMode::Dark);
    let (fg, bg) = match (tone, dark) {
        (StatusTone::Idle, _) => (col().MUTED, col().INSET),
        (StatusTone::Run, false) => (
            Color32::from_rgb(0, 120, 50),
            Color32::from_rgb(220, 245, 228),
        ),
        (StatusTone::Run, true) => (
            Color32::from_rgb(48, 209, 88),
            Color32::from_rgb(20, 60, 36),
        ),
        (StatusTone::Warn, false) => (
            Color32::from_rgb(160, 90, 0),
            Color32::from_rgb(255, 240, 220),
        ),
        (StatusTone::Warn, true) => (
            Color32::from_rgb(255, 179, 64),
            Color32::from_rgb(70, 45, 10),
        ),
        (StatusTone::Danger, false) => (
            Color32::from_rgb(180, 30, 30),
            Color32::from_rgb(255, 230, 228),
        ),
        (StatusTone::Danger, true) => (
            Color32::from_rgb(255, 105, 97),
            Color32::from_rgb(70, 28, 28),
        ),
    };
    Frame::none()
        .fill(bg)
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).strong().color(fg));
        });
}

#[derive(Clone, Copy)]
pub enum StatusTone {
    Idle,
    Run,
    Warn,
    Danger,
}

/// Themed checkbox with accent-colored check when selected.
pub fn themed_checkbox(ui: &mut Ui, checked: &mut bool, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Checkbox::new(checked, label))
}

/// Single-line text edit with accent focus ring via global visuals.
pub fn themed_text_edit(ui: &mut Ui, text: &mut String) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(ui.available_width())
            .margin(egui::vec2(10.0, 6.0)),
    )
}

/// Modal / dialog window shell with panel fill and edge stroke.
pub fn themed_window(title: impl Into<egui::WidgetText>) -> egui::Window<'static> {
    let c = col();
    egui::Window::new(title)
        .frame(
            Frame::none()
                .fill(c.PANEL)
                .stroke(Stroke::new(1.0, c.PANEL_EDGE))
                .rounding(Rounding::same(12.0))
                .inner_margin(Margin::same(16.0))
                .shadow(egui::Shadow {
                    offset: [0.0, 8.0].into(),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(if matches!(theme_mode(), ThemeMode::Dark) {
                        80
                    } else {
                        28
                    }),
                }),
        )
}

/// Section title used inside modals (replaces raw `ui.heading`).
pub fn modal_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(18.0).strong().color(col().TEXT));
}
