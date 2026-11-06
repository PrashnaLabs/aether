use crate::theme::Corners;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Selectable, Sizable,
    button::{ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    scroll::ScrollableElement,
    v_flex,
};
use gpui_kit::rems;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Role, ScrollStrategy,
    Subscription, UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};

use super::{TriggerBounds, anchored_popup, field_fill, overlay_strong, popup_height, room};

actions!(branch_select, [SelectPrev, SelectNext, SelectConfirm, SelectDismiss]);

pub const BRANCH_SELECT_CONTEXT: &str = "BranchSelect";

pub enum BranchSelectEvent {
    Pick(String),
}

pub struct BranchSelectState {
    branches: Vec<String>,
    query: Entity<InputState>,
    highlight: Option<usize>,
    filtered: Vec<usize>,
    open: bool,
    trigger: TriggerBounds,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    restore_focus: Option<FocusHandle>,
    _query_changes: Subscription,
}

impl EventEmitter<BranchSelectEvent> for BranchSelectState {}

impl BranchSelectState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search branches"));
        let _query_changes = cx.subscribe(&query, |this, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                this.highlight = None;
                this.refresh(cx);
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
        });
        Self {
            branches: Vec::new(),
            query,
            highlight: None,
            filtered: Vec::new(),
            open: false,
            trigger: TriggerBounds::default(),
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            restore_focus: None,
            _query_changes,
        }
    }

    pub fn sync(&mut self, branches: Vec<String>, selected: Option<&str>, cx: &mut Context<Self>) {
        self.branches = branches;
        self.highlight = selected
            .and_then(|s| self.branches.iter().position(|b| b == s))
            .or(if self.branches.is_empty() { None } else { Some(0) });
        self.refresh(cx);
    }

    pub fn selected(&self) -> Option<&str> {
        self.highlight
            .and_then(|ix| self.filtered.get(ix))
            .and_then(|&ix| self.branches.get(ix))
            .map(|s| s.as_str())
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let q = self.query.read(cx).value().to_lowercase();
        self.filtered = if q.is_empty() {
            (0..self.branches.len()).collect()
        } else {
            self.branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.to_lowercase().contains(&q))
                .map(|(ix, _)| ix)
                .collect()
        };
        if self.filtered.is_empty() {
            self.highlight = None;
        } else if let Some(ix) = self.highlight
            && !self.filtered.contains(&ix)
        {
            self.highlight = Some(0);
        }
        cx.notify();
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_focus = window.focused(cx);
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        self.refresh(cx);
        self.highlight = self.highlight.filter(|&ix| self.filtered.contains(&ix));
        if self.highlight.is_none() && !self.filtered.is_empty() {
            self.highlight = Some(0);
        }
        if let Some(ix) = self.highlight {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        self.open = true;
        self.query.update(cx, |q, cx| q.focus(window, cx));
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        if let Some(previous) = self.restore_focus.take()
            && (self.focus.contains_focused(window, cx)
                || self.query.focus_handle(cx).is_focused(window))
        {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.restore_focus = None;
        self.highlight = None;
        cx.notify();
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.filtered.len();
        self.highlight = match self.highlight {
            Some(ix) if ix < len => Some((ix as isize + delta).rem_euclid(len as isize) as usize),
            _ if delta < 0 => Some(len - 1),
            _ => Some(0),
        };
        if let Some(ix) = self.highlight {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.highlight
            && let Some(&branch_ix) = self.filtered.get(ix)
        {
            let branch = self.branches[branch_ix].clone();
            self.close(window, cx);
            cx.emit(BranchSelectEvent::Pick(branch));
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let branch_ix = self.filtered[ix];
        let branch = &self.branches[branch_ix];
        let highlighted = self.highlight == Some(ix);
        let selected = self.highlight == Some(ix) && !self.open;
        h_flex()
            .id(("branch-row", ix))
            .w_full()
            .h(px(32.))
            .px_2()
            .gap_2()
            .corner(rems(0.375))
            .cursor_pointer()
            .role(Role::ListBoxOption)
            .aria_label(branch.clone())
            .aria_selected(selected)
            .when(highlighted, |r| r.bg(theme.accent))
            .when(selected, |r| r.bg(theme.secondary))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && this.highlight != Some(ix) {
                    this.highlight = Some(ix);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.highlight = Some(ix);
                this.confirm(window, cx);
            }))
            .child(div().text_sm().min_w_0().truncate().child(branch.clone()))
            .when(selected, |r| {
                r.child(Icon::new(IconName::Check).small().flex_none().text_color(theme.primary))
            })
    }

    fn render_panel(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let height = popup_height(room(self.trigger.get(), false, window), 320.);
        let width = self.trigger.get().size.width;
        let panel = div()
            .id("branch-select-panel")
            .key_context(BRANCH_SELECT_CONTEXT)
            .role(Role::Dialog)
            .aria_label("Choose base branch")
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_highlight(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_highlight(1, cx)))
            .on_action(cx.listener(|this, _: &SelectConfirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &SelectDismiss, window, cx| this.close(window, cx)))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .track_focus(&self.focus)
            .overflow_hidden()
            .corner(rems(0.5))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_lg()
            .w(width)
            .h(px(height))
            .flex()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div().px_2().py_1().border_b_1().border_color(theme.border).child(
                            super::field(&self.query)
                                .small()
                                .icon(IconName::Search)
                                .aria_label("Search branches"),
                        ),
                    )
                    .child(
                        div()
                            .id("branch-select-list")
                            .role(Role::ListBox)
                            .aria_label("Branches")
                            .w_full()
                            .flex_1()
                            .min_h_0()
                            .px_2()
                            .py_1()
                            .child(if self.filtered.is_empty() {
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .p_4()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("No branches match your search")
                                    .into_any_element()
                            } else {
                                uniform_list(
                                    "branch-rows",
                                    self.filtered.len(),
                                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                        range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                                    }),
                                )
                                .track_scroll(&self.scroll)
                                .size_full()
                                .into_any_element()
                            })
                            .vertical_scrollbar(&self.scroll),
                    ),
            );
        let _ = window;
        panel
    }
}

impl Render for BranchSelectState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_panel(window, cx)
    }
}

#[derive(IntoElement)]
pub struct BranchSelect {
    state: Entity<BranchSelectState>,
}

impl BranchSelect {
    pub fn new(state: &Entity<BranchSelectState>) -> Self {
        Self { state: state.clone() }
    }
}

impl RenderOnce for BranchSelect {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let (open, bounds) = (state.open, state.trigger.clone());
        let selected = state.selected().map(|s| s.to_string()).unwrap_or_default();
        let entity = self.state.clone();
        let trigger = {
            let fill = field_fill(cx);
            let hover = overlay_strong(cx);
            crate::components::button("branch-select")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(fill)
                        .foreground(cx.theme().foreground)
                        .hover(hover)
                        .active(hover),
                )
                .bg(fill)
                .cursor_pointer()
                .w_full()
                .h(px(40.))
                .px(px(12.))
                .corner(px(10.))
                .child(h_flex().min_w_0().gap_2().child(div().min_w_0().truncate().child(selected)))
                .dropdown_caret(true)
        }
        .accessibility_label("Base branch")
        .selected(open)
        .when(!open, |b| {
            b.on_click(move |_, window, cx| {
                entity.update(cx, |s, cx| s.open(window, cx));
            })
        });
        anchored_popup(("branch-select-popup", self.state.entity_id()), trigger, &bounds, false)
            .when(open, |popup| popup.content(self.state.clone()))
    }
}
