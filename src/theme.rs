//! Visual system — Apple HIG–inspired refined minimalism
//! Soft system gray, SF Blue accent, segmented controls, generous air.
//! Guided by frontend-design: restraint, precision, memorable clarity.

use eframe::egui::{
    self, Color32, FontFamily, FontId, Frame, Margin, RichText, Rounding, Sense, Stroke, Ui, Vec2,
    Visuals,
};

pub mod colors {
    use eframe::egui::Color32;

    /// macOS window chrome / content gray
    pub const BG: Color32 = Color32::from_rgb(245, 245, 247);
    pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255);
    pub const PANEL_ELEVATED: Color32 = Color32::from_rgb(255, 255, 255);
    pub const PANEL_EDGE: Color32 = Color32::from_rgb(229, 229, 234);
    pub const INSET: Color32 = Color32::from_rgb(242, 242, 247);
    pub const SEGMENT: Color32 = Color32::from_rgb(232, 232, 237);

    pub const TEXT: Color32 = Color32::from_rgb(29, 29, 31);
    pub const MUTED: Color32 = Color32::from_rgb(110, 110, 115);
    pub const FAINT: Color32 = Color32::from_rgb(174, 174, 178);

    /// SF Blue
    pub const ACCENT: Color32 = Color32::from_rgb(0, 122, 255);
    pub const ACCENT_HOT: Color32 = Color32::from_rgb(10, 132, 255);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(0, 102, 220);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(224, 236, 255);

    pub const STEEL: Color32 = Color32::from_rgb(88, 86, 214); // rarely used secondary

    pub const SUCCESS: Color32 = Color32::from_rgb(52, 199, 89);
    pub const DANGER: Color32 = Color32::from_rgb(255, 59, 48);
    pub const WARN: Color32 = Color32::from_rgb(255, 149, 0);

    /// Flow canvas — soft charcoal (macOS dark content)
    pub const CANVAS: Color32 = Color32::from_rgb(28, 28, 30);
    pub const GRID: Color32 = Color32::from_rgb(44, 44, 46);
    pub const WIRE: Color32 = Color32::from_rgb(100, 180, 255);

    pub const NODE_START: Color32 = Color32::from_rgb(48, 209, 88);
    pub const NODE_END: Color32 = Color32::from_rgb(142, 142, 147);
    pub const NODE_CLICK: Color32 = Color32::from_rgb(10, 132, 255);
    pub const NODE_WAIT: Color32 = Color32::from_rgb(255, 159, 10);
    pub const NODE_PAUSE: Color32 = Color32::from_rgb(255, 69, 58);
    pub const NODE_MANUAL: Color32 = Color32::from_rgb(94, 92, 230);
    pub const NODE_LOOP: Color32 = Color32::from_rgb(50, 215, 175);
    pub const NODE_IF: Color32 = Color32::from_rgb(255, 149, 0);
    pub const NODE_TYPE: Color32 = Color32::from_rgb(100, 210, 255);
    pub const NODE_BG: Color32 = Color32::from_rgb(44, 44, 46);
    pub const NODE_SEL: Color32 = Color32::from_rgb(10, 132, 255);

    pub const HUD_BG: Color32 = Color32::from_rgb(28, 28, 30);
}

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::light();
    visuals.window_fill = colors::PANEL;
    visuals.panel_fill = colors::BG;
    visuals.extreme_bg_color = colors::INSET;
    visuals.faint_bg_color = colors::PANEL;
    visuals.override_text_color = Some(colors::TEXT);

    let r = Rounding::same(10.0);

    visuals.widgets.noninteractive.bg_fill = colors::PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors::PANEL_EDGE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors::TEXT);
    visuals.widgets.noninteractive.rounding = r;

    visuals.widgets.inactive.bg_fill = colors::INSET;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, colors::PANEL_EDGE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, colors::TEXT);
    visuals.widgets.inactive.rounding = r;

    visuals.widgets.hovered.bg_fill = colors::ACCENT_SOFT;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(180, 210, 255));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, colors::ACCENT_DIM);
    visuals.widgets.hovered.rounding = r;

    visuals.widgets.active.bg_fill = colors::ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.rounding = r;

    visuals.widgets.open.bg_fill = colors::PANEL_ELEVATED;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, colors::PANEL_EDGE);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, colors::TEXT);
    visuals.widgets.open.rounding = r;

    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0, 122, 255, 40);
    visuals.selection.stroke = Stroke::new(1.0, colors::ACCENT);
    visuals.hyperlink_color = colors::ACCENT;
    visuals.window_rounding = Rounding::same(14.0);
    visuals.menu_rounding = Rounding::same(12.0);
    visuals.window_stroke = Stroke::new(1.0, colors::PANEL_EDGE);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.window_margin = Margin::same(16.0);
    style.spacing.indent = 16.0;
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(26.0, FontFamily::Proportional),
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

/// Soft Apple-like wash: light gray with subtle top highlight (no hatch noise).
pub fn paint_atmosphere(ui: &Ui) {
    let rect = ui.ctx().screen_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, colors::BG);

    // Gentle top luminosity
    let band_h = 180.0_f32.min(rect.height() * 0.35);
    let steps = 24;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let y0 = rect.top() + band_h * t0;
        let y1 = rect.top() + band_h * t1;
        let alpha = ((1.0 - t0) * 40.0) as u8;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), y0),
                egui::pos2(rect.right(), y1),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
        );
    }
}

pub fn card_frame() -> Frame {
    Frame::none()
        .fill(colors::PANEL_ELEVATED)
        .stroke(Stroke::new(1.0, colors::PANEL_EDGE))
        .rounding(Rounding::same(16.0))
        .inner_margin(Margin::symmetric(20.0, 18.0))
        .outer_margin(Margin::same(12.0))
        .shadow(egui::Shadow {
            offset: [0.0, 4.0].into(),
            blur: 24.0,
            spread: 0.0,
            color: Color32::from_black_alpha(12),
        })
}

pub fn inset_frame() -> Frame {
    Frame::none()
        .fill(colors::INSET)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(12.0))
}

pub fn section_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(title)
                .size(26.0)
                .color(colors::TEXT)
                .strong(),
        );
        if !subtitle.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(subtitle).size(13.0).color(colors::MUTED));
        }
    });
    ui.add_space(14.0);
}

pub fn brand_title(ui: &mut Ui) {
    ui.allocate_ui_with_layout(
        Vec2::new(128.0, 36.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::hover());
            let c = mark.center();
            let p = ui.painter();
            p.rect_filled(mark, 7.0, colors::ACCENT);
            p.circle_filled(c, 5.0, Color32::from_rgba_unmultiplied(255, 255, 255, 230));
            p.circle_stroke(c, 5.0, Stroke::new(1.5, Color32::from_white_alpha(180)));
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    RichText::new("Mouse Suite")
                        .size(15.0)
                        .strong()
                        .color(colors::TEXT),
                );
                ui.label(
                    RichText::new("视觉自动化")
                        .size(11.0)
                        .color(colors::MUTED),
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
                colors::TEXT
            } else {
                colors::MUTED
            },
        )
    });
    let size = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());

    if selected {
        // White pill with soft shadow feel
        ui.painter().rect_filled(
            rect.translate(egui::vec2(0.0, 0.5)),
            8.0,
            Color32::from_black_alpha(18),
        );
        ui.painter().rect_filled(rect, 8.0, Color32::WHITE);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 8.0, Color32::from_white_alpha(120));
    }

    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        if selected {
            colors::TEXT
        } else {
            colors::MUTED
        },
    );
    resp.clicked()
}

pub fn paint_segment_backdrop(ui: &Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 10.0, colors::SEGMENT);
}

/// Unified control height for toolbars / form rows.
pub const CTRL_H: f32 = 34.0;

pub fn primary_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(colors::ACCENT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

pub fn secondary_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(colors::TEXT))
        .fill(colors::INSET)
        .stroke(Stroke::new(1.0, colors::PANEL_EDGE))
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

pub fn danger_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(colors::DANGER)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, CTRL_H));
    ui.add(btn)
}

pub fn quiet_button(ui: &mut Ui, label: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).size(12.0).color(colors::MUTED))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, 28.0));
    ui.add(btn)
}

/// Card without outer margin (for grid columns that share spacing).
pub fn panel_frame() -> Frame {
    Frame::none()
        .fill(colors::PANEL_ELEVATED)
        .stroke(Stroke::new(1.0, colors::PANEL_EDGE))
        .rounding(Rounding::same(14.0))
        .inner_margin(Margin::same(14.0))
        .shadow(egui::Shadow {
            offset: [0.0, 2.0].into(),
            blur: 16.0,
            spread: 0.0,
            color: Color32::from_black_alpha(10),
        })
}

pub fn hairline(ui: &mut Ui) {
    let rect = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(
        rect.x_range(),
        y,
        Stroke::new(1.0, colors::PANEL_EDGE),
    );
    ui.add_space(1.0);
    ui.add_space(10.0);
}

pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .strong()
            .color(colors::MUTED),
    );
}

/// Full-width selectable list row with fixed height.
pub fn list_row(ui: &mut Ui, selected: bool, label: &str) -> egui::Response {
    let w = ui.available_width().max(40.0);
    let h = 32.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());

    let bg = if selected {
        colors::ACCENT_SOFT
    } else if resp.hovered() {
        colors::INSET
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, bg);
    if selected {
        ui.painter().rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, Color32::from_rgb(180, 210, 255)),
        );
    }

    let color = if selected {
        colors::ACCENT_DIM
    } else {
        colors::TEXT
    };
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            FontId::new(13.0, FontFamily::Proportional),
            color,
        )
    });
    let text_pos = egui::pos2(
        rect.left() + 10.0,
        rect.center().y - galley.size().y * 0.5,
    );
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
                colors::TEXT,
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
    ui.painter().rect_filled(track, 10.0, colors::SEGMENT);

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
                Color32::from_black_alpha(16),
            );
            ui.painter().rect_filled(rect, 8.0, Color32::WHITE);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 8.0, Color32::from_white_alpha(140));
        }

        let color = if i == selected {
            colors::TEXT
        } else {
            colors::MUTED
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
    let (fg, bg) = match tone {
        StatusTone::Idle => (colors::MUTED, colors::INSET),
        StatusTone::Run => (
            Color32::from_rgb(0, 120, 50),
            Color32::from_rgb(220, 245, 228),
        ),
        StatusTone::Warn => (
            Color32::from_rgb(160, 90, 0),
            Color32::from_rgb(255, 240, 220),
        ),
        StatusTone::Danger => (
            Color32::from_rgb(180, 30, 30),
            Color32::from_rgb(255, 230, 228),
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
