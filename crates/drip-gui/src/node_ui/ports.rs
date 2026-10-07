//! Port text derived from the library's logical type and capability names.
//! Connection checking stays in the library; this only names its contracts.

use drip::graph::{Graph, NodeId, Port};
use drip::ports::OutputType;

/// Shown for an output whose type depends on an input without a resolved
/// type. Pending is unknown, never evidence of compatibility.
pub const PENDING: &str = "pending";

/// `Rec2020Rgb` → `rec2020 rgb`; names already written as words pass through.
pub fn name(semantic: &str) -> String {
    let mut words = String::with_capacity(semantic.len() + 4);
    let mut previous = None::<char>;
    for c in semantic.chars() {
        if c.is_uppercase() && previous.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
            words.push(' ');
        }
        words.extend(c.to_lowercase());
        previous = Some(c);
    }
    words
}

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
    /// Required type or capability for inputs; resolved type or [`PENDING`]
    /// for outputs.
    pub label: String,
    /// The input a preserving output takes its type from.
    pub origin: Option<&'static str>,
}

impl PortText {
    pub fn hover(&self) -> String {
        let requires = match self.role {
            PortRole::Input | PortRole::OptionalInput => "requires ",
            PortRole::Output => "",
        };
        let mut text = format!("{} {}: {requires}{}", self.role.label(), self.port, self.label);
        if let Some(input) = self.origin {
            text += &format!(", same type as input {input}");
        }
        text
    }
}

/// Inputs then outputs, in the editor's row order. Output types come from
/// the current graph, so preserved types follow their sources.
pub fn texts(graph: &Graph, id: NodeId) -> Vec<PortText> {
    let kind = graph.node(id).expect("existing node").kind;
    let inputs = kind.inputs().map(|input| PortText {
        role: if input.requirement.optional { PortRole::OptionalInput } else { PortRole::Input },
        port: input.name,
        label: name(input.requirement.name),
        origin: None,
    });
    let outputs = kind.outputs().map(|output| PortText {
        role: PortRole::Output,
        port: output.name,
        label: graph
            .output_type(&Port(id, output.name.into()))
            .map_or_else(|| PENDING.into(), |ty| name(ty.name)),
        origin: match output.ty {
            OutputType::Fixed(_) => None,
            OutputType::Preserve(index) => kind.inputs().nth(index).map(|input| input.name),
        },
    });
    inputs.chain(outputs).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip::nodes;

    #[test]
    fn names_split_semantic_identifiers_into_words() {
        for (semantic, expected) in [
            ("Rec2020Rgb", "rec2020 rgb"),
            ("LinearRgb", "linear rgb"),
            ("Rec2020", "rec2020"),
            ("Camera", "camera"),
            ("raw metadata", "raw metadata"),
        ] {
            assert_eq!(name(semantic), expected);
        }
    }

    #[test]
    fn preserved_outputs_follow_the_graph_and_stay_pending_without_a_source() {
        let mut graph = Graph::default();
        let tone = graph.add_node(&nodes::SIGMOID);
        let first = graph.add_node(&nodes::EXPOSURE);
        let second = graph.add_node(&nodes::EXPOSURE);
        let port = |id| Port(id, "image".into());
        graph.connect(port(first), port(second)).unwrap();
        let output = |graph: &Graph, id| texts(graph, id).pop().unwrap();
        for id in [first, second] {
            let text = output(&graph, id);
            assert_eq!((text.label.as_str(), text.origin), (PENDING, Some("image")));
        }
        assert_eq!(
            output(&graph, second).hover(),
            "output image: pending, same type as input image"
        );
        graph.connect(port(tone), port(first)).unwrap();
        for id in [first, second] {
            assert_eq!(output(&graph, id).label, "rec2020");
        }
        assert_eq!(texts(&graph, first)[0].hover(), "input image: requires scale invariant");
    }

    #[test]
    fn labels_fit_default_node_widths() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            let mut graph = Graph::default();
            for kind in nodes::registry().kinds() {
                let id = graph.add_node(kind);
                let width = super::super::of(kind).size(graph.node(id).unwrap()).x - 16.0;
                for text in texts(&graph, id).into_iter().map(|t| t.label).chain([PENDING.into()]) {
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
