//! Frontend-owned access to opaque persisted UI data. Invalid layout falls back
//! locally; invalid processing detail rejects the project before replacing it.

use drip::graph::NodeId;
use egui::{Vec2, vec2};
use serde_json::{Value as Json, json};

/// Preview levels never go coarser than 1/256 of the sensor.
pub const MAX_LEVEL: u8 = 8;
pub const DEFAULT_LEVEL: u8 = 1;
pub const MIN_VIEW_SIZE: Vec2 = vec2(80.0, 60.0);
const DEFAULT_VIEW_SIZE: Vec2 = vec2(280.0, 190.0);
const PREVIEW_LEVEL: &str = "preview_level";

#[derive(Clone, Copy)]
pub enum LayoutField {
    Position,
    ViewSize,
}

impl LayoutField {
    fn key(self) -> &'static str {
        match self {
            Self::Position => "pos",
            Self::ViewSize => "size",
        }
    }

    fn read(self, ui: &Json) -> Option<Vec2> {
        let [x, y] = ui.get(self.key())?.as_array()?.as_slice() else { return None };
        let value = vec2(x.as_f64()? as f32, y.as_f64()? as f32);
        value.is_finite().then_some(value)
    }

    pub fn write(self, ui: &mut Json, value: Vec2) {
        set(ui, self.key(), json!([value.x, value.y]));
    }
}

pub fn position(ui: &Json, id: NodeId) -> Vec2 {
    LayoutField::Position
        .read(ui)
        .unwrap_or_else(|| vec2(40.0 + 200.0 * (id.0 % 5) as f32, 40.0 + 140.0 * (id.0 / 5) as f32))
}

pub fn view_size(ui: &Json) -> Vec2 {
    LayoutField::ViewSize
        .read(ui)
        .filter(|size| size.x >= MIN_VIEW_SIZE.x && size.y >= MIN_VIEW_SIZE.y)
        .unwrap_or(DEFAULT_VIEW_SIZE)
}

pub fn preview_level(ui: &Json) -> Result<u8, String> {
    if !ui.is_null() && !ui.is_object() {
        return Err("project UI state must be an object".into());
    }
    match ui.get(PREVIEW_LEVEL) {
        None => Ok(DEFAULT_LEVEL),
        Some(value) => value
            .as_u64()
            .filter(|&level| level <= u64::from(MAX_LEVEL))
            .map(|level| level as u8)
            .ok_or_else(|| format!("preview level must be an integer from 0 to {MAX_LEVEL}")),
    }
}

pub fn set_preview_level(ui: &mut Json, level: u8) {
    set(ui, PREVIEW_LEVEL, json!(level));
}

fn set(ui: &mut Json, key: &str, value: Json) {
    if !ui.is_object() {
        *ui = json!({});
    }
    ui[key] = value;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_handles_invalid_saved_coordinates_without_losing_other_state() {
        let mut ui = json!({"pos": [1e300, 2], "size": [-1, 200], "other": {"keep": true}});
        assert_eq!(position(&ui, NodeId(6)), vec2(240.0, 180.0));
        assert_eq!(view_size(&ui), DEFAULT_VIEW_SIZE);
        LayoutField::Position.write(&mut ui, vec2(-20.0, 50.0));
        LayoutField::ViewSize.write(&mut ui, vec2(100.0, 200.0));
        assert_eq!(position(&ui, NodeId(6)), vec2(-20.0, 50.0));
        assert_eq!(view_size(&ui), vec2(100.0, 200.0));
        assert_eq!(ui["other"], json!({"keep": true}));
        for value in [Json::Null, json!([]), json!({"pos": [1, 2, 3], "size": ["a", 2]})] {
            assert_eq!(position(&value, NodeId(0)), vec2(40.0, 40.0));
            assert_eq!(view_size(&value), DEFAULT_VIEW_SIZE);
        }
    }

    #[test]
    fn detail_state_defaults_only_when_absent_and_preserves_unknown_fields() {
        assert_eq!(preview_level(&Json::Null).unwrap(), DEFAULT_LEVEL);
        let mut ui = json!({"other": 5});
        set_preview_level(&mut ui, MAX_LEVEL);
        assert_eq!(preview_level(&ui).unwrap(), MAX_LEVEL);
        assert_eq!(ui["other"], 5);
        for value in [json!([]), json!({"preview_level": null}), json!({"preview_level": 9})] {
            assert!(preview_level(&value).is_err());
        }
    }
}
