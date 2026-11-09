use crate::theme::Corners;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, StyledExt, h_flex, input::InputState, menu::ContextMenuExt,
    menu::PopupMenu, tooltip::Tooltip, v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, FocusHandle, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, Pixels, Role, ScrollHandle, SharedString, Window, div, prelude::*,
    px, rems,
};

use super::field;

type TabClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type TabDoubleClickHandler = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type TabMenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;
type CancelRenameHandler = Rc<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;

const TAB_GROUP: &str = "aether-tab";

struct TabClose {
    id: ElementId,
    tooltip: SharedString,
    on_click: TabClickHandler,
}

/// Shared, reusable tab pill used across Terminal groups, Explorer files, and Side Panel tabs.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    icon_color: Option<Hsla>,
    selected: bool,
    badge_id: Option<ElementId>,
    badge: Option<SharedString>,
    badge_tooltip: Option<SharedString>,
    dirty: bool,
    tooltip: Option<SharedString>,
    max_label_width: Pixels,
    on_click: Option<TabClickHandler>,
    on_double_click: Option<TabDoubleClickHandler>,
    close: Option<TabClose>,
    context_menu: Option<TabMenuBuilder>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            icon_color: None,
            selected: false,
            badge_id: None,
            badge: None,
            badge_tooltip: None,
            dirty: false,
            tooltip: None,
            max_label_width: px(160.),
            on_click: None,
            on_double_click: None,
            close: None,
            context_menu: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn badge_with_id(
        mut self,
        id: impl Into<ElementId>,
        badge: impl Into<SharedString>,
        tooltip: Option<SharedString>,
    ) -> Self {
        self.badge_id = Some(id.into());
        self.badge = Some(badge.into());
        self.badge_tooltip = tooltip;
        self
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    /// Overrides the default tooltip, which is the full label when it may truncate.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn max_label_width(mut self, width: Pixels) -> Self {
        self.max_label_width = width;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn on_double_click(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_double_click = Some(Rc::new(handler));
        self
    }

    pub fn on_close(
        mut self,
        id: impl Into<ElementId>,
        tooltip: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.close =
            Some(TabClose { id: id.into(), tooltip: tooltip.into(), on_click: Rc::new(handler) });
        self
    }

    pub fn context_menu(
        mut self,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.context_menu = Some(Rc::new(builder));
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus: FocusHandle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_index(0).tab_stop(true)
            })
            .read(cx)
            .clone();
        let keyboard_focused = focus.is_focused(window) && window.last_input_was_keyboard();
        // ponytail: ~0.42rem per char guess; measure with text_system if tips misfire
        let may_truncate = self.label.chars().count() as f32 * f32::from(window.rem_size()) * 0.42
            > f32::from(self.max_label_width);
        let tooltip = self.tooltip.or_else(|| may_truncate.then(|| self.label.clone()));
        let theme = cx.theme();
        let ring = theme.ring;
        let fg = if self.selected { theme.foreground } else { theme.muted_foreground };
        let tab_bg = if self.selected {
            theme.foreground.opacity(0.10)
        } else {
            gpui_kit::transparent_black()
        };
        let has_trailing = self.close.is_some() || self.badge.is_some() || self.dirty;
        let icon_fg = self.icon_color.unwrap_or(fg);

        let mut el = h_flex()
            .id(self.id)
            .group(TAB_GROUP)
            .role(Role::Tab)
            .aria_label(self.label.clone())
            .aria_selected(self.selected)
            .when(self.dirty, |el| el.aria_description("Unsaved changes"))
            .track_focus(&focus)
            .flex_none()
            .h(px(26.))
            .pl(px(8.))
            .pr(if has_trailing { px(5.) } else { px(8.) })
            .gap_1p5()
            .items_center()
            .rounded(theme.radius)
            .bg(tab_bg)
            .border_1()
            .border_color(if self.selected { theme.border } else { gpui_kit::transparent_black() })
            .focus_visible(move |s| s.border_color(ring))
            .hover(|s| s.bg(theme.foreground.opacity(0.12)).text_color(theme.foreground))
            .cursor_pointer()
            .when_some(tooltip, |el, tip| {
                el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            });

        if let Some(double_click) = self.on_double_click {
            el = el.on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
                if e.click_count == 2 {
                    cx.stop_propagation();
                    double_click(e, window, cx);
                }
            });
        }

        if let Some(on_click) = self.on_click {
            el = el.on_click(move |ev, window, cx| on_click(ev, window, cx));
        }

        if let Some(close) = &self.close {
            let on_close = close.on_click.clone();
            el = el.on_aux_click(move |ev, window, cx| {
                if ev.is_middle_click() {
                    on_close(ev, window, cx);
                }
            });
        }

        if let Some(icon) = self.icon {
            el = el.child(icon.size(px(13.)).text_color(icon_fg).flex_none());
        }

        el = el.child(
            div()
                .min_w_0()
                .max_w(self.max_label_width)
                .truncate()
                .text_xs()
                .when(self.selected, |d| d.font_medium())
                .text_color(fg)
                .child(self.label),
        );

        if let Some(badge) = self.badge {
            let badge_id = self.badge_id.unwrap_or_else(|| ElementId::Name("tab-badge".into()));
            let badge_tip = self.badge_tooltip;
            el = el.child(
                div()
                    .id(badge_id)
                    .px_1()
                    .corner(rems(0.25))
                    .bg(theme.foreground.opacity(0.12))
                    .text_color(theme.muted_foreground)
                    .text_xs()
                    .when_some(badge_tip, |b, tip| {
                        b.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    })
                    .child(badge),
            );
        }

        let dot = || div().w(px(6.)).h(px(6.)).rounded_full().bg(theme.primary).flex_none();
        match self.close {
            Some(close) => {
                let tip = close.tooltip;
                let on_close = close.on_click;
                let button = div()
                    .id(close.id)
                    .w(px(20.))
                    .h(px(20.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .corner(rems(0.25))
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.foreground.opacity(0.14)).text_color(theme.foreground))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |ev, window, cx| {
                        cx.stop_propagation();
                        on_close(ev, window, cx);
                    })
                    .child(Icon::new(IconName::Close).size(px(11.)));
                el = if self.dirty && !keyboard_focused {
                    el.child(
                        div()
                            .relative()
                            .flex_none()
                            .child(button.invisible().group_hover(TAB_GROUP, |s| s.visible()))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .group_hover(TAB_GROUP, |s| s.invisible())
                                    .child(dot()),
                            ),
                    )
                } else {
                    el.child(button)
                };
            }
            None if self.dirty => el = el.child(dot()),
            None => {}
        }

        if let Some(menu_builder) = self.context_menu {
            el.context_menu(move |menu, window, cx| menu_builder(menu, window, cx))
                .into_any_element()
        } else {
            el.into_any_element()
        }
    }
}

/// Shared horizontal tab bar container used across Terminal, Explorer, and Side Panel.
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs_id: Option<ElementId>,
    height: Pixels,
    bg: Option<Hsla>,
    border_color: Option<Hsla>,
    border_top: bool,
    border_bottom: bool,
    leading: Option<AnyElement>,
    tabs: Vec<AnyElement>,
    trailing: Option<AnyElement>,
    info: Option<AnyElement>,
    actions: Vec<AnyElement>,
    selected: Option<usize>,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tabs_id: None,
            height: px(36.),
            bg: None,
            border_color: None,
            border_top: false,
            border_bottom: true,
            leading: None,
            tabs: Vec::new(),
            trailing: None,
            info: None,
            actions: Vec::new(),
            selected: None,
        }
    }

    pub fn tabs_id(mut self, id: impl Into<ElementId>) -> Self {
        self.tabs_id = Some(id.into());
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn bg(mut self, bg: Hsla) -> Self {
        self.bg = Some(bg);
        self
    }

    pub fn border_color(mut self, color: Hsla) -> Self {
        self.border_color = Some(color);
        self
    }

    pub fn border_top(mut self, border_top: bool) -> Self {
        self.border_top = border_top;
        self
    }

    pub fn border_bottom(mut self, border_bottom: bool) -> Self {
        self.border_bottom = border_bottom;
        self
    }

    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    pub fn tab(mut self, tab: impl IntoElement) -> Self {
        self.tabs.push(tab.into_any_element());
        self
    }

    pub fn tabs(mut self, tabs: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.tabs.extend(tabs.into_iter().map(IntoElement::into_any_element));
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }

    pub fn info(mut self, el: impl IntoElement) -> Self {
        self.info = Some(el.into_any_element());
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.actions.push(el.into_any_element());
        self
    }

    pub fn actions(mut self, els: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.actions.extend(els.into_iter().map(IntoElement::into_any_element));
        self
    }

    /// The selected tab's index; the strip scrolls it into view when it changes.
    pub fn selected_index(mut self, ix: usize) -> Self {
        self.selected = Some(ix);
        self
    }
}

impl RenderOnce for TabBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window
            .use_keyed_state(self.id.clone(), cx, |_, _| (ScrollHandle::new(), None::<usize>));
        let (scroll, shown) = state.read(cx).clone();
        if self.selected != shown {
            if let Some(ix) = self.selected {
                scroll.scroll_to_item(ix);
            }
            state.update(cx, |state, _| state.1 = self.selected);
        }
        let theme = cx.theme();
        let border = self.border_color.unwrap_or(theme.border);
        let tabs_id = self.tabs_id.unwrap_or_else(|| ElementId::Name("tab-bar-strip".into()));

        h_flex()
            .id(self.id)
            .flex_none()
            .w_full()
            .h(self.height)
            .pl(px(8.))
            .pr(px(8.))
            .gap_1p5()
            .items_center()
            .text_xs()
            .when_some(self.bg, |bar, bg| bar.bg(bg))
            .when(self.border_top, |bar| bar.border_t_1())
            .when(self.border_bottom, |bar| bar.border_b_1())
            .border_color(border)
            .children(self.leading)
            .child(
                h_flex()
                    .id(tabs_id)
                    .role(Role::TabList)
                    .min_w_0()
                    .gap_1()
                    .items_center()
                    .overflow_x_scroll()
                    .track_scroll(&scroll)
                    .children(self.tabs)
                    .children(self.trailing),
            )
            .children(self.info)
            .child(div().flex_1())
            .when(!self.actions.is_empty(), |bar| {
                bar.child(h_flex().flex_none().gap_1().items_center().children(self.actions))
            })
    }
}

/// Shared inline rename field overlaid on an invisible ghost element so the row/tab keeps its geometry.
#[derive(IntoElement)]
pub struct InlineRename {
    title: SharedString,
    editor: Entity<InputState>,
    is_tab: bool,
    has_subtitle: bool,
    on_cancel: Option<CancelRenameHandler>,
}

impl InlineRename {
    /// Full-width sidebar row rename (used by Chat sidebar and Terminal sidebar).
    pub fn row(title: impl Into<SharedString>, editor: &Entity<InputState>) -> Self {
        Self {
            title: title.into(),
            editor: editor.clone(),
            is_tab: false,
            has_subtitle: false,
            on_cancel: None,
        }
    }

    /// Compact inline tab pill rename (used by Terminal group tabs).
    pub fn tab(title: impl Into<SharedString>, editor: &Entity<InputState>) -> Self {
        Self {
            title: title.into(),
            editor: editor.clone(),
            is_tab: true,
            has_subtitle: false,
            on_cancel: None,
        }
    }

    /// Preserves the three-line height of agent chat rows.
    pub fn has_subtitle(mut self, has_subtitle: bool) -> Self {
        self.has_subtitle = has_subtitle;
        self
    }

    pub fn on_cancel(
        mut self,
        handler: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for InlineRename {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let ghost = if self.is_tab {
            h_flex()
                .invisible()
                .min_w(px(120.))
                .max_w(px(180.))
                .h(px(26.))
                .px_3()
                .text_xs()
                .child(self.title)
                .into_any_element()
        } else {
            v_flex()
                .invisible()
                .w_full()
                .border_1()
                .py_1p5()
                .gap_0p5()
                .text_sm()
                .when(self.has_subtitle, |el| el.child(div().text_xs().child("·")))
                .child(h_flex().min_h(rems(1.25)).child(self.title))
                .when(self.has_subtitle, |el| el.child(div().text_xs().child("·")))
                .into_any_element()
        };

        let mut wrapper = div()
            .relative()
            .when(!self.is_tab, |el| el.w_full())
            .when(self.is_tab, |el| el.flex_none())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        if let Some(on_cancel) = self.on_cancel {
            wrapper = wrapper.on_key_down(move |ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    on_cancel(ev, window, cx);
                    cx.stop_propagation();
                }
            });
        }

        wrapper.child(ghost).child(
            div().absolute().inset_0().flex().items_center().child(field(&self.editor).small()),
        )
    }
}

/// Room at the end of a window-top bar for the window controls floating over it.
pub fn window_controls_gap(window: &Window) -> gpui_kit::Div {
    let room = cfg!(any(target_os = "linux", target_os = "windows")) && !window.is_fullscreen();
    gpui_kit::div().flex_none().w(px(if room { 110. } else { 0. }))
}
