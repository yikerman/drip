//! One neutral style for the whole UI (DESIGN G5): everything sits on middle
//! grey, the standard surround for judging color, with no shadows, rounding or
//! strokes beyond what is needed to tell elements apart.

use egui::{Color32, CornerRadius, Shadow, Stroke, Visuals};

/// sRGB 118 is 18% linear reflectance.
pub const GREY: Color32 = Color32::from_gray(118);
pub const DARKER: Color32 = Color32::from_gray(104);
pub const LIGHTER: Color32 = Color32::from_gray(134);
pub const LIGHTEST: Color32 = Color32::from_gray(150);
pub const TEXT: Color32 = Color32::from_gray(16);
pub const WEAK: Color32 = Color32::from_gray(56);
pub const ERROR: Color32 = Color32::from_rgb(96, 16, 16);

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
    v.hyperlink_color = TEXT;
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
    // Separators between panels are the only lines left.
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DARKER);
    ctx.set_visuals(v);
}
