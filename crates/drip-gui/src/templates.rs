//! Built-in templates: `.drip-template` files from the repository's
//! `templates/`, embedded in the binary and listed by hand. They load through
//! the same path as template files a user opens.
use crate::model::Project;

pub struct Template {
    pub name: &'static str,
    text: &'static str,
}

/// Every built-in template, in menu order. The first is what New creates.
pub static TEMPLATES: &[Template] = &[Template {
    name: "Neutral restore",
    text: include_str!("../../../templates/neutral-restore.drip-template"),
}];

impl Template {
    pub fn project(&self) -> drip::Result<Project> {
        Project::from_json(self.text)
    }
}

/// The project New creates.
pub fn default_project() -> Project {
    TEMPLATES[0].project().expect("built-in templates load")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn built_in_templates_load_as_saved() {
        for template in TEMPLATES {
            let project = template.project().unwrap();
            assert_eq!(project.to_json().unwrap(), template.text, "{} is stale", template.name);
        }
    }

    #[test]
    fn raw_adjustments_are_shared_by_views_and_export_and_survive_reopening() {
        let mut project = default_project();
        let raw = project.graph.find("Bayer RAW").unwrap();
        let orientation = project.graph.find("Orientation").unwrap();
        let color = project.graph.find("Camera RGB to color RGB").unwrap();
        let adaptation = project.graph.find("Chromatic adaptation").unwrap();
        let gains = project.graph.find("Bayer site gains").unwrap();
        let highlights = project.graph.find("Opposed highlights").unwrap();
        assert!(project.graph.find("Apply clipping-level gains").is_none());
        let exposure = project.graph.find("Exposure").unwrap();
        let sigmoid = project.graph.find("Sigmoid").unwrap();
        assert_eq!(project.graph.node(orientation).unwrap().params["orientation"], "from_metadata");
        for (from, output, to, input) in [
            (raw, "metadata", orientation, "metadata"),
            (raw, "metadata", gains, "metadata"),
            (raw, "metadata", color, "metadata"),
            (gains, "levels", highlights, "levels"),
            (color, "output", adaptation, "image"),
            (raw, "metadata", adaptation, "metadata"),
            (adaptation, "output", orientation, "image"),
            (orientation, "output", exposure, "image"),
        ] {
            assert_eq!(
                project.graph.source(project.graph.dag.input_port(to, input).unwrap()),
                Some(project.graph.dag.output_port(from, output).unwrap())
            );
        }
        for name in ["Preview", "Histogram", "Waveform", "Vectorscope", "Export TIFF"] {
            let id = project.graph.find(name).unwrap();
            assert_eq!(
                project.graph.source(project.graph.dag.input_port(id, "image").unwrap()),
                Some(project.graph.dag.output_port(sigmoid, "output").unwrap())
            );
        }
        project.graph.set_param(orientation, "orientation", json!("rotate_90_clockwise")).unwrap();
        project
            .graph
            .set_param(adaptation, "illuminant", json!({"custom":{"x":0.44757,"y":0.40745}}))
            .unwrap();
        let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
        let orientation = reopened.graph.find("Orientation").unwrap();
        let raw = reopened.graph.find("Bayer RAW").unwrap();
        let adaptation = reopened.graph.find("Chromatic adaptation").unwrap();
        assert_eq!(
            reopened.graph.node(adaptation).unwrap().params["illuminant"],
            json!({"custom":{"x":0.44757,"y":0.40745}})
        );
        assert_eq!(
            reopened.graph.node(orientation).unwrap().params["orientation"],
            "rotate_90_clockwise"
        );
        assert_eq!(
            reopened.graph.source(reopened.graph.dag.input_port(orientation, "metadata").unwrap()),
            Some(reopened.graph.dag.output_port(raw, "metadata").unwrap())
        );
    }
}
