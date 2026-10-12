//! Neutral UI on middle grey, the standard surround for judging color, with
//! lighter menus, dark tooltips and no shadows, rounding or strokes beyond what is
//! needed to tell elements apart. Text is set in Source Sans 3 and Source
//! Code Pro, vendored under `vendor/`; egui's bundled fonts are disabled.

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Shadow,
    Stroke, TextStyle, Visuals,
};
use std::sync::Arc;

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

/// Hover text, dark so it reads as information rather than as a menu of actions.
/// Use instead of egui's `on_hover_text`, whose tooltips look like menus.
pub trait Tip {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self;
}

impl Tip for egui::Response {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let mut tooltip = egui::Tooltip::for_enabled(&self);
        tooltip.popup = tooltip.popup.style(|style: &mut egui::Style| {
            style.visuals.window_fill = Color32::from_gray(40);
            style.visuals.window_stroke = Stroke::NONE;
            style.visuals.override_text_color = Some(Color32::from_gray(220));
        });
        tooltip.show(|ui| {
            // As egui's own: keep auto-sizing from shrinking dynamic text.
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.label(text);
        });
        self
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::light();
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(WEAK);
    v.panel_fill = GREY;
    // Drip has no egui windows, so this styles menus and dropdowns.
    v.window_fill = LIGHTEST;
    v.window_stroke = Stroke::new(1.0, WEAK);
    v.faint_bg_color = GREY;
    v.extreme_bg_color = DARKER;
    v.code_bg_color = DARKER;
    v.error_fg_color = ERROR;
    v.warn_fg_color = ERROR;
    v.hyperlink_color = LINK;
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
    fonts(ctx);
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

/// Installs Drip's fonts, which `apply` also does; for contexts that keep
/// egui's style, such as tests.
pub fn fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();
    for (family, name, data) in [
        (
            FontFamily::Proportional,
            "Source Sans 3",
            &include_bytes!("../../../vendor/source-sans/SourceSans3VF-Upright.ttf")[..],
        ),
        (
            FontFamily::Monospace,
            "Source Code Pro",
            &include_bytes!("../../../vendor/source-code-pro/SourceCodeVF-Upright.ttf")[..],
        ),
    ] {
        // Both variable fonts default to ExtraLight; text asks for other weights.
        let tweak =
            FontTweak { coords: VariationCoords::new([(b"wght", 400.0)]), ..Default::default() };
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(data).tweak(tweak)));
        fonts.families.insert(family, vec![name.into()]);
    }
    ctx.set_fonts(fonts);
}
