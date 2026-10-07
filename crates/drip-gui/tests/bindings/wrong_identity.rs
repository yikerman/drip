struct Probe;

impl crate::node_ui::GuiNode for Probe {
    // These nodes share parameters and inputs, but their identities must differ.
    type Node = drip::nodes::scopes::HistogramNode;
    type Prepared = ();
    const NODE: &'static drip::node::TypedNode<Self::Node> = &drip::nodes::WAVEFORM;
}

const _: crate::node_ui::Binding = crate::node_ui::Binding::new::<Probe>();
