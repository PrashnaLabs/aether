use gpui_kit::component::IndexPath;
use gpui_kit::component::select::{SearchableVec, SelectItem, SelectState};
use gpui_kit::{App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div};

#[derive(Clone)]
pub(crate) struct FontItem {
    pub name: SharedString,
    value: SharedString,
}

impl SelectItem for FontItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &SharedString {
        &self.value
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let row = div().child(self.name.clone());
        if self.value.is_empty() { row } else { row.font_family(self.value.clone()) }
    }
}

pub(crate) fn font_choices(installed: &[String]) -> Vec<FontItem> {
    let mut names: Vec<&String> =
        installed.iter().filter(|n| !n.is_empty() && !n.starts_with('.')).collect();
    names.sort_by_cached_key(|n| (n.to_lowercase(), *n));
    names.dedup();
    std::iter::once(FontItem { name: "Default".into(), value: "".into() })
        .chain(
            names.into_iter().map(|n| FontItem { name: n.clone().into(), value: n.clone().into() }),
        )
        .collect()
}

pub(crate) fn font_row(choices: &[FontItem], family: &str) -> IndexPath {
    IndexPath::default().row(choices.iter().position(|f| f.value == family).unwrap_or(0))
}

pub(crate) fn fill_font_select(
    select: &Entity<SelectState<SearchableVec<FontItem>>>,
    choices: Vec<FontItem>,
    family: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let row = font_row(&choices, family);
    select.update(cx, |select, cx| {
        select.set_items(SearchableVec::new(choices), window, cx);
        select.set_selected_index(Some(row), window, cx);
    });
}

#[cfg(test)]
#[path = "../tests/unit/font_picker.rs"]
mod tests;
