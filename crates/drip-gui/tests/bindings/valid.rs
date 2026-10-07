struct Probe;

impl crate::node_ui::GuiNode for Probe {
    type Node = drip::nodes::preview::PreviewNode;
    type Presentation = crate::node_ui::preview::PreviewImage;
    const NODE: &'static drip::node::TypedNode<Self::Node> = &drip::nodes::PREVIEW;
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(crate::node_ui::preview::prepare);
}

const _: crate::node_ui::Binding = crate::node_ui::Binding::new::<Probe>();
