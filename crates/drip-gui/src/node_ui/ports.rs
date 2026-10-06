//! User-facing names for the existing edge contracts. These label contracts,
//! not accepted-type lists, so connection checking stays in the library.

use drip::image::{
    CameraRgb, ColorspaceRgbMatrix, DisplayRec2020, LinearThreeChannelMatrix, Mosaic, RawMetadata,
    Rec2020, RgbIn, SceneRec2020, ThreeChannelMatrix,
};
use drip::ports::{Input, Read};

pub fn label(contract: &'static str) -> &'static str {
    const LABELS: &[(&str, &str)] = &[
        (Read::<Mosaic>::REQUIREMENT.name, "sensor mosaic"),
        (Read::<RawMetadata>::REQUIREMENT.name, "RAW metadata"),
        (Read::<CameraRgb>::REQUIREMENT.name, "camera RGB img"),
        (Read::<SceneRec2020>::REQUIREMENT.name, "scn rec2020 img"),
        (Read::<DisplayRec2020>::REQUIREMENT.name, "disp rec2020 img"),
        (Read::<dyn ThreeChannelMatrix>::REQUIREMENT.name, "3-channel img"),
        (Read::<dyn LinearThreeChannelMatrix>::REQUIREMENT.name, "linear 3-channel img"),
        (Read::<dyn ColorspaceRgbMatrix>::REQUIREMENT.name, "linear RGB img + color space"),
        (Read::<dyn RgbIn<Rec2020>>::REQUIREMENT.name, "scn / disp rec2020 img"),
    ];
    LABELS.iter().find(|(name, _)| *name == contract).map_or(contract, |(_, label)| label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_ports_have_user_facing_names() {
        for kind in drip::nodes::registry().kinds() {
            let inputs = kind.inputs().map(|port| port.requirement.name);
            let outputs = kind.outputs().map(|port| port.ty.name);
            for contract in inputs.chain(outputs) {
                assert_ne!(label(contract), contract, "{}: {contract}", kind.id);
            }
        }
    }

    #[test]
    fn labels_fit_default_node_widths() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            let mut graph = drip::graph::Graph::default();
            for kind in drip::nodes::registry().kinds() {
                let id = graph.add_node(kind);
                let width = super::super::of(kind).size(graph.node(id).unwrap()).x - 16.0;
                let inputs = kind.inputs().map(|port| port.requirement.name);
                let outputs = kind.outputs().map(|port| port.ty.name);
                for contract in inputs.chain(outputs) {
                    let text = label(contract);
                    let galley = ui.fonts_mut(|fonts| {
                        fonts.layout_no_wrap(
                            text.into(),
                            egui::FontId::proportional(crate::theme::SMALL_SIZE),
                            crate::theme::WEAK,
                        )
                    });
                    assert!(galley.size().x <= width, "{}: {text}", kind.id);
                }
            }
        })
        .textures_delta
        .clear();
    }
}
