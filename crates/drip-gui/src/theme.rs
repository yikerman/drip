//! Neutral UI on middle grey, the standard surround for judging color, with
//! lighter menus and no shadows, rounding or strokes beyond what is
//! needed to tell elements apart.

use egui::{Color32, CornerRadius, FontId, Shadow, Stroke, TextStyle, Visuals};

/// sRGB 118 is 18% linear reflectance.
pub const GREY: Color32 = Color32::from_gray(118);
pub const DARKER: Color32 = Color32::from_gray(104);
pub const LIGHTER: Color32 = Color32::from_gray(134);
pub const LIGHTEST: Color32 = Color32::from_gray(150);
pub const TEXT: Color32 = Color32::from_gray(16);
pub const WEAK: Color32 = Color32::from_gray(56);
pub const ERROR: Color32 = Color32::from_rgb(96, 16, 16);
pub const LINK: Color32 = Color32::from_rgb(0, 32, 80);

pub const BODY_SIZE: f32 = 16.0;
pub const SMALL_SIZE: f32 = 14.0;

pub fn context_menu(response: &egui::Response) -> egui::Popup<'_> {
    egui::Popup::context_menu(response).style(menu_style)
}

pub fn menu(response: &egui::Response) -> egui::Popup<'_> {
    egui::Popup::menu(response).style(menu_style)
}

fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.visuals.window_fill = LIGHTEST;
    style.visuals.window_stroke = Stroke::new(1.0, WEAK);
}

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::light();
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(WEAK);
    v.panel_fill = GREY;
    v.window_fill = GREY;
    v.faint_bg_color = GREY;
    v.extreme_bg_color = DARKER;
    v.code_bg_color = DARKER;
    v.error_fg_color = ERROR;
    v.warn_fg_color = ERROR;
    v.hyperlink_color = LINK;
    v.window_stroke = Stroke::NONE;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.selection.bg_fill = LIGHTEST;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.striped = false;
    v.slider_trailing_fill = true;
    for (w, fill) in [
        (&mut v.widgets.noninteractive, GREY),
        (&mut v.widgets.inactive, DARKER),
        (&mut v.widgets.hovered, LIGHTER),
        (&mut v.widgets.active, LIGHTEST),
        (&mut v.widgets.open, LIGHTER),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0, TEXT);
        w.corner_radius = CornerRadius::ZERO;
        w.expansion = 0.0;
    }
    // Keep panel separators visible against the surround.
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DARKER);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(22.0)),
            (TextStyle::Body, FontId::proportional(BODY_SIZE)),
            (TextStyle::Button, FontId::proportional(BODY_SIZE)),
            (TextStyle::Small, FontId::proportional(SMALL_SIZE)),
            (TextStyle::Monospace, FontId::monospace(BODY_SIZE)),
        ]
        .into();
    });
}
