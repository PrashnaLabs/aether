use crate::theme::Corners;
use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Selectable, StyledExt, button::Button, h_flex, v_flex,
};
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Render, Role, ScrollHandle,
    SharedString, Window, actions, div, img, prelude::*, px,
};

use crate::project_mark_sized;

use super::{TriggerBounds, anchored_popup, popup_height, room};

actions!(
    choice_menu,
    [ChoiceUp, ChoiceDown, ChoiceFirst, ChoiceLast, ChoiceConfirm, ChoiceDismiss]
);

/// Key context on the open panel; the app binds the choice actions under it.
pub const CHOICE_MENU_CONTEXT: &str = "ChoiceMenu";
/// Rows a filterable menu draws at once; typing narrows the rest.
const SHOWN: usize = 100;

/// Option indexes whose titles contain `query`, ignoring case; the first `keep` always stay.
fn matching(titles: &[&str], query: &str, keep: usize) -> Vec<usize> {
    let query = query.to_lowercase();
    (0..titles.len())
        .filter(|&ix| ix < keep || titles[ix].to_lowercase().contains(&query))
        .collect()
}

#[derive(Clone, Default)]
pub struct ChoiceOption {
    /// Project name drawn as its mark, instead of `icon`.
    pub mark: Option<SharedString>,
    pub icon: Option<IconName>,
    /// Picture shown instead of `icon`, e.g. an app icon.
    pub image: Option<PathBuf>,
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub checked: bool,
    pub disabled: bool,
}

impl ChoiceOption {
    /// Disables the row, saying why under its title.
    pub fn disabled_because(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled = true;
        self.description = Some(reason.into());
        self
    }

    // IconName has no PartialEq; the icon follows the title, so it's skipped
    fn same_as(&self, other: &Self) -> bool {
        (&self.title, &self.description, &self.image, &self.mark, self.checked, self.disabled)
            == (
                &other.title,
                &other.description,
                &other.image,
                &other.mark,
                other.checked,
                other.disabled,
            )
    }
}

pub enum ChoiceMenuEvent {
    Pick(usize),
}

/// Next enabled row `delta` steps from `current`, wrapping; `None` when every row is disabled.
pub fn next_enabled(current: Option<usize>, disabled: &[bool], delta: isize) -> Option<usize> {
    let len = disabled.len() as isize;
    let mut ix = match current {
        Some(c) if (c as isize) < len => c as isize,
        _ if delta < 0 => 0,
        _ => -1,
    };
    for _ in 0..len {
        ix = (ix + delta).rem_euclid(len);
        if !disabled[ix as usize] {
            return Some(ix as usize);
        }
    }
    None
}

pub struct ChoiceMenuState {
    options: Vec<ChoiceOption>,
    header: Option<SharedString>,
    width: f32,
    below: bool,
    highlight: Option<usize>,
    /// Typed filter text; `None` when the menu doesn't filter.
    filter: Option<String>,
    /// Leading rows the filter never hides.
    keep: usize,
    open: bool,
    focus: FocusHandle,
    restore_focus: Option<FocusHandle>,
    trigger: TriggerBounds,
    scroll: ScrollHandle,
}

impl EventEmitter<ChoiceMenuEvent> for ChoiceMenuState {}

impl ChoiceMenuState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            options: Vec::new(),
            header: None,
            width: 310.,
            below: false,
            highlight: None,
            filter: None,
            keep: 0,
            open: false,
            focus: cx.focus_handle(),
            restore_focus: None,
            trigger: TriggerBounds::default(),
            scroll: ScrollHandle::new(),
        }
    }

    /// Open the panel under the trigger instead of above it.
    pub fn below(mut self) -> Self {
        self.below = true;
        self
    }

    /// Typing while open narrows the rows to titles containing the text; the
    /// first `keep` rows always show.
    pub fn filterable(mut self, keep: usize) -> Self {
        self.filter = Some(String::new());
        self.keep = keep;
        self
    }

    /// Rows shown for the current filter, as option indexes.
    fn visible(&self) -> Vec<usize> {
        let titles: Vec<&str> = self.options.iter().map(|o| o.title.as_ref()).collect();
        matching(&titles, self.filter.as_deref().unwrap_or_default(), self.keep)
    }

    /// Cheap to call every render: only a real change notifies.
    pub fn sync(
        &mut self,
        options: Vec<ChoiceOption>,
        header: Option<SharedString>,
        width: f32,
        cx: &mut Context<Self>,
    ) {
        let same = self.options.len() == options.len()
            && self.options.iter().zip(&options).all(|(a, b)| a.same_as(b));
        if !same || self.header != header || self.width != width {
            self.options = options;
            self.header = header;
            self.width = width;
            cx.notify();
        }
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_focus = window.focused(cx);
        if let Some(query) = &mut self.filter {
            query.clear();
        }
        let disabled: Vec<bool> = self.options.iter().map(|o| o.disabled).collect();
        self.highlight = self
            .options
            .iter()
            .position(|o| o.checked && !o.disabled)
            .or_else(|| next_enabled(None, &disabled, 1));
        if let Some(ix) = self.highlight {
            self.scroll.scroll_to_item(ix + usize::from(self.header.is_some()));
        }
        self.open = true;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        if let Some(previous) = self.restore_focus.take()
            && self.focus.contains_focused(window, cx)
        {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    /// Page changed under an open panel: drop it without touching focus.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.open) {
            self.restore_focus = None;
            cx.notify();
        }
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let shown: Vec<usize> = self.visible().into_iter().take(SHOWN).collect();
        let disabled: Vec<bool> = shown.iter().map(|&ix| self.options[ix].disabled).collect();
        let at = self.highlight.and_then(|h| shown.iter().position(|&ix| ix == h));
        let next = next_enabled(at, &disabled, delta);
        self.highlight = next.map(|pos| shown[pos]);
        if let Some(pos) = next {
            // rows follow an optional header child
            self.scroll.scroll_to_item(pos + usize::from(self.header.is_some()));
        }
        cx.notify();
    }

    /// Typed characters and backspace edit the filter; everything else passes on.
    fn type_filter(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(query) = &mut self.filter else {
            return;
        };
        let keys = &ev.keystroke;
        if keys.modifiers.control || keys.modifiers.platform || keys.modifiers.function {
            return;
        }
        match (keys.key.as_str(), keys.key_char.as_deref()) {
            ("backspace", _) => {
                query.pop();
            }
            (_, Some(text)) if !text.chars().any(char::is_control) => query.push_str(text),
            _ => return,
        }
        cx.stop_propagation();
        self.highlight = None;
        self.move_highlight(1, cx);
        self.scroll.scroll_to_item(0);
    }

    fn pick(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.options.get(ix).is_none_or(|o| o.disabled) {
            return;
        }
        self.close(window, cx);
        cx.emit(ChoiceMenuEvent::Pick(ix));
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.highlight {
            self.pick(ix, window, cx);
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let option = &self.options[ix];
        let (muted, fg) = (theme.muted_foreground, theme.popover_foreground);
        let disabled = option.disabled;
        let (image, icon, mark) = (&option.image, &option.icon, option.mark.clone());
        let glyph = match (mark, image, icon) {
            (Some(name), _, _) => Some(project_mark_sized(&name, px(18.), cx)),
            (None, Some(path), _) => Some(img(path.clone()).size(px(20.)).into_any_element()),
            (None, None, Some(icon)) => {
                Some(Icon::new(*icon).size(px(16.)).text_color(muted).into_any_element())
            }
            (None, None, None) => None,
        };
        h_flex()
            .id(("choice-row", ix))
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .gap(px(10.))
            .items_center()
            .corner(px(8.))
            .role(Role::MenuItem)
            .aria_label(option.title.clone())
            .aria_selected(option.checked)
            .when_some(option.description.clone(), |r, text| r.aria_description(text))
            .when(!disabled, |r| r.cursor_pointer())
            .when(disabled, |r| r.opacity(0.5))
            .when(self.highlight == Some(ix) && !disabled, |r| {
                r.bg(cx.theme().accent).aria_active_descendant()
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && !disabled && this.highlight != Some(ix) {
                    this.highlight = Some(ix);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.pick(ix, window, cx)))
            .when_some(glyph, |r, glyph| {
                r.child(
                    div()
                        .w(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(glyph),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_sm()
                            .when(option.checked, |t| t.font_semibold())
                            .when(!option.checked, |t| t.font_medium())
                            .child(option.title.clone()),
                    )
                    .when_some(option.description.clone(), |c, text| {
                        c.child(div().text_xs().text_color(muted).child(text))
                    }),
            )
            .child(div().w(px(16.)).flex_none().flex().items_center().when(option.checked, |d| {
                d.child(Icon::new(IconName::Check).size(px(16.)).text_color(fg))
            }))
    }
}

impl Render for ChoiceMenuState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let max_h = popup_height(room(self.trigger.get(), self.below, window), f32::INFINITY);
        let shown = self.visible();
        let muted = cx.theme().muted_foreground;
        let note = move |text: SharedString| {
            div().px(px(10.)).py(px(6.)).text_xs().text_color(muted).child(text)
        };
        let theme = cx.theme();
        v_flex()
            .id("choice-menu-panel")
            .key_context(CHOICE_MENU_CONTEXT)
            .track_focus(&self.focus)
            .role(Role::Menu)
            .on_action(cx.listener(|this, _: &ChoiceUp, _, cx| this.move_highlight(-1, cx)))
            .on_action(cx.listener(|this, _: &ChoiceDown, _, cx| this.move_highlight(1, cx)))
            .on_action(cx.listener(|this, _: &ChoiceFirst, _, cx| {
                this.highlight = None;
                this.move_highlight(1, cx)
            }))
            .on_action(cx.listener(|this, _: &ChoiceLast, _, cx| {
                this.highlight = None;
                this.move_highlight(-1, cx)
            }))
            .on_action(cx.listener(|this, _: &ChoiceConfirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &ChoiceDismiss, window, cx| this.close(window, cx)))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.type_filter(ev, cx)))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .w(px(self.width))
            .max_h(px(max_h))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(6.))
            .gap(px(2.))
            .corner(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_lg()
            .when_some(self.header.clone(), |p, header| {
                p.child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(match self.filter.as_deref() {
                            Some("") => format!("{header} · type to filter"),
                            Some(query) => format!("{header} · “{query}”"),
                            None => header.to_string(),
                        }),
                )
            })
            .children(shown.iter().take(SHOWN).map(|&ix| self.render_row(ix, cx)))
            .when(
                self.filter.as_deref().is_some_and(|q| !q.is_empty()) && shown.len() <= self.keep,
                |p| p.child(note("No matches. Backspace to widen.".into())),
            )
            .when(shown.len() > SHOWN, |p| {
                p.child(note(format!("{} more. Type to narrow down.", shown.len() - SHOWN).into()))
            })
    }
}

/// Chip trigger plus the popover of choices; keep the state entity across renders.
#[derive(IntoElement)]
pub struct ChoiceMenu {
    state: Entity<ChoiceMenuState>,
    trigger: Button,
}

impl ChoiceMenu {
    pub fn new(state: &Entity<ChoiceMenuState>, trigger: Button) -> Self {
        Self { state: state.clone(), trigger }
    }
}

impl RenderOnce for ChoiceMenu {
    fn render(self, _: &mut Window, cx: &mut gpui_kit::App) -> impl IntoElement {
        let state = self.state.read(cx);
        let (open, bounds, below) = (state.open, state.trigger.clone(), state.below);
        let entity = self.state.clone();
        let trigger = self.trigger.selected(open).when(!open, |b| {
            // open only: the panel's outside mouse-down already closed it, a click would reopen
            b.on_click(move |_, window, cx| entity.update(cx, |s, cx| s.open(window, cx)))
        });
        anchored_popup(("choice-menu-popup", self.state.entity_id()), trigger, &bounds, below)
            .when(open, |popup| popup.content(self.state.clone()))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/components/choice_menu.rs"]
mod tests;
