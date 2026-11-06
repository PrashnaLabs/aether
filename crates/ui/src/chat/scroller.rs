use std::{cell::Cell, ops::Range, rc::Rc, time::Duration};

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, StyledExt as _, button::Button,
};
use gpui_kit::component::{
    button::ButtonVariants as _,
    scroll::{ScrollableElement as _, ScrollableMask},
};
use gpui_kit::{
    AnyElement, App, Axis, Context, ElementId, Entity, FollowMode, Hsla, InteractiveElement as _,
    IntoElement, ListAlignment, ListOffset, ListState, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    linear_color_stop, linear_gradient, list, prelude::FluentBuilder as _, px, rems,
};

// Vendored from gpui-component's MessageScroller: identical except `reset`
// seeds every row with a height hint (on a fresh list when leaving measure-all
// mode), and `splice` remeasures the row after it. Without hints fresh rows estimate 0px,
// so wheel seeks land inside them at offset 0 and the viewport sticks to
// tall rows' tops once they measure. Revisit on component upgrades.
// (Upstream tests dropped: they need gpui's test harness, which we don't
// depend on.)

const LIST_OVERDRAW: gpui_kit::Pixels = px(400.);
const JUMP_BUTTON_TRANSITION: Duration = Duration::from_millis(200);
const BOTTOM_FADE_TRANSITION: Duration = Duration::from_millis(200);
/// Typical transcript row, one block plus its top gap: step rows are ~28 px,
/// text and cards 100-300 px. Only a pre-measure hint; real heights replace it on render.
const ESTIMATED_ROW_HEIGHT: gpui_kit::Pixels = px(48.);
/// Measure every row up front below this size so seeks and anchors are exact
/// from the first frame. Above it the hints above are all we get; measuring
/// hundreds of markdown rows would stall opening the chat.
const MEASURE_ALL_ROWS: usize = 100;

/// Leaving measure-all mode takes a fresh list; every other reset keeps the list.
fn needs_fresh_list(measuring_all: bool, item_count: usize) -> bool {
    measuring_all && item_count > MEASURE_ALL_ROWS
}

/// A wheel flag only survives while the list stays scrolled up.
fn wheeled_up(wheeled: bool, scrolled_up: bool) -> bool {
    wheeled && scrolled_up
}

/// The entity-owned scrolling state for a [`ChatScroller`].
///
/// The state owns only GPUI's virtual-list bookkeeping. Message data remains
/// with the caller and is read by the row renderer passed to
/// [`ChatScroller::new`].
pub struct ChatScrollerState {
    list_state: ListState,
    wheeled: Rc<Cell<bool>>,
    measuring_all: bool,
    /// A fresh list's first paint drops its row hints; set them again after it.
    rehint: Cell<bool>,
}

impl ChatScrollerState {
    /// Create a state for `item_count` rows and enable tail following.
    ///
    /// The constructor receives the entity context so the list's scroll
    /// handler can safely defer its entity update until GPUI has released the
    /// list's internal borrow.
    pub fn new(item_count: usize, cx: &mut Context<Self>) -> Self {
        let wheeled = Rc::new(Cell::new(false));
        let list_state = Self::list_state(&wheeled, cx);
        let mut this = Self { list_state, wheeled, measuring_all: false, rehint: Cell::new(false) };
        this.fill(item_count);
        this
    }

    fn list_state(wheeled: &Rc<Cell<bool>>, cx: &mut Context<Self>) -> ListState {
        let list_state = ListState::new(0, ListAlignment::Top, LIST_OVERDRAW);
        let weak_state = cx.weak_entity();
        let flag = wheeled.clone();
        list_state.set_scroll_handler(move |_, _, cx| {
            flag.set(true);
            let weak_state = weak_state.clone();

            cx.defer(move |cx| {
                let _ = weak_state.update(cx, |_, cx| cx.notify());
            });
        });
        list_state
    }

    /// Portable scroll state for moving a session between native windows.
    pub fn snapshot(&self) -> (usize, f32, bool) {
        let offset = self.list_state.logical_scroll_top();
        (offset.item_ix, f32::from(offset.offset_in_item), self.list_state.is_following_tail())
    }

    pub fn restore_snapshot(&mut self, snapshot: (usize, f32, bool), cx: &mut Context<Self>) {
        if snapshot.2 {
            self.scroll_to_end(cx);
        } else if self.item_count() > 0 {
            self.list_state.scroll_to(ListOffset {
                item_ix: snapshot.0.min(self.item_count() - 1),
                offset_in_item: px(if snapshot.1.is_finite() { snapshot.1.max(0.) } else { 0. }),
            });
            cx.notify();
        }
    }

    /// Return the current number of rows known by the virtual list.
    pub fn item_count(&self) -> usize {
        self.list_state.item_count()
    }

    /// Return whether the user has scrolled away from the latest content.
    pub fn is_scrolled_up(&self) -> bool {
        self.list_state.max_offset_for_scrollbar().y > px(0.)
            && !self.list_state.is_following_tail()
            && !self.list_state.is_scrolled_to_end().unwrap_or(false)
    }

    /// Return whether the user wheeled away from the latest content. The
    /// list's scroll handler only fires for wheel events, so programmatic
    /// scrolls never set it.
    pub fn user_scrolled_up(&self) -> bool {
        let up = wheeled_up(self.wheeled.get(), self.is_scrolled_up());
        self.wheeled.set(up);
        up
    }

    /// Return whether the list is actively following its tail.
    pub fn is_following_tail(&self) -> bool {
        self.list_state.is_following_tail()
    }

    /// Reset the list to `item_count` rows. Every row starts with a height
    /// hint so seeks and the scrollbar are sane before rows measure. Small
    /// chats measure every row up front so anchors never collapse onto tall
    /// rows; gpui can't leave that mode, so a large chat gets a fresh list
    /// that measures only what it shows.
    pub fn reset(&mut self, item_count: usize, cx: &mut Context<Self>) {
        if needs_fresh_list(self.measuring_all, item_count) {
            self.list_state = Self::list_state(&self.wheeled, cx);
            self.rehint.set(true);
        }
        self.fill(item_count);
        self.wheeled.set(false);
        cx.notify();
    }

    fn fill(&mut self, item_count: usize) {
        self.list_state.reset_with_uniform_height(item_count, ESTIMATED_ROW_HEIGHT);
        self.measuring_all = item_count <= MEASURE_ALL_ROWS;
        if self.measuring_all {
            self.list_state = self.list_state.clone().measure_all();
        }
        self.list_state.set_follow_mode(FollowMode::Tail);
    }

    /// Replace `old_range` with `count` new rows.
    ///
    /// Returns `false` when the range is outside the current list and leaves
    /// the state unchanged.
    pub fn splice(
        &mut self,
        old_range: Range<usize>,
        count: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.valid_range(&old_range) {
            return false;
        }

        let after = old_range.start + count;
        self.list_state.splice(old_range, count);

        // the survivor after the splice has a new row before it, so its top gap may change
        if after < self.list_state.item_count() {
            self.list_state.remeasure_items(after..after + 1);
        }

        cx.notify();
        true
    }

    /// Append `count` rows to the end of the list.
    pub fn append(&mut self, count: usize, cx: &mut Context<Self>) -> bool {
        let item_count = self.list_state.item_count();
        self.splice(item_count..item_count, count, cx)
    }

    /// Prepend `count` rows while preserving the current scroll anchor.
    pub fn prepend(&mut self, count: usize, cx: &mut Context<Self>) -> bool {
        self.splice(0..0, count, cx)
    }

    /// Mark all rows for remeasurement while preserving a proportional anchor.
    pub fn remeasure(&mut self, cx: &mut Context<Self>) {
        self.list_state.remeasure();
        cx.notify();
    }

    /// Mark rows in `range` for remeasurement while preserving an item anchor.
    ///
    /// Returns `false` when the range is outside the current list.
    pub fn remeasure_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> bool {
        if !self.valid_range(&range) {
            return false;
        }

        self.list_state.remeasure_items(range);
        cx.notify();
        true
    }

    /// Scroll to the row at `index`, if it exists.
    pub fn scroll_to_item(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        if index >= self.list_state.item_count() {
            return false;
        }

        self.list_state.scroll_to(ListOffset { item_ix: index, offset_in_item: px(0.) });
        cx.notify();
        true
    }

    /// Resume tail following and scroll to the latest row.
    pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.list_state.set_follow_mode(FollowMode::Tail);
        self.list_state.scroll_to_end();
        self.wheeled.set(false);
        cx.notify();
    }

    fn valid_range(&self, range: &Range<usize>) -> bool {
        range.start <= range.end && range.end <= self.list_state.item_count()
    }
}

/// A virtualized message list with optional scrollbar and jump-to-latest UI.
#[derive(IntoElement)]
pub struct ChatScroller {
    id: ElementId,
    state: Entity<ChatScrollerState>,
    renderer: Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static>,
    style: StyleRefinement,
    content_style: StyleRefinement,
    list_style: StyleRefinement,
    row_style: StyleRefinement,
    jump_button_style: StyleRefinement,
    jump_button_renderer: Option<Box<dyn FnOnce(Button) -> Button>>,
    jump_button_transition: Duration,
    bottom_fade: Option<Hsla>,
    scrollbar: bool,
    jump_button: bool,
    jump_button_label: SharedString,
}

impl ChatScroller {
    /// Create a message scroller with a renderer for each row.
    pub fn new<E>(
        id: impl Into<ElementId>,
        state: Entity<ChatScrollerState>,
        renderer: impl FnMut(usize, &mut Window, &mut App) -> E + 'static,
    ) -> Self
    where
        E: IntoElement,
    {
        let mut renderer = renderer;
        Self {
            id: id.into(),
            state,
            renderer: Box::new(move |index, window, cx| {
                renderer(index, window, cx).into_any_element()
            }),
            style: StyleRefinement::default(),
            content_style: StyleRefinement::default(),
            list_style: StyleRefinement::default(),
            row_style: StyleRefinement::default(),
            jump_button_style: StyleRefinement::default(),
            jump_button_renderer: None,
            jump_button_transition: JUMP_BUTTON_TRANSITION,
            bottom_fade: None,
            scrollbar: true,
            jump_button: true,
            jump_button_label: "Jump to latest".into(),
        }
    }

    /// Enable or disable the virtual-list scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Enable or disable the built-in jump-to-latest button.
    pub fn jump_button(mut self, jump_button: bool) -> Self {
        self.jump_button = jump_button;
        self
    }

    /// Set the label used by the built-in jump-to-latest button.
    pub fn with_jump_button_label(mut self, label: impl Into<SharedString>) -> Self {
        self.jump_button_label = label.into();
        self
    }

    /// Refine the viewport that contains the list and scrollbar.
    pub fn with_content_style(mut self, style: StyleRefinement) -> Self {
        self.content_style = style;
        self
    }

    /// Refine the GPUI list element used to render rows.
    pub fn with_list_style(mut self, style: StyleRefinement) -> Self {
        self.list_style = style;
        self
    }

    /// Refine the full-width wrapper around every rendered row.
    pub fn with_row_style(mut self, style: StyleRefinement) -> Self {
        self.row_style = style;
        self
    }

    /// Refine the built-in jump-to-latest button after its defaults.
    pub fn with_jump_button_style(mut self, style: StyleRefinement) -> Self {
        self.jump_button_style = style;
        self
    }

    /// Customize the built-in jump button without replacing its scroll action.
    ///
    /// The callback receives the fully configured Button, so its variant,
    /// semantic size, icon, tooltip, or instance styling may be adjusted.
    pub fn with_jump_button_renderer(
        mut self,
        renderer: impl FnOnce(Button) -> Button + 'static,
    ) -> Self {
        self.jump_button_renderer = Some(Box::new(renderer));
        self
    }

    /// Set how long the built-in jump button takes to enter or leave.
    ///
    /// A zero duration disables its transition. Reduced-motion preferences
    /// always adopt the final state immediately.
    pub fn with_jump_button_transition(mut self, duration: Duration) -> Self {
        self.jump_button_transition = duration;
        self
    }

    /// Fade the transcript's bottom edge into `color`.
    ///
    /// A partially visible row melts into the surface behind the scroller
    /// instead of clipping mid-line. The fade shows only while the reader is
    /// away from the live edge — at the bottom nothing is clipped. Pass the
    /// color of that surface; the fade is off by default and sits under the
    /// jump button.
    pub fn with_bottom_fade(mut self, color: impl Into<Hsla>) -> Self {
        self.bottom_fade = opaque_fade(color.into());
        self
    }
}

// on a translucent surface the overlay stacks alpha and shows as a darker band
fn opaque_fade(color: Hsla) -> Option<Hsla> {
    (color.a >= 1.).then_some(color)
}

impl Styled for ChatScroller {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ChatScroller {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let root_id = self.id.clone();
        let (list_state, scrolled_up, rehint) = {
            let state = self.state.read(cx);
            (state.list_state.clone(), state.is_scrolled_up(), state.rehint.replace(false))
        };
        if rehint {
            let list_state = list_state.clone();
            window.on_next_frame(move |window, _| {
                list_state.with_uniform_item_height(ESTIMATED_ROW_HEIGHT);
                window.refresh();
            });
        }
        let show_jump_button = self.jump_button && scrolled_up;
        let jump_button_visibility = if self.jump_button {
            transition(
                (root_id.clone(), "jump-button-visibility"),
                if show_jump_button { 1. } else { 0. },
                Transition::new(self.jump_button_transition),
                window,
                cx,
            )
        } else {
            0.
        };
        // At the live edge nothing is clipped below, so a visible fade would
        // suggest more content than there is.
        let bottom_fade_visibility = if self.bottom_fade.is_some() {
            transition(
                (root_id.clone(), "bottom-fade-visibility"),
                if scrolled_up { 1. } else { 0. },
                Transition::new(BOTTOM_FADE_TRANSITION),
                window,
                cx,
            )
        } else {
            0.
        };
        let tokens = cx.theme().semantic_tokens();
        let row_style = self.row_style;
        let jump_button_style = self.jump_button_style;
        let jump_button_renderer = self.jump_button_renderer;
        let mut renderer = self.renderer;

        // GPUI's `list` lays rows out at the full list width and offsets them
        // only by vertical padding, so the horizontal component of the list
        // style must be carried by every row wrapper instead.
        let mut list_style = self.list_style;
        let row_inset_left = list_style.padding.left.take();
        let row_inset_right = list_style.padding.right.take();

        let list = list(list_state.clone(), move |index, window, cx| {
            div()
                .w_full()
                .min_w_0()
                .px_3()
                .when_some(row_inset_left, |this, left| this.pl(left))
                .when_some(row_inset_right, |this, right| this.pr(right))
                .refine_style(&row_style)
                .child(renderer(index, window, cx))
                .into_any_element()
        })
        .size_full()
        .min_h_0()
        .py_2()
        .refine_style(&list_style);

        let viewport = div()
            .id((root_id.clone(), "viewport"))
            // Announce appended rows as a log region, like shadcn's
            // `role="log"` transcript content.
            .role(Role::Log)
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(list)
            // The fade sits above the rows but below the scrollbar and the
            // jump button, so neither control is washed out by it.
            .when_some(self.bottom_fade.filter(|_| bottom_fade_visibility > 0.), |this, color| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(rems(3.))
                        .opacity(bottom_fade_visibility)
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(color.opacity(0.), 0.),
                            linear_color_stop(color, 1.),
                        )),
                )
            })
            .when(self.scrollbar, |this| this.vertical_scrollbar(&list_state))
            .refine_style(&self.content_style);

        div()
            .id(root_id.clone())
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .child(viewport)
            // Keep vertical wheel scrolling from leaking into an ancestor
            // scroller (like in Table): the mask consumes vertical-dominant
            // wheel events while the list can move and chains to the ancestor
            // only at the edges.
            .child(ScrollableMask::new(Axis::Vertical, &list_state).id(root_id.clone()))
            .when(self.jump_button && jump_button_visibility > 0., |this| {
                let state = self.state.clone();

                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(rems(0.5 + jump_button_visibility * 0.5))
                        .flex()
                        .justify_center()
                        .opacity(jump_button_visibility)
                        .child(
                            // No explicit width or height: Button sizes an
                            // icon-only button as a square on its own, and a
                            // renderer that adds a label or another semantic
                            // size must be able to change the layout.
                            crate::components::button((root_id, "jump-to-latest"))
                                .secondary()
                                .icon(IconName::ArrowDown)
                                .tooltip(self.jump_button_label)
                                .rounded(cx.theme().radius_full())
                                .border_1()
                                .border_color(tokens.colors.border)
                                .bg(tokens.colors.background)
                                .text_color(tokens.colors.foreground)
                                .refine_style(&jump_button_style)
                                .on_click(move |_, _, cx| {
                                    state.update(cx, |state, cx| state.scroll_to_end(cx));
                                })
                                .when_some(jump_button_renderer, |button, renderer| {
                                    renderer(button)
                                })
                                .when(!show_jump_button, |button| button.disabled(true)),
                        ),
                )
            })
            .refine_style(&self.style)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/scroller.rs"]
mod tests;
