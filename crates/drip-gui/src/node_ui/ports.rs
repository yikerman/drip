//! Port text derived from the library's declared concrete payload names.
//! Connection checking stays in the library; this only names its contracts.

use crate::model::{Graph, NodeId};

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
        role: if input.optional { PortRole::OptionalInput } else { PortRole::Input },
        port: input.name,
        label: label(input),
    });
    let outputs = kind.outputs().map(|output| PortText {
        role: PortRole::Output,
        port: output.name,
        label: label(output),
    });
    inputs.chain(outputs).collect()
}

fn label(spec: &drip::ports::PortSpec) -> String {
    spec.payload_name.replace("drip::node::data::", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concrete_outputs_are_known_before_inputs_are_connected() {
        let mut graph = Graph::default();
        for kind in [
            crate::model::Registry.get("apply-rec2020-sigmoid").unwrap(),
            crate::model::Registry.get("rgb-exposure").unwrap(),
        ] {
            let id = graph.add_node(kind).unwrap();
            let ports = texts(&graph, id);
            assert_eq!(ports[0].hover(), "input image: requires ColorRgb");
            assert_eq!(ports.last().unwrap().hover(), "output output: ColorRgb");
        }
    }

    #[test]
    fn canvas_contract_rows_fit_default_node_widths() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            let mut graph = Graph::default();
            for kind in crate::model::Registry.kinds() {
                let id = graph.add_node(kind).unwrap();
                let width = super::super::of(kind).size(&graph.node(id).unwrap()).x - 16.0;
                for text in texts(&graph, id).into_iter().map(|t| t.label) {
                    let galley = ui.fonts_mut(|fonts| {
                        fonts.layout_job({
                            let mut job = egui::text::LayoutJob::simple(
                                text.clone(),
                                egui::FontId::proportional(crate::theme::SMALL_SIZE),
                                crate::theme::WEAK,
                                width,
                            );
                            job.wrap.max_rows = 1;
                            job.wrap.break_anywhere = true;
                            job
                        })
                    });
                    assert!(galley.size().x <= width, "{}: {text}", kind.id);
                }
            }
        })
        .textures_delta
        .clear();
    }
}
