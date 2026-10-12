//! Node picker: a palette over the window that searches registered kinds by
//! name, ID and tag, narrows by tags, and chooses with the pointer or
//! Up/Down/Enter. Independent of graph edits; the editor places the node.

use crate::model::Kind;
use crate::registry::REGISTRY;
use crate::theme::{self, Tip};
use drip::definition::Tag;
use egui::epaint::RectShape;
use egui::{
    Button, Context, Frame, Id, Key, Label, Modifiers, Response, RichText, ScrollArea, Sense,
    Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};

pub(super) struct Picker {
    /// Where the chosen node goes, in graph units; `None` is the centre of the
    /// visible canvas.
    pub at: Option<Vec2>,
    query: String,
    tags: Vec<&'static Tag>,
    /// The highlighted result, chosen by Enter.
    cursor: usize,
}

pub(super) enum Picked {
    Kind(&'static Kind),
    Closed,
}

impl Picker {
    pub fn new(at: Option<Vec2>) -> Self {
        Picker { at, query: String::new(), tags: Vec::new(), cursor: 0 }
    }

    /// Draws the palette as a modal sized to the window, which keeps input
    /// from the canvas and closes on Escape or a click outside; `None` keeps
    /// it open.
    pub fn show(&mut self, ctx: &Context) -> Option<Picked> {
        let key = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let (down, up, enter) = (key(Key::ArrowDown), key(Key::ArrowUp), key(Key::Enter));
        let found = matches(&self.query, &self.tags);
        let last = found.len().saturating_sub(1);
        self.cursor = (self.cursor + usize::from(down)).saturating_sub(usize::from(up)).min(last);
        let screen = ctx.content_rect().size();
        let mut clicked = None;
        let modal = egui::Modal::new(Id::new("node picker")).show(ctx, |ui| {
            ui.set_width((screen.x * 0.6).clamp(420.0, 1000.0));
            let search = ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("Search nodes")
                    .desired_width(f32::INFINITY),
            );
            search.request_focus();
            // The chips are every used tag, so wrapping keeps a constant height.
            ui.horizontal_wrapped(|ui| {
                for &tag in used_tags() {
                    let on = self.tags.iter().any(|t| t.id == tag.id);
                    if chip(ui, tag, on).clicked() {
                        self.tags.retain(|t| t.id != tag.id);
                        if !on {
                            self.tags.push(tag);
                        }
                        self.cursor = 0;
                    }
                }
            });
            ui.separator();
            // The whole palette takes 85% of the height; the list gets what the
            // search and chips leave.
            let height = screen.y * 0.85 - ui.min_rect().height();
            ScrollArea::vertical()
                .min_scrolled_height(height)
                .max_height(height)
                .auto_shrink(false)
                .show(ui, |ui| {
                    for (i, &kind) in found.iter().enumerate() {
                        let row = row(ui, kind, i == self.cursor);
                        if i == self.cursor && (down || up) {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            clicked = Some(kind);
                        }
                    }
                    if found.is_empty() {
                        ui.weak("No matching nodes");
                    }
                });
            if search.changed() {
                self.cursor = 0;
            }
        });
        let chosen = clicked.or_else(|| enter.then(|| found.get(self.cursor).copied()).flatten());
        chosen.map(Picked::Kind).or(modal.should_close().then_some(Picked::Closed))
    }
}

/// A tag filter; a chosen one is dark, like a pressed key.
fn chip(ui: &mut Ui, tag: &Tag, on: bool) -> Response {
    let (fill, text) =
        if on { (theme::TEXT, theme::LIGHTEST) } else { (theme::DARKER, theme::TEXT) };
    ui.add(Button::new(RichText::new(tag.id).color(text)).fill(fill)).tip(tag.description)
}

/// One result as a box: its name and tags, then its help's summary, wrapped. The whole box is clickable; the keyboard's choice is outlined.
fn row(ui: &mut Ui, kind: &Kind, current: bool) -> Response {
    let background = ui.painter().add(Shape::Noop);
    let content = Frame::new().inner_margin(6.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(kind.name).variation("wght", theme::NAME_WEIGHT));
            for tag in kind.tags {
                ui.weak(tag.id).tip(tag.description);
            }
        });
        ui.add(Label::new(RichText::new(summary(kind.help)).small()).wrap());
    });
    let rect = content.response.rect;
    let row = ui.interact(rect, ui.id().with(kind.id), Sense::click());
    let fill = if row.hovered() { theme::LIGHTER } else { theme::DARKER };
    let stroke = if current { Stroke::new(1.0, theme::TEXT) } else { Stroke::NONE };
    ui.painter().set(background, RectShape::new(rect, 0.0, fill, stroke, StrokeKind::Inside));
    row
}

/// The help's first sentence, as Rustdoc summaries go; source wraps become spaces.
fn summary(help: &str) -> String {
    let help = help.replace('\n', " ");
    help.split_inclusive(". ").next().unwrap_or_default().trim_end().to_owned()
}

/// Tags some registered kind carries, in registry order.
fn used_tags() -> impl Iterator<Item = &'static &'static Tag> {
    drip::registry::TAGS
        .iter()
        .filter(|tag| REGISTRY.kinds().any(|kind| kind.tags.iter().any(|t| t.id == tag.id)))
}

/// Kinds carrying every selected tag that match the query, best first: names
/// starting with it, then names with a word starting with it, then any other
/// match in the name, ID or tags. Ties and an empty query sort by name.
fn matches(query: &str, tags: &[&Tag]) -> Vec<&'static Kind> {
    let query = query.trim().to_lowercase();
    let mut found: Vec<_> = REGISTRY
        .kinds()
        .filter(|kind| tags.iter().all(|tag| kind.tags.iter().any(|t| t.id == tag.id)))
        .filter_map(|kind| rank(kind, &query).map(|rank| (rank, kind)))
        .collect();
    found.sort_by_key(|&(rank, kind)| (rank, kind.name));
    found.into_iter().map(|(_, kind)| kind).collect()
}

fn rank(kind: &Kind, query: &str) -> Option<usize> {
    let name = kind.name.to_lowercase();
    let mut words = name.split(|c: char| !c.is_alphanumeric());
    let elsewhere = || kind.id.contains(query) || kind.tags.iter().any(|t| t.id.contains(query));
    [name.starts_with(query), words.any(|word| word.starts_with(query)), name.contains(query)]
        .into_iter()
        .chain([elsewhere()])
        .position(|hit| hit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip::registry::tag;

    fn names(query: &str, tags: &[&Tag]) -> Vec<&'static str> {
        matches(query, tags).iter().map(|kind| kind.name).collect()
    }

    #[test]
    fn ranks_name_prefixes_then_words_then_ids_and_tags() {
        let found = names("demosaic", &[]);
        assert_eq!(found[..2], ["Bayer 2×2 demosaic", "RCD demosaic"]);
        assert_eq!(names("rcd", &[])[0], "RCD demosaic");
        assert_eq!(names("RAW", &[])[0], "RAW", "case-insensitive, prefix first");
        assert!(names("raw", &[]).contains(&"Opposed highlights"), "tags match");
        assert!(names("tiff", &[]).contains(&"Export TIFF"));
        assert!(names("export-tiff", &[]).contains(&"Export TIFF"), "IDs match");
        assert!(names("zzz", &[]).is_empty());
    }

    #[test]
    fn rows_summarize_help_by_its_first_sentence() {
        assert_eq!(
            summary("Reduce Bayer by averaging.\nOutput keeps it. More."),
            "Reduce Bayer by averaging."
        );
        assert_eq!(summary("Display linear\nRec.2020/D65 RGB"), "Display linear Rec.2020/D65 RGB");
    }

    #[test]
    fn selected_tags_must_all_be_carried() {
        assert_eq!(names("", &[&tag::LOOK]), ["Exposure"]);
        assert_eq!(names("", &[&tag::RAW, &tag::DEMOSAIC]), ["Bayer 2×2 demosaic", "RCD demosaic"]);
        let all = names("", &[]);
        assert!(all.windows(2).all(|pair| pair[0] <= pair[1]), "an empty query sorts by name");
    }
}
