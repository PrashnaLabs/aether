use crate::theme::Corners;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, button::ButtonVariants, h_flex, v_flex,
};
use gpui_kit::rems;
use gpui_kit::{AnyElement, App, ClickEvent, Role, SharedString, Window, div, prelude::*, px};

use crate::components::{ClickHandler, Column};

/// Scrollable settings screen: a titled header with an optional back button,
/// followed by sections.
#[derive(IntoElement)]
pub struct SettingsPage {
    title: SharedString,
    subtitle: Option<SharedString>,
    sidebar: Option<AnyElement>,
    on_back: Option<ClickHandler>,
    actions: Vec<AnyElement>,
    children: Vec<AnyElement>,
    fill: bool,
}

impl SettingsPage {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            sidebar: None,
            on_back: None,
            actions: Vec::new(),
            children: Vec::new(),
            fill: false,
        }
    }

    /// Add fixed-width navigation beside the scrollable content canvas.
    pub fn sidebar(mut self, sidebar: impl IntoElement) -> Self {
        self.sidebar = Some(sidebar.into_any_element());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Fit the window instead of scrolling, so a child list can take the
    /// remaining height and scroll on its own.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// Adds a control to the right of the title, such as a "New" button.
    pub fn action(mut self, element: impl IntoElement) -> Self {
        self.actions.push(element.into_any_element());
        self
    }

    pub fn on_back(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_back = Some(Box::new(handler));
        self
    }
}

impl ParentElement for SettingsPage {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingsPage {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some(sidebar) = self.sidebar {
            return h_flex()
                .id("settings-page")
                .size_full()
                .min_h_0()
                .items_start()
                .child(div().w(px(240.)).h_full().flex_none().child(sidebar))
                .child(
                    div()
                        .id("settings-canvas")
                        .role(Role::TabPanel)
                        .aria_label(self.title.clone())
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .p_8()
                        .child(
                            v_flex()
                                .w_full()
                                .max_w(px(860.))
                                .mx_auto()
                                .gap_8()
                                .child(
                                    h_flex()
                                        .items_start()
                                        .gap_4()
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w_0()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_2xl()
                                                        .font_semibold()
                                                        .child(self.title),
                                                )
                                                .children(self.subtitle.map(|subtitle| {
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(subtitle)
                                                })),
                                        )
                                        .children(self.actions),
                                )
                                .children(self.children),
                        ),
                )
                .into_any_element();
        }
        v_flex()
            .id("settings-page")
            .size_full()
            .when(!self.fill, |page| page.overflow_y_scroll())
            .child(
                Column::new()
                    .py_8()
                    .flex()
                    .flex_col()
                    .gap_8()
                    .when(self.fill, |column| column.h_full().min_h_0())
                    .child(
                        h_flex()
                            .gap_2()
                            .when_some(self.on_back, |row, handler| {
                                row.child(
                                    crate::components::button("settings-back")
                                        .ghost()
                                        .small()
                                        .icon(IconName::ArrowLeft)
                                        .tooltip("Back to chat")
                                        .on_click(handler),
                                )
                            })
                            .child(div().flex_1().text_2xl().font_semibold().child(self.title))
                            .children(self.actions),
                    )
                    .children(self.children),
            )
            .into_any_element()
    }
}

/// A titled card grouping related settings rows.
#[derive(IntoElement)]
pub struct SettingsSection {
    title: SharedString,
    icon: Option<Icon>,
    rows: Vec<AnyElement>,
}

impl SettingsSection {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), icon: None, rows: Vec::new() }
    }

    /// Shows the icon before the title, which turns uppercase.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

pub(crate) fn section_surface(cx: &App) -> gpui_kit::Hsla {
    let theme = cx.theme();
    if theme.is_dark() { theme.secondary.opacity(0.5) } else { theme.background }
}

impl ParentElement for SettingsSection {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.rows.extend(elements);
    }
}

impl RenderOnce for SettingsSection {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let surface = section_surface(cx);
        let title = match self.icon {
            Some(_) => self.title.to_uppercase().into(),
            None => self.title,
        };
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .px_1()
                    .gap_2()
                    .text_sm()
                    .font_medium()
                    .when(self.icon.is_some(), |header| header.font_semibold())
                    .text_color(theme.muted_foreground)
                    .children(self.icon.map(|icon| icon.size_4().flex_none()))
                    .child(title),
            )
            .child(
                v_flex().corner(rems(0.75)).border_1().border_color(border).bg(surface).children(
                    self.rows.into_iter().enumerate().map(|(ix, row)| {
                        div().when(ix > 0, |row| row.border_t_1().border_color(border)).child(row)
                    }),
                ),
            )
    }
}

/// One setting: title and description on the left, control on the right,
/// or below when `stacked` for wide controls.
#[derive(IntoElement)]
pub struct SettingsRow {
    title: SharedString,
    description: Option<SharedString>,
    control: Option<AnyElement>,
    icon: Option<Icon>,
    stacked: bool,
}

impl SettingsRow {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), description: None, control: None, icon: None, stacked: false }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }
}

impl RenderOnce for SettingsRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_1p5()
                    .children(self.icon.map(|icon| icon.size_4().flex_none()))
                    .child(div().text_sm().font_medium().child(self.title)),
            )
            .children(self.description.map(|description| {
                div().text_xs().text_color(cx.theme().muted_foreground).child(description)
            }));
        if self.stacked {
            return v_flex().gap_3().p_4().child(text).children(self.control).into_any_element();
        }
        h_flex()
            .gap_6()
            .p_4()
            .child(text)
            .children(self.control.map(|control| div().flex_none().child(control)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_page_single_column_preserves_structure() {
        let page = SettingsPage::new("Agents").fill().child(div());
        assert!(page.sidebar.is_none());
        assert!(page.fill);
        assert_eq!(page.children.len(), 1);
    }

    #[test]
    fn settings_page_supports_sidebar_slot() {
        let page = SettingsPage::new("Appearance")
            .sidebar(div().id("test-sidebar"))
            .subtitle("Manage preferences")
            .action(div())
            .child(div());
        assert!(page.sidebar.is_some());
        assert_eq!(page.subtitle.as_deref(), Some("Manage preferences"));
        assert_eq!(page.actions.len(), 1);
        assert_eq!(page.children.len(), 1);
    }
}
