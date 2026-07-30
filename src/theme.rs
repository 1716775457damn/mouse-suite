use eframe::egui::{self, Color32, Rounding, Stroke, Visuals};

/// Blueprint-workshop palette: cool slate chrome + teal accent (not purple).
pub mod colors {
    use eframe::egui::Color32;

    pub const BG: Color32 = Color32::from_rgb(236, 240, 244);
    pub const PANEL: Color32 = Color32::from_rgb(248, 250, 252);
    pub const PANEL_EDGE: Color32 = Color32::from_rgb(203, 213, 225);
    pub const TEXT: Color32 = Color32::from_rgb(15, 23, 42);
    pub const MUTED: Color32 = Color32::from_rgb(100, 116, 139);
    pub const ACCENT: Color32 = Color32::from_rgb(13, 148, 136);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(45, 212, 191);
    pub const CANVAS: Color32 = Color32::from_rgb(11, 31, 51);
    pub const GRID: Color32 = Color32::from_rgb(22, 48, 74);
    pub const WIRE: Color32 = Color32::from_rgb(94, 234, 212);

    pub const NODE_START: Color32 = Color32::from_rgb(52, 211, 153);
    pub const NODE_END: Color32 = Color32::from_rgb(148, 163, 184);
    pub const NODE_CLICK: Color32 = Color32::from_rgb(56, 189, 248);
    pub const NODE_WAIT: Color32 = Color32::from_rgb(251, 191, 36);
    pub const NODE_PAUSE: Color32 = Color32::from_rgb(251, 113, 133);
    pub const NODE_MANUAL: Color32 = Color32::from_rgb(167, 139, 250);
    pub const NODE_BG: Color32 = Color32::from_rgb(30, 58, 95);
    pub const NODE_SEL: Color32 = Color32::from_rgb(45, 212, 191);
}

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::light();
    visuals.window_fill = colors::PANEL;
    visuals.panel_fill = colors::BG;
    visuals.extreme_bg_color = Color32::from_rgb(226, 232, 240);
    visuals.faint_bg_color = Color32::from_rgb(241, 245, 249);
    visuals.override_text_color = Some(colors::TEXT);
    visuals.widgets.noninteractive.bg_fill = colors::PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors::TEXT);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(226, 232, 240);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, colors::TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(204, 251, 241);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, colors::ACCENT);
    visuals.widgets.active.bg_fill = colors::ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(13, 148, 136, 80);
    visuals.selection.stroke = Stroke::new(1.0, colors::ACCENT);
    visuals.hyperlink_color = colors::ACCENT;
    visuals.window_rounding = Rounding::same(8.0);
    visuals.menu_rounding = Rounding::same(6.0);
    visuals.widgets.noninteractive.rounding = Rounding::same(4.0);
    visuals.widgets.inactive.rounding = Rounding::same(4.0);
    visuals.widgets.hovered.rounding = Rounding::same(4.0);
    visuals.widgets.active.rounding = Rounding::same(4.0);
    visuals.window_stroke = Stroke::new(1.0, colors::PANEL_EDGE);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    ctx.set_style(style);
}

pub fn section_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(20.0)
            .color(colors::TEXT)
            .strong(),
    );
    if !subtitle.is_empty() {
        ui.label(egui::RichText::new(subtitle).size(12.0).color(colors::MUTED));
    }
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(6.0);
}
