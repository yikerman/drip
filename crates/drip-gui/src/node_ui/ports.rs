//! Port text and colors derived from the library's declared payloads and
//! requirements. Connection checking stays in the library; this only presents
//! its contracts.

use crate::model::Kind;
use drip::node::data::{Bayer, BayerLevels, CaptureMetadata, Rgb};
use drip::node::field::RgbField;
use drip::ports::PortSpec;
use egui::Color32;

#[derive(Clone, Copy)]
pub enum PortRole {
    Input,
    OptionalInput,
    Output,
}

impl PortRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "Input",
            Self::OptionalInput => "Optional input",
            Self::Output => "Output",
        }
    }
}

/// One port as the editor and help show it.
pub struct PortText {
    pub role: PortRole,
    pub port: &'static str,
    /// Accepted input types or the declared output type.
    pub label: String,
    pub color: Color32,
}

impl PortText {
    /// The role and port name; a name that only repeats the role is omitted.
    pub fn heading(&self) -> String {
        let role = self.role.label();
        if self.port.eq_ignore_ascii_case(role) {
            role.into()
        } else {
            format!("{role} · {}", self.port)
        }
    }

    pub fn hover(&self) -> String {
        let requires = match self.role {
            PortRole::Input | PortRole::OptionalInput => "requires ",
            PortRole::Output => "",
        };
        format!("{}: {requires}{}", self.heading(), self.label)
    }
}

/// One color per payload, the type ports must share to connect. Requirements
/// are checked on evaluation and only named in hover text. Tints stay near
/// neutral so they do not bias color judgment against the grey surround.
pub fn color(spec: &PortSpec) -> Option<Color32> {
    [
        (spec.is::<Rgb>(), Color32::from_rgb(172, 166, 146)),
        (spec.is::<RgbField>(), Color32::from_rgb(178, 158, 150)),
        (spec.is::<Bayer>(), Color32::from_rgb(150, 164, 150)),
        (spec.is::<BayerLevels>(), Color32::from_rgb(148, 156, 170)),
        (spec.is::<CaptureMetadata>(), Color32::from_rgb(164, 150, 166)),
    ]
    .into_iter()
    .find_map(|(is, color)| is.then_some(color))
}

/// Inputs then outputs, in the editor's row order. Every output type is known
/// before evaluation, including on nodes with missing inputs.
pub fn texts(kind: &Kind) -> Vec<PortText> {
    let text = |role, spec: &PortSpec| PortText {
        role,
        port: spec.name,
        label: spec.label(),
        color: color(spec).unwrap_or(crate::theme::TEXT),
    };
    let inputs = kind.inputs.iter().map(|input| {
        text(if input.optional { PortRole::OptionalInput } else { PortRole::Input }, input)
    });
    let outputs = kind.outputs.iter().map(|output| text(PortRole::Output, output));
    inputs.chain(outputs).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Graph;

    #[test]
    fn concrete_outputs_are_known_before_inputs_are_connected() {
        let mut graph = Graph::default();
        for (kind, label) in [
            (&drip::node::sigmoid::apply::KIND, "RGB · Rec.2020 / D65"),
            (&drip::node::exposure::compute::KIND, "RGB"),
        ] {
            let id = graph.add_node(kind).unwrap();
            let ports = texts(graph.kind(id).unwrap());
            assert_eq!(ports[0].hover(), format!("Input · image: requires {label}"));
            assert_eq!(ports.last().unwrap().hover(), format!("Output: {label}"));
        }
    }

    #[test]
    fn every_registered_payload_has_a_color() {
        for kind in crate::registry::REGISTRY.kinds() {
            for spec in kind.inputs.iter().chain(kind.outputs) {
                assert!(color(spec).is_some(), "{}.{}: {}", kind.id, spec.name, spec.payload);
            }
        }
    }
}
