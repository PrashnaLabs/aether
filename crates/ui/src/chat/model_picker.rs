use gpui_kit::component::{
    ActiveTheme, Sizable, h_flex, searchable_list::SearchableListItem, tag::Tag, v_flex,
};
use gpui_kit::{AnyElement, App, SharedString, Window, prelude::*};

/// Picker row standing for an empty CLI model: no `--model` flag.
pub const CLI_DEFAULT: &str = "Default";

/// Picker row; `detail` reads `"{provider} · {source}"`, or one name when they're the same.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelItem {
    pub id: SharedString,
    pub name: SharedString,
    pub detail: SharedString,
    pub default: bool,
    pub description: SharedString,
}

impl ModelItem {
    /// A model the catalog doesn't list, kept so it stays selectable.
    pub fn custom(id: impl Into<SharedString>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            detail: "Custom".into(),
            default: false,
            description: "".into(),
        }
    }
}

impl SearchableListItem for ModelItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        if self.default { format!("{} (default)", self.name).into() } else { self.name.clone() }
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(self.name.clone().into_any_element())
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex().min_w_0().child(self.name.clone()).child(
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(self.detail.clone())
                .when(self.default, |row| row.child(Tag::secondary().small().child("default"))),
        )
    }

    fn value(&self) -> &SharedString {
        &self.id
    }

    fn matches(&self, query: &str) -> bool {
        let haystack = format!("{}\n{}\n{}", self.name, self.id, self.detail).to_lowercase();
        query.to_lowercase().split_whitespace().all(|token| haystack.contains(token))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/model_picker.rs"]
mod tests;
