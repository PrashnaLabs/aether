use crate::theme::Corners;
use gpui_kit::rems;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, StyledExt, h_flex, tooltip::Tooltip};
use gpui_kit::{
    App, ElementId, KeyDownEvent, Role, SharedString, Toggled, Window, div, prelude::*, px,
};

use super::overlay_strong;

type ChangeHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

struct SegmentedItem {
    label: Option<SharedString>,
    icon: Option<IconName>,
    tooltip: Option<SharedString>,
}

/// Single-choice segmented control supporting full-width and compact pill modes,
/// with optional icons and tooltips.
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    options: Vec<SegmentedItem>,
    selected: usize,
    compact: bool,
    on_change: Option<ChangeHandler>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), options: Vec::new(), selected: 0, compact: false, on_change: None }
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn option(mut self, label: impl Into<SharedString>) -> Self {
        self.options.push(SegmentedItem { label: Some(label.into()), icon: None, tooltip: None });
        self
    }

    pub fn option_with_icon(
        mut self,
        icon: impl Into<IconName>,
        label: impl Into<SharedString>,
    ) -> Self {
        self.options.push(SegmentedItem {
            label: Some(label.into()),
            icon: Some(icon.into()),
            tooltip: None,
        });
        self
    }

    pub fn icon_option(
        mut self,
        icon: impl Into<IconName>,
        tooltip: impl Into<SharedString>,
    ) -> Self {
        self.options.push(SegmentedItem {
            label: None,
            icon: Some(icon.into()),
            tooltip: Some(tooltip.into()),
        });
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// The option an arrow, Home or End key moves to; arrows wrap like a radio group.
fn key_target(key: &str, selected: usize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match key {
        "left" => Some((selected + count - 1) % count),
        "right" => Some((selected + 1) % count),
        "home" => Some(0),
        "end" => Some(count - 1),
        _ => None,
    }
    .filter(|&ix| ix != selected)
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track, fg, muted, border) =
            (theme.muted, theme.foreground, theme.muted_foreground, theme.border);
        let pill = if self.compact {
            overlay_strong(cx)
        } else if theme.is_dark() {
            theme.foreground.opacity(0.16)
        } else {
            theme.background
        };
        let ring = theme.ring;
        let selected = self.selected;
        let compact = self.compact;
        let on_change = self.on_change;
        let count = self.options.len();

        let mut root = div()
            .id(self.id)
            .role(Role::RadioGroup)
            .flex()
            .tab_index(0)
            .border_1()
            .focus_visible(move |s| s.border_color(ring))
            .when_some(on_change.clone(), |el, handler| {
                el.on_key_down(move |ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.modifiers.modified() {
                        return;
                    }
                    if let Some(ix) = key_target(&ev.keystroke.key, selected, count) {
                        cx.stop_propagation();
                        handler(&ix, window, cx);
                    }
                })
            });
        if compact {
            root = root.corner(px(7.)).border_color(border).overflow_hidden();
        } else {
            // 2px + 1px ring border = old 3px inset
            root = root
                .p(px(2.))
                .gap(px(2.))
                .corner(rems(0.5))
                .bg(track)
                .border_color(gpui_kit::transparent_black());
        }

        root.children(self.options.into_iter().enumerate().map(|(ix, item)| {
            let active = ix == selected;
            let aria = item
                .label
                .clone()
                .or_else(|| item.tooltip.clone())
                .unwrap_or_else(|| SharedString::from(format!("Option {ix}")));
            let item_fg = if active { fg } else { muted };
            let icon_only = item.icon.is_some() && item.label.is_none();

            let mut option = h_flex()
                .id(("option", ix))
                .role(Role::RadioButton)
                .aria_label(aria)
                .aria_toggled(if active { Toggled::True } else { Toggled::False })
                .when(active, |el| el.aria_active_descendant())
                .items_center()
                .justify_center()
                .gap_1()
                .cursor_pointer();

            if compact {
                option = option
                    .h(px(24.))
                    .px(if icon_only { px(7.) } else { px(10.) })
                    .text_xs()
                    .text_color(item_fg)
                    .when(active, |el| el.bg(pill).font_medium())
                    .when(!active, |el| el.hover(|s| s.text_color(fg)));
            } else {
                option =
                    option.px_3().py_1().corner(rems(0.375)).text_sm().flex_1().text_center().map(
                        |el| {
                            if active {
                                el.bg(pill).text_color(fg).shadow_xs()
                            } else {
                                el.text_color(muted).hover(|s| s.text_color(fg))
                            }
                        },
                    );
            }

            if let Some(icon) = item.icon {
                option = option.child(
                    Icon::new(icon)
                        .size(if compact { px(13.) } else { px(14.) })
                        .text_color(item_fg)
                        .flex_none(),
                );
            }

            if let Some(label) = item.label {
                option = option.child(label);
            }

            if let Some(tip) = item.tooltip {
                option =
                    option.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
            }

            option.when_some(on_change.clone(), |el, handler| {
                el.on_click(move |_, window, cx| handler(&ix, window, cx))
            })
        }))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/segmented.rs"]
mod tests;
