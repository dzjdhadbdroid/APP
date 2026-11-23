//! Thème visuel : palette sobre (sombre ou claire), accent indigo, coins arrondis,
//! espacements aérés, police système (Segoe UI sous Windows) et cartes.

use std::sync::{Arc, Once};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle,
};

pub const ACCENT: Color32 = Color32::from_rgb(99, 102, 241);

struct Palette {
    background: Color32,
    surface: Color32,
    card: Color32,
    line: Color32,
    text: Color32,
    muted: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            background: Color32::from_rgb(15, 17, 23),
            surface: Color32::from_rgb(21, 24, 33),
            card: Color32::from_rgb(28, 32, 44),
            line: Color32::from_rgb(44, 49, 64),
            text: Color32::from_rgb(226, 229, 238),
            muted: Color32::from_rgb(140, 147, 166),
        }
    } else {
        Palette {
            background: Color32::from_rgb(244, 245, 249),
            surface: Color32::from_rgb(255, 255, 255),
            card: Color32::from_rgb(255, 255, 255),
            line: Color32::from_rgb(222, 225, 234),
            text: Color32::from_rgb(30, 34, 46),
            muted: Color32::from_rgb(105, 112, 130),
        }
    }
}

fn load_system_font(ctx: &egui::Context) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let candidates = [
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "/System/Library/Fonts/SFNS.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        for path in candidates {
            if let Ok(bytes) = std::fs::read(path) {
                let mut fonts = FontDefinitions::default();
                fonts.font_data.insert(
                    "system_ui".to_owned(),
                    Arc::new(FontData::from_owned(bytes)),
                );
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.insert(0, "system_ui".to_owned()); // les polices par défaut restent en secours
                }
                ctx.set_fonts(fonts);
                break;
            }
        }
    });
}

/// Applique le thème à tout le contexte egui.
pub fn apply(ctx: &egui::Context, dark: bool) {
    load_system_font(ctx);
    let p = palette(dark);
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.background;
    v.window_fill = p.surface;
    v.extreme_bg_color = if dark {
        Color32::from_rgb(11, 13, 18)
    } else {
        Color32::WHITE
    };
    v.faint_bg_color = p.card;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = if dark {
        Color32::from_rgb(150, 160, 255)
    } else {
        ACCENT
    };
    v.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { ACCENT });
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, p.line);

    let radius = CornerRadius::same(8);
    v.widgets.noninteractive.bg_fill = p.card;
    v.widgets.noninteractive.weak_bg_fill = p.card;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.noninteractive.corner_radius = radius;
    v.widgets.inactive.bg_fill = p.card;
    v.widgets.inactive.weak_bg_fill = p.card;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.inactive.corner_radius = radius;
    v.widgets.hovered.weak_bg_fill = ACCENT.gamma_multiply(0.25);
    v.widgets.hovered.bg_fill = ACCENT.gamma_multiply(0.25);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.corner_radius = radius;
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.5);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.corner_radius = radius;
    v.widgets.open.corner_radius = radius;
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.window_margin = Margin::same(14);
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(15.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(14.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// Fond de la barre du haut et des barres latérales (un cran au-dessus du fond de page).
pub fn bar_frame(ctx: &egui::Context, margin: Margin) -> egui::Frame {
    let dark = ctx.style().visuals.dark_mode;
    egui::Frame::new()
        .fill(palette(dark).surface)
        .stroke(Stroke::new(1.0, palette(dark).line))
        .inner_margin(margin)
}

/// Zone de contenu principale, avec de la marge.
pub fn page_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::new()
        .fill(ctx.style().visuals.panel_fill)
        .inner_margin(Margin::symmetric(22, 16))
}

/// Carte arrondie pour regrouper des informations.
pub fn card(ui: &egui::Ui) -> egui::Frame {
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(palette(dark).card)
        .stroke(Stroke::new(1.0, palette(dark).line))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(18, 14))
}

pub fn muted(ui: &egui::Ui) -> Color32 {
    palette(ui.visuals().dark_mode).muted
}
