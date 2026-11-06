use crate::theme::Corners;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, button::ButtonVariants, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ObjectFit, Pixels, Role, SharedString, StyledImage, Window, div, img,
    prelude::*, px,
};

use crate::components::overlay;

#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentView {
    pub name: SharedString,
    pub path: PathBuf,
    pub image: bool,
    pub size: u64,
    pub missing: bool,
}

pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.;
    match bytes as f64 {
        b if b < KB => format!("{bytes} B"),
        b if b < KB * KB => format!("{:.1} KB", b / KB),
        b => format!("{:.1} MB", b / (KB * KB)),
    }
}

pub fn attachment_caption(view: &AttachmentView) -> String {
    if view.missing { "File missing".to_string() } else { format_size(view.size) }
}

type OpenHandler = Rc<dyn Fn(&Path, &mut Window, &mut App)>;
type RemoveHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

fn remove_button(
    ix: usize,
    name: &SharedString,
    on_remove: &RemoveHandler,
    cx: &App,
) -> impl IntoElement + use<> {
    let on_remove = on_remove.clone();
    let label = format!("Remove {name}");
    crate::components::button(SharedString::from(format!("attachment-remove-{ix}")))
        .ghost()
        .xsmall()
        .icon(IconName::X)
        .rounded_full()
        .bg(cx.theme().background.opacity(0.85))
        .accessibility_label(label.clone())
        .tooltip(label)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_remove(ix, window, cx);
        })
}

/// Row of image thumbnails and file chips, `tile` px tall. Pending rows pass a
/// remove handler; the transcript passes an open handler.
pub fn attachment_strip(
    items: Vec<AttachmentView>,
    tile: Pixels,
    on_open: Option<OpenHandler>,
    on_remove: Option<RemoveHandler>,
    end: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let tint = overlay(cx);
    h_flex()
        .w_full()
        .flex_wrap()
        .when(end, |row| row.justify_end())
        .gap(px(8.))
        .children(items.into_iter().enumerate().map(|(ix, view)| {
            let open = on_open.clone().filter(|_| !view.missing);
            let path = view.path.clone();
            let open_label = SharedString::from(format!("Open {}", view.name));
            let caption = attachment_caption(&view);
            let fg = if view.missing { theme.muted_foreground } else { theme.foreground };
            let base = div()
                .id(("attachment", ix))
                .relative()
                .flex_none()
                .h(tile)
                .corner(px(10.))
                .overflow_hidden()
                .when(open.is_some(), |d| d.cursor_pointer())
                .when_some(open, move |d, open| {
                    d.role(Role::Button)
                        .tab_index(0)
                        .aria_label(open_label)
                        .on_click(move |_, window, cx| open(&path, window, cx))
                });
            let body = if view.image && !view.missing {
                base.w(tile).child(img(view.path.clone()).size_full().object_fit(ObjectFit::Cover))
            } else {
                let icon =
                    if view.image && view.missing { IconName::Image } else { IconName::FileText };
                base.max_w(px(220.))
                    .px(px(10.))
                    .bg(tint)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Icon::new(icon).small().text_color(theme.muted_foreground))
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(fg)
                                    .child(view.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child(caption),
                            ),
                    )
                    .children(on_remove.as_ref().map(|r| remove_button(ix, &view.name, r, cx)))
            };
            if view.image && !view.missing {
                body.children(on_remove.as_ref().map(|r| {
                    div()
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .child(remove_button(ix, &view.name, r, cx))
                }))
            } else {
                body
            }
        }))
        .into_any_element()
}

#[cfg(test)]
#[path = "../../tests/unit/chat/attachments.rs"]
mod tests;
