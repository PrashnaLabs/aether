use crate::theme::Corners;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme, Icon, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ElementId, Role, SharedString, Window, div, prelude::*, px, transparent_black,
};

use crate::overlay;
use crate::theme::CONTENT_WIDTH;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A compact recent chat or commit, shared by the start screen and project page.
#[derive(IntoElement)]
pub struct RecentRow {
    id: ElementId,
    icon: Icon,
    title: SharedString,
    detail: Option<(Icon, SharedString)>,
    when: SharedString,
    label: SharedString,
    on_click: Option<Handler>,
}

impl RecentRow {
    pub fn new(
        id: impl Into<ElementId>,
        icon: Icon,
        title: impl Into<SharedString>,
        when: impl Into<SharedString>,
    ) -> Self {
        let title = title.into();
        let when = when.into();
        let label = format!("{title}, {when}").into();
        Self { id: id.into(), icon, title, detail: None, when, label, on_click: None }
    }

    pub fn detail(mut self, icon: Icon, text: impl Into<SharedString>) -> Self {
        self.detail = Some((icon, text.into()));
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for RecentRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, fg, ring, mono) =
            (theme.muted_foreground, theme.foreground, theme.ring, theme.mono_font_family.clone());
        let hover = overlay(cx);
        h_flex()
            .id(self.id)
            .role(if self.on_click.is_some() { Role::Link } else { Role::ListItem })
            .aria_label(self.label)
            .h(px(38.))
            .px(px(8.))
            .gap(px(10.))
            .items_center()
            .corner(px(10.))
            .border_1()
            .border_color(transparent_black())
            .text_size(px(13.))
            .text_color(muted)
            .child(self.icon.size(px(14.)).flex_none())
            .child(div().flex_1().min_w_0().truncate().text_color(fg).child(self.title))
            .when_some(self.detail, |row, (icon, text)| {
                row.child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .flex_none()
                        .text_size(px(12.))
                        .font_family(mono)
                        .child(icon.size(px(11.)))
                        .child(text),
                )
            })
            .child(div().w(px(64.)).flex_none().text_right().text_size(px(12.)).child(self.when))
            .when_some(self.on_click, |row, on_click| {
                row.tab_index(0)
                    .focus_visible(move |s| s.border_color(ring))
                    .active(move |s| s.bg(hover))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .on_click(move |_, window, cx| on_click(window, cx))
            })
    }
}

struct Row {
    icon: Icon,
    title: SharedString,
    branch: SharedString,
    when: SharedString,
    on_click: Handler,
}

/// The start screen's list of recent chats in a project.
#[derive(IntoElement)]
pub struct RecentChats {
    heading: SharedString,
    view_all: Option<(SharedString, Handler)>,
    rows: Vec<Row>,
}

impl RecentChats {
    pub fn new(heading: impl Into<SharedString>) -> Self {
        Self { heading: heading.into(), view_all: None, rows: Vec::new() }
    }

    pub fn view_all(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.view_all = Some((label.into(), Rc::new(handler)));
        self
    }

    pub fn row(
        mut self,
        icon: Icon,
        title: impl Into<SharedString>,
        branch: impl Into<SharedString>,
        when: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.rows.push(Row {
            icon,
            title: title.into(),
            branch: branch.into(),
            when: when.into(),
            on_click: Rc::new(on_click),
        });
        self
    }
}

pub fn recent_row_label(title: &str, branch: &str, when: &str) -> String {
    format!("Open {title}, {branch}, {when}")
}

impl RenderOnce for RecentChats {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, fg) = (theme.muted_foreground, theme.foreground);
        let ring = theme.ring;
        let rows = self.rows.into_iter().enumerate().map(|(ix, row)| {
            let on_click = row.on_click;
            let label = recent_row_label(&row.title, &row.branch, &row.when);
            RecentRow::new(("recent", ix), row.icon, row.title, row.when)
                .detail(Icon::new(Lucide::GitBranch), row.branch)
                .accessibility_label(label)
                .on_click(move |window, cx| on_click(window, cx))
                .into_any_element()
        });
        let view_all = self.view_all.map(|(label, handler)| {
            div()
                .id("recent-view-all")
                .role(Role::Link)
                .tab_index(0)
                .aria_label(label)
                .corner(px(6.))
                .px(px(4.))
                .border_1()
                .border_color(transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .child("View all")
                .on_click(move |_, window, cx| handler(window, cx))
                .into_any_element()
        });
        v_flex()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .mx_auto()
            .px(px(24.))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .mx(px(8.))
                    .mb(px(6.))
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(self.heading)
                    .children(view_all),
            )
            .children(rows.collect::<Vec<AnyElement>>())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/recent.rs"]
mod tests;
