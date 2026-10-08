//! Port text derived from the library's declared concrete payload names.
//! Connection checking stays in the library; this only names its contracts.

use drip::graph::{Graph, NodeId};
use drip::value::{Residence, TypeDescriptor};

#[derive(Clone, Copy)]
pub enum PortRole {
    Input,
    OptionalInput,
    Output,
}

impl PortRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::OptionalInput => "optional input",
            Self::Output => "output",
        }
    }
}

/// One port as the editor and help show it.
pub struct PortText {
    pub role: PortRole,
    pub port: &'static str,
    /// Payload family names kept compact for graph rows. Runtime meaning is
    /// carried by values; these names do not assert a color space or scene model.
    pub label: String,
    contract: String,
}

impl PortText {
    pub fn contract(&self) -> &str {
        &self.contract
    }

    pub fn hover(&self) -> String {
        let requires = match self.role {
            PortRole::Input | PortRole::OptionalInput => "requires ",
            PortRole::Output => "",
        };
        format!("{} {}: {requires}{}", self.role.label(), self.port, self.contract)
    }
}

/// Inputs then outputs, in the editor's row order. Every output type is known
/// before evaluation, including on nodes with missing inputs.
pub fn texts(graph: &Graph, id: NodeId) -> Vec<PortText> {
    let kind = graph.node(id).expect("existing node").kind;
    let inputs = kind.inputs().map(|input| PortText {
        role: if input.requirement.optional { PortRole::OptionalInput } else { PortRole::Input },
        port: input.name,
        label: input.requirement.name(),
        contract: input.requirement.types.iter().map(describe).collect::<Vec<_>>().join(" or "),
    });
    let outputs = kind.outputs().map(|output| PortText {
        role: PortRole::Output,
        port: output.name,
        label: output.ty.name.into(),
        contract: describe(&output.ty),
    });
    inputs.chain(outputs).collect()
}

fn describe(ty: &TypeDescriptor) -> String {
    let placement = match ty.residence {
        Residence::Host => "host record",
        Residence::Cpu => "CPU",
        Residence::Gpu => "GPU",
    };
    format!("{} ({placement})", ty.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip::nodes;

    #[test]
    fn concrete_outputs_are_known_before_inputs_are_connected() {
        let mut graph = Graph::default();
        for kind in [nodes::SIGMOID.kind(), nodes::EXPOSURE.kind()] {
            let id = graph.add_node(kind);
            let ports = texts(&graph, id);
            assert_eq!(ports[0].hover(), "input image: requires Color image (GPU)");
            assert_eq!(ports[1].hover(), "output image: Color image (GPU)");
        }
    }

    #[test]
    fn host_records_and_device_consumers_do_not_claim_color_meaning() {
        let mut graph = Graph::default();
        let source = graph.add_node(&nodes::READ);
        let outputs = texts(&graph, source);
        assert_eq!(outputs[0].contract(), "Sensor mosaic (CPU)");
        assert_eq!(outputs[1].contract(), "Raw metadata (host record)");
        let preview = graph.add_node(&nodes::PREVIEW);
        assert_eq!(texts(&graph, preview)[0].contract(), "Color image (GPU)");
    }

    #[test]
    fn labels_fit_default_node_widths() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            let mut graph = Graph::default();
            for kind in nodes::registry().kinds() {
                let id = graph.add_node(kind);
                let width = super::super::of(kind).size(graph.node(id).unwrap()).x - 16.0;
                for text in texts(&graph, id).into_iter().map(|t| t.label) {
                    let galley = ui.fonts_mut(|fonts| {
                        fonts.layout_no_wrap(
                            text.clone(),
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
