//! Node picker: search registered kinds by name, ID and tag, narrow by tags,
//! and choose with the pointer or Up/Down/Enter. Independent of graph edits.

use crate::model::Kind;
use crate::registry::REGISTRY;
use crate::theme::Tip;
use drip::definition::Tag;
use egui::{Context, Id, Key, Modifiers, Order, Pos2, ScrollArea, TextEdit, Vec2};

const WIDTH: f32 = 260.0;
const LIST_HEIGHT: f32 = 320.0;

pub(super) struct Picker {
    /// Where the chosen node goes, in graph units.
    pub at: Vec2,
    /// The picker's top-left corner on screen.
    screen: Pos2,
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
    pub fn new(at: Vec2, screen: Pos2) -> Self {
        Picker { at, screen, query: String::new(), tags: Vec::new(), cursor: 0 }
    }

    /// Draws the picker above the canvas; `None` keeps it open.
    pub fn show(&mut self, ctx: &Context) -> Option<Picked> {
        let key = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let (down, up, enter, escape) =
            (key(Key::ArrowDown), key(Key::ArrowUp), key(Key::Enter), key(Key::Escape));
        let found = matches(&self.query, &self.tags);
        let last = found.len().saturating_sub(1);
        self.cursor = (self.cursor + usize::from(down)).saturating_sub(usize::from(up)).min(last);
        let mut clicked = None;
        let area = egui::Area::new(Id::new("node picker"))
            .order(Order::Foreground)
            .fixed_pos(self.screen)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(WIDTH);
                    let search = ui.add(
                        TextEdit::singleline(&mut self.query)
                            .hint_text("Search nodes")
                            .desired_width(f32::INFINITY),
                    );
                    search.request_focus();
                    ui.horizontal_wrapped(|ui| {
                        for &tag in used_tags() {
                            let on = self.tags.iter().any(|t| t.id == tag.id);
                            if ui.selectable_label(on, tag.id).tip(tag.description).clicked() {
                                self.tags.retain(|t| t.id != tag.id);
                                if !on {
                                    self.tags.push(tag);
                                }
                            }
                        }
                    });
                    ui.separator();
                    ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
                        for (i, &kind) in found.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let row = ui.selectable_label(i == self.cursor, kind.name);
                                let tags: Vec<_> = kind.tags.iter().map(|tag| tag.id).collect();
                                ui.weak(tags.join(" · "));
                                if i == self.cursor && (down || up) {
                                    row.scroll_to_me(None);
                                }
                                if row.clicked() {
                                    clicked = Some(kind);
                                }
                            });
                        }
                        if found.is_empty() {
                            ui.weak("No matching nodes");
                        }
                    });
                    search.changed()
                })
            });
        if area.inner.inner {
            self.cursor = 0;
        }
        let chosen = clicked.or_else(|| enter.then(|| found.get(self.cursor).copied()).flatten());
        let closed = escape || area.response.clicked_elsewhere();
        chosen.map(Picked::Kind).or(closed.then_some(Picked::Closed))
    }
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
    fn selected_tags_must_all_be_carried() {
        assert_eq!(names("", &[&tag::LOOK]), ["Exposure"]);
        assert_eq!(names("", &[&tag::RAW, &tag::DEMOSAIC]), ["Bayer 2×2 demosaic", "RCD demosaic"]);
        let all = names("", &[]);
        assert!(all.windows(2).all(|pair| pair[0] <= pair[1]), "an empty query sorts by name");
    }
}
