//! Port text derived from the library's declared concrete payload names.
//! Connection checking stays in the library; this only names its contracts.

use drip::graph::{Graph, NodeId};

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
    /// Accepted input types or the declared output type.
    pub label: String,
}

impl PortText {
    pub fn hover(&self) -> String {
        let requires = match self.role {
            PortRole::Input | PortRole::OptionalInput => "requires ",
            PortRole::Output => "",
        };
        format!("{} {}: {requires}{}", self.role.label(), self.port, self.label)
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
    });
    let outputs = kind.outputs().map(|output| PortText {
        role: PortRole::Output,
        port: output.name,
        label: output.ty.name.into(),
    });
    inputs.chain(outputs).collect()
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
            assert_eq!(ports[0].hover(), "input image: requires Rec.2020 RGB");
            assert_eq!(ports[1].hover(), "output image: Rec.2020 RGB");
        }
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
