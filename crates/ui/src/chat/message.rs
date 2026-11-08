use std::{path::Path, rc::Rc};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::SelectableText;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable,
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    message::MessageAlignment,
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, Div, MouseButton, SharedString, Window, div, linear_color_stop,
    linear_gradient, prelude::*, px,
};

use super::attachments::{AttachmentView, attachment_strip};
use crate::components::Column;

pub(crate) type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The user's message, right-aligned in a bubble.
#[derive(IntoElement)]
pub struct UserMessage {
    id: SharedString,
    text: SharedString,
    on_edit: Option<Handler>,
    on_copy: Option<Handler>,
    on_save_note: Option<Handler>,
    copied: bool,
    time: Option<String>,
    attachments: Vec<AttachmentView>,
    on_open: Option<Rc<dyn Fn(&Path, &mut Window, &mut App)>>,
}

impl UserMessage {
    pub fn new(id: impl Into<SharedString>, text: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            on_edit: None,
            on_copy: None,
            on_save_note: None,
            copied: false,
            time: None,
            attachments: Vec::new(),
            on_open: None,
        }
    }

    /// Thumbnail and chip row above the bubble; clicking one calls `on_open`.
    pub fn attachments(
        mut self,
        items: Vec<AttachmentView>,
        on_open: Rc<dyn Fn(&Path, &mut Window, &mut App)>,
    ) -> Self {
        self.attachments = items;
        self.on_open = Some(on_open);
        self
    }

    /// Shows an Edit button under the bubble.
    pub fn on_edit(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_edit = Some(Rc::new(handler));
        self
    }

    /// Shows a Copy button beside Edit.
    pub fn on_copy(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_copy = Some(Rc::new(handler));
        self
    }

    /// Adds Save to note to the context menu.
    pub fn on_save_note(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_save_note = Some(Rc::new(handler));
        self
    }

    /// Swaps the Copy button for a Copied tick.
    pub fn copied(mut self, copied: bool) -> Self {
        self.copied = copied;
        self
    }

    /// Send time shown after the buttons.
    pub fn time(mut self, time: Option<String>) -> Self {
        self.time = time;
        self
    }
}

impl RenderOnce for UserMessage {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (edit, copy, save_note) =
            (self.on_edit.clone(), self.on_copy.clone(), self.on_save_note);
        let has_footer = edit.is_some() || copy.is_some() || self.time.is_some();
        let time = self
            .time
            .map(|time| div().text_xs().text_color(cx.theme().muted_foreground).child(time));
        let actions = actions_row(&self.id, window, cx)
            .when_some(self.on_edit, |row, handler| {
                row.child(
                    crate::components::button(SharedString::from(format!("{}-edit", self.id)))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Pencil)
                        .accessibility_label("Edit message")
                        .tooltip("Edit message")
                        .on_click(move |ev, window, cx| handler(ev, window, cx)),
                )
            })
            .when_some(self.on_copy, |row, handler| {
                row.child(copy_button(&self.id, self.copied, handler, cx))
            });
        let strip = (!self.attachments.is_empty()).then(|| {
            attachment_strip(
                self.attachments.clone(),
                px(48.),
                self.on_open.clone(),
                None,
                false,
                cx,
            )
        });
        let long = self.text.lines().count() > 8;
        let expanded_state = window.use_keyed_state(
            SharedString::from(format!("{}-expanded", self.id)),
            cx,
            |_, _| false,
        );
        let expanded = *expanded_state.read(cx);
        let text = (!self.text.is_empty()).then(|| {
            div()
                .relative()
                .when(long && !expanded, |d| d.max_h(px(182.)).overflow_hidden())
                .child(SelectableText::new(
                    SharedString::from(format!("{}-text", self.id)),
                    self.text.clone(),
                ))
                .when(long && !expanded, |d| {
                    let color = cx.theme().tokens.muted;
                    d.child(div().absolute().left_0().right_0().bottom_0().h(px(36.)).bg(
                        linear_gradient(
                            180.,
                            linear_color_stop(color.opacity(0.), 0.),
                            linear_color_stop(color, 1.),
                        ),
                    ))
                })
        });
        let toggle = long.then(|| {
            h_flex().justify_start().child(
                crate::components::button(SharedString::from(format!("{}-toggle", self.id)))
                    .ghost()
                    .xsmall()
                    .px_0()
                    .icon(if expanded { Lucide::ChevronUp } else { Lucide::ChevronDown })
                    .label(if expanded { "Show less" } else { "Show more" })
                    .on_click(move |_, _, cx| expanded_state.update(cx, |open, _| *open = !*open)),
            )
        });
        let bubble = Bubble::new()
            .alignment(MessageAlignment::End)
            .with_variant(BubbleVariant::Secondary)
            .child(v_flex().gap_1().children(strip).children(text).children(toggle));
        Column::new()
            .px_0()
            .group("message")
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(bubble)
            .when(has_footer, |row| {
                row.child(
                    h_flex().justify_end().gap_2().items_center().child(actions).children(time),
                )
            })
            .context_menu(move |menu, _, _| {
                let menu = menu_item(menu, "Copy", IconName::Copy, copy.as_ref());
                let menu = menu_item(menu, "Save to note", Lucide::FileText, save_note.as_ref());
                menu_item(menu, "Edit", Lucide::Pencil, edit.as_ref())
            })
    }
}

/// Hover actions that also show while keyboard focus is inside them; opacity,
/// not `invisible`, since hidden elements drop out of the tab order.
pub(crate) fn actions_row(id: &SharedString, window: &mut Window, cx: &mut App) -> Div {
    let focus = window
        .use_keyed_state(SharedString::from(format!("{id}-actions")), cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone();
    let keyboard = focus.contains_focused(window, cx) && window.last_input_was_keyboard();
    h_flex()
        .track_focus(&focus)
        // a click between buttons would focus the row and blur the composer
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .gap_2()
        .items_center()
        .when(!keyboard, |row| row.opacity(0.).group_hover("message", |s| s.opacity(1.)))
}

pub(crate) fn copy_button(id: &SharedString, copied: bool, handler: Handler, cx: &App) -> Button {
    let (icon, label) =
        if copied { (IconName::Check, "Copied") } else { (IconName::Copy, "Copy message") };
    crate::components::button(SharedString::from(format!("{id}-copy")))
        .ghost()
        .xsmall()
        .icon(icon)
        .accessibility_label(label)
        .tooltip(label)
        .when(copied, |b| b.text_color(crate::theme::success_text(cx)))
        .on_click(move |ev, window, cx| handler(ev, window, cx))
}

pub(crate) fn menu_item(
    menu: PopupMenu,
    label: &'static str,
    icon: impl Into<Icon>,
    handler: Option<&Handler>,
) -> PopupMenu {
    let Some(handler) = handler.cloned() else {
        return menu;
    };
    menu.item(
        PopupMenuItem::new(label)
            .icon(icon)
            .on_click(move |ev, window, cx| handler(ev, window, cx)),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/chat/message.rs"]
mod tests;
