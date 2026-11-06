use crate::theme::Corners;
use gpui_kit::rems;
use std::ops::Range;

use aether_core::{Provider, effort_label};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable,
    button::{ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    scroll::ScrollableElement,
    searchable_list::SearchableListItem,
    tag::Tag,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, Render,
    Role, ScrollStrategy, SharedString, Subscription, Toggled, UniformListScrollHandle, Window,
    actions, div, prelude::*, px, uniform_list,
};

use super::ModelItem;
use crate::components::{
    TriggerBounds, anchored_popup, control_chip, field, field_fill, overlay_strong, popup_height,
    provider_icon, room,
};
use crate::theme;

actions!(
    agent_selector,
    [
        SelectorPrev,
        SelectorNext,
        SelectorFirst,
        SelectorLast,
        SelectorPrevTab,
        SelectorNextTab,
        SelectorConfirm,
        SelectorDismiss
    ]
);

/// Key context on the open panel; the app binds the selector actions under it.
pub const AGENT_SELECTOR_CONTEXT: &str = "AgentSelector";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectorTab {
    Favorites,
    Provider(Provider),
}

/// One rail tab's catalogue. `hidden` drops the tab (e.g. CLI not installed).
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderModels {
    pub provider: Provider,
    pub items: Vec<ModelItem>,
    pub hidden: bool,
}

/// Saved form of a favorite: `"<provider key>:<model id>"`.
pub fn favorite_key(provider: Provider, model: &str) -> String {
    format!("{}:{model}", provider.key())
}

pub fn parse_favorite(key: &str) -> Option<(Provider, &str)> {
    let (provider, model) = key.split_once(':')?;
    Provider::ALL.into_iter().find(|p| p.key() == provider).map(|p| (p, model))
}

fn usable(models: &ProviderModels, lock: Option<Provider>) -> bool {
    !models.hidden && lock.is_none_or(|locked| locked == models.provider)
}

/// Favorites, then each provider this chat may use, in rail order.
pub fn rail_tabs(providers: &[ProviderModels], lock: Option<Provider>) -> Vec<SelectorTab> {
    std::iter::once(SelectorTab::Favorites)
        .chain(
            providers.iter().filter(|p| usable(p, lock)).map(|p| SelectorTab::Provider(p.provider)),
        )
        .collect()
}

pub struct RailEntry {
    pub tab: SelectorTab,
    pub enabled: bool,
}

/// Favorites, then every shown provider; ones the chat is locked out of stay, disabled.
pub fn rail_entries(providers: &[ProviderModels], lock: Option<Provider>) -> Vec<RailEntry> {
    std::iter::once(RailEntry { tab: SelectorTab::Favorites, enabled: true })
        .chain(providers.iter().filter(|p| !p.hidden).map(|p| RailEntry {
            tab: SelectorTab::Provider(p.provider),
            enabled: usable(p, lock),
        }))
        .collect()
}

/// Second row line: the description inside a provider tab, else the provider or detail.
pub fn row_subtitle(item: &ModelItem, provider_label: &str, cross: bool) -> SharedString {
    if cross {
        provider_label.to_string().into()
    } else if item.description.is_empty() {
        item.detail.clone()
    } else {
        item.description.clone()
    }
}

pub fn lock_note(provider_label: &str) -> String {
    format!("This chat stays on {provider_label}. Start a new chat to switch agents.")
}

pub fn no_reasoning_note(provider_label: &str) -> String {
    format!("{provider_label} sets its own reasoning")
}

/// Rows the list shows: a non-empty query searches every usable provider,
/// otherwise the tab's own rows.
pub fn visible_rows(
    tab: SelectorTab,
    query: &str,
    lock: Option<Provider>,
    favorites: &[String],
    providers: &[ProviderModels],
) -> Vec<(Provider, ModelItem)> {
    let usable: Vec<&ProviderModels> = providers.iter().filter(|p| usable(p, lock)).collect();
    let rows = |p: &&ProviderModels, keep: &dyn Fn(&ModelItem) -> bool| {
        p.items.iter().filter(|m| keep(m)).map(|m| (p.provider, m.clone())).collect::<Vec<_>>()
    };
    let query = query.trim();
    if !query.is_empty() {
        return usable.iter().flat_map(|p| rows(p, &|m| m.matches(query))).collect();
    }
    match tab {
        SelectorTab::Provider(provider) => usable
            .iter()
            .filter(|p| p.provider == provider)
            .flat_map(|p| rows(p, &|_| true))
            .collect(),
        SelectorTab::Favorites => favorites
            .iter()
            .filter_map(|key| {
                let (provider, model) = parse_favorite(key)?;
                let p = usable.iter().find(|p| p.provider == provider)?;
                let item = p.items.iter().find(|m| m.id == model)?;
                Some((provider, item.clone()))
            })
            .collect(),
    }
}

/// Moves the highlight by `delta`, wrapping; from none, down lands first and up last.
pub fn next_highlight(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        Some(ix) if ix < len => (ix as isize + delta).rem_euclid(len as isize) as usize,
        _ if delta < 0 => len - 1,
        _ => 0,
    })
}

/// The tab `delta` steps from `current`, wrapping; first tab if `current` isn't usable.
pub fn cycle_tab(current: SelectorTab, tabs: &[SelectorTab], delta: isize) -> SelectorTab {
    match tabs.iter().position(|t| *t == current) {
        Some(ix) => tabs[(ix as isize + delta).rem_euclid(tabs.len() as isize) as usize],
        None => tabs.first().copied().unwrap_or(current),
    }
}

/// True when stepping `delta` from `current` would leave the rail, so Tab should move focus on.
pub fn leaves_rail(current: SelectorTab, tabs: &[SelectorTab], delta: isize) -> bool {
    tabs.iter()
        .position(|t| *t == current)
        .is_some_and(|ix| !(0..tabs.len() as isize).contains(&(ix as isize + delta)))
}

pub enum AgentSelectorEvent {
    Pick { provider: Provider, model: SharedString },
    ToggleFavorite { provider: Provider, model: SharedString },
    Effort(String),
    OpenSettings,
}

/// Provider rail + searchable model list behind the composer's agent chip.
/// With a single provider the rail is hidden.
pub struct AgentSelectorState {
    providers: Vec<ProviderModels>,
    favorites: Vec<String>,
    provider: Provider,
    model: SharedString,
    efforts: Vec<String>,
    effort: String,
    locked: bool,
    tab: SelectorTab,
    query: Entity<InputState>,
    highlight: Option<usize>,
    rows: Vec<(Provider, ModelItem)>,
    open: bool,
    form: bool,
    trigger: TriggerBounds,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    restore_focus: Option<FocusHandle>,
    _query_changes: Subscription,
}

impl EventEmitter<AgentSelectorEvent> for AgentSelectorState {}

impl AgentSelectorState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search models"));
        let _query_changes = cx.subscribe(&query, |this, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                this.highlight = None;
                this.refresh(cx);
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
        });
        Self {
            providers: Vec::new(),
            favorites: Vec::new(),
            provider: Provider::default(),
            model: SharedString::default(),
            efforts: Vec::new(),
            effort: String::new(),
            locked: false,
            tab: SelectorTab::Favorites,
            query,
            highlight: None,
            rows: Vec::new(),
            open: false,
            form: false,
            trigger: TriggerBounds::default(),
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            restore_focus: None,
            _query_changes,
        }
    }

    /// Form-control trigger whose panel drops below it, for settings pages.
    pub fn form(mut self) -> Self {
        self.form = true;
        self
    }

    /// Catalogues in rail order, saved favorites, and the chat's provider, model row and effort.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        &mut self,
        providers: Vec<ProviderModels>,
        favorites: Vec<String>,
        provider: Provider,
        model: SharedString,
        efforts: Vec<String>,
        effort: String,
        cx: &mut Context<Self>,
    ) {
        self.providers = providers;
        self.favorites = favorites;
        self.provider = provider;
        self.model = model;
        self.efforts = efforts;
        self.effort = effort;
        self.refresh(cx);
    }

    fn lock(&self) -> Option<Provider> {
        self.locked.then_some(self.provider)
    }

    fn single(&self) -> bool {
        self.providers.len() == 1
    }

    fn has_agents(&self) -> bool {
        rail_tabs(&self.providers, self.lock()).len() > 1
    }

    fn searching(&self, cx: &App) -> bool {
        !self.query.read(cx).value().trim().is_empty()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value();
        let key = self.highlight.and_then(|ix| self.rows.get(ix)).map(|(p, m)| (*p, m.id.clone()));
        self.rows = visible_rows(self.tab, &query, self.lock(), &self.favorites, &self.providers);
        // by identity: unstarring in Favorites or a catalogue landing shifts indices
        self.highlight =
            key.and_then(|(p, id)| self.rows.iter().position(|(rp, m)| *rp == p && m.id == id));
        cx.notify();
    }

    /// Model name plus the effort label, for the folded composer bar.
    pub fn model_summary(&self) -> SharedString {
        let name = self.current_name();
        match self.shown_effort() {
            Some(effort) => format!("{name} {effort}").into(),
            None => name,
        }
    }

    fn shown_effort(&self) -> Option<String> {
        effort_shown(&self.efforts, &self.effort)
    }

    fn current_name(&self) -> SharedString {
        self.providers
            .iter()
            .find(|p| p.provider == self.provider)
            .and_then(|p| p.items.iter().find(|m| m.id == self.model))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| {
                if self.model.is_empty() {
                    self.provider.label().into()
                } else {
                    self.model.clone()
                }
            })
    }

    fn open(&mut self, locked: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.locked = locked;
        self.restore_focus = window.focused(cx);
        self.tab = SelectorTab::Provider(self.provider);
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        self.refresh(cx);
        let (provider, model) = (self.provider, &self.model);
        self.highlight = self.rows.iter().position(|(p, m)| *p == provider && m.id == *model);
        self.scroll.scroll_to_item(self.highlight.unwrap_or(0), ScrollStrategy::Center);
        self.open = true;
        if self.has_agents() {
            self.query.update(cx, |q, cx| q.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
    }

    /// Chat or page changed under an open panel: drop it without touching focus.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.locked = false;
        self.restore_focus = None;
        self.highlight = None;
        cx.notify();
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

    fn select_tab(&mut self, tab: SelectorTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.highlight = None;
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        self.refresh(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.highlight = next_highlight(self.highlight, self.rows.len(), delta);
        if let Some(ix) = self.highlight {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn cycle(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.searching(cx) || self.single() {
            cx.propagate();
            return;
        }
        let tabs = rail_tabs(&self.providers, self.lock());
        if leaves_rail(self.tab, &tabs, delta) {
            cx.propagate();
            return;
        }
        let tab = cycle_tab(self.tab, &tabs, delta);
        self.select_tab(tab, window, cx);
    }

    fn pick(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((provider, item)) = self.rows.get(ix).cloned() else {
            return;
        };
        self.close(window, cx);
        cx.emit(AgentSelectorEvent::Pick { provider, model: item.id });
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_agents() {
            self.pick(self.highlight.unwrap_or(0), window, cx);
        } else {
            self.open_settings(window, cx);
        }
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        cx.emit(AgentSelectorEvent::OpenSettings);
    }

    fn rail_tab(
        &self,
        ix: usize,
        tab: SelectorTab,
        icon: Icon,
        label: &'static str,
        usable: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.tab == tab && !self.searching(cx);
        let tip: SharedString = if usable {
            label.into()
        } else {
            format!("{label} · this chat stays on {}", self.provider.label()).into()
        };
        let theme = cx.theme();
        div()
            .id(("selector-tab", ix))
            .relative()
            .size(px(36.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .corner(rems(0.375))
            .role(Role::Tab)
            .aria_label(label)
            .aria_selected(selected)
            .when(!usable, |t| t.aria_description(tip.clone()))
            .when(selected, |t| {
                t.bg(theme.secondary).child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(9.))
                        .bottom(px(9.))
                        .w(px(3.))
                        .rounded_full()
                        .bg(theme.primary),
                )
            })
            .when(usable, |t| {
                t.cursor_pointer().hover(|t| t.bg(theme.muted)).on_click(
                    cx.listener(move |this, _, window, cx| this.select_tab(tab, window, cx)),
                )
            })
            .when(!usable, |t| t.opacity(0.3))
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .child(icon.size(px(18.)))
    }

    fn render_rail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entries = rail_entries(&self.providers, self.lock());
        let favorites = self.rail_tab(
            0,
            SelectorTab::Favorites,
            Icon::new(IconName::Star),
            "Favorites",
            true,
            cx,
        );
        let tabs: Vec<_> = entries
            .into_iter()
            .skip(1)
            .enumerate()
            .filter_map(|(ix, e)| match e.tab {
                SelectorTab::Provider(p) => {
                    Some(self.rail_tab(ix + 1, e.tab, provider_icon(p), p.label(), e.enabled, cx))
                }
                SelectorTab::Favorites => None,
            })
            .collect();
        let theme = cx.theme();
        v_flex()
            .id("selector-rail")
            .role(Role::TabList)
            .aria_label("Providers")
            .w(px(50.))
            .h_full()
            .flex_none()
            .items_center()
            .gap_1()
            .py_2()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.muted.opacity(0.4))
            .child(favorites)
            .child(div().w(px(28.)).h(px(1.)).my_1().bg(theme.border))
            .children(tabs)
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (provider, item) = self.rows[ix].clone();
        let current = provider == self.provider && item.id == self.model;
        let highlighted = self.highlight == Some(ix);
        let favorite = self.favorites.contains(&favorite_key(provider, &item.id));
        let star_label = if favorite { "Remove from favorites" } else { "Add to favorites" };
        let group = SharedString::from(format!("selector-row-{ix}"));
        let cross = self.searching(cx) || self.tab == SelectorTab::Favorites;
        let subtitle = row_subtitle(&item, provider.label(), cross);
        let mango = theme::mango_text(cx);
        let theme = cx.theme();
        let star = div()
            .id(("selector-star", ix))
            .size(px(26.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .corner(rems(0.375))
            .cursor_pointer()
            .role(Role::Button)
            .aria_label(star_label)
            .aria_toggled(if favorite { Toggled::True } else { Toggled::False })
            .hover(|s| s.bg(theme.muted))
            .when(!favorite && !highlighted, |s| {
                s.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .tooltip(move |window, cx| Tooltip::new(star_label).build(window, cx))
            .on_click({
                let model = item.id.clone();
                cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    let model = model.clone();
                    cx.emit(AgentSelectorEvent::ToggleFavorite { provider, model });
                })
            })
            .child(if favorite {
                Icon::new(IconName::StarFill).small().text_color(mango)
            } else {
                Icon::new(IconName::Star).small().text_color(theme.muted_foreground)
            });
        h_flex()
            .id(("selector-row", ix))
            .group(group)
            .w_full()
            .h(px(52.))
            .px_2()
            .gap_2()
            .corner(rems(0.375))
            .cursor_pointer()
            .role(Role::ListBoxOption)
            .aria_label(item.title())
            .aria_selected(current)
            .when(current, |r| r.bg(theme.secondary))
            .when(highlighted, |r| r.bg(theme.accent).aria_active_descendant())
            .hover(|r| r.bg(theme.accent))
            .on_click(cx.listener(move |this, _, window, cx| this.pick(ix, window, cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().truncate().child(item.name.clone()))
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(provider_icon(provider).xsmall())
                            .child(div().min_w_0().truncate().child(subtitle)),
                    ),
            )
            .when(item.default, |r| r.child(Tag::secondary().small().flex_none().child("default")))
            .when(current, |r| {
                r.child(Icon::new(IconName::Check).small().flex_none().text_color(theme.primary))
            })
            .child(star)
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .w(px(320.))
            .when(self.form, |body| body.w_full())
            .p_5()
            .gap_3()
            .items_center()
            .child(Icon::new(IconName::Bot).size(px(20.)).text_color(muted))
            .child(div().text_sm().child("No agents available"))
            .child(
                crate::components::button("selector-open-settings")
                    .small()
                    .outline()
                    .icon(IconName::Settings)
                    .label("Open provider settings")
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
    }
}

impl Render for AgentSelectorState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = popup_height(room(self.trigger.get(), self.form, window), 470.);
        let panel = div()
            .id("agent-selector-panel")
            .key_context(AGENT_SELECTOR_CONTEXT)
            .role(Role::Dialog)
            .aria_label("Choose agent and model")
            .on_action(cx.listener(|this, _: &SelectorPrev, _, cx| this.move_highlight(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectorNext, _, cx| this.move_highlight(1, cx)))
            .on_action(cx.listener(|this, _: &SelectorFirst, _, cx| {
                this.highlight = None;
                this.move_highlight(1, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectorLast, _, cx| {
                this.highlight = None;
                this.move_highlight(-1, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectorPrevTab, window, cx| this.cycle(-1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectorNextTab, window, cx| this.cycle(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectorConfirm, window, cx| this.confirm(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectorDismiss, window, cx| this.close(window, cx)))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .overflow_hidden()
            .corner(rems(0.5))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .when(self.form, |panel| panel.w(px(280.)))
            .text_color(cx.theme().popover_foreground)
            .shadow_lg();
        if !self.has_agents() {
            // only here: a focus-tracking panel would take focus from the search field on every click
            return panel.track_focus(&self.focus).child(self.render_empty_state(cx));
        }
        let rows = if self.rows.is_empty() {
            let text = if self.searching(cx) {
                "No models match your search"
            } else if self.tab == SelectorTab::Favorites && self.favorites.is_empty() {
                "No favorites yet. Star a model to keep it here."
            } else if self.tab == SelectorTab::Favorites {
                "No favorites available for this chat"
            } else {
                "No models available"
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text)
                .into_any_element()
        } else {
            uniform_list(
                "selector-rows",
                self.rows.len(),
                cx.processor(|this, range: Range<usize>, _, cx| {
                    range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full()
            .px_2()
            .py_1()
            .into_any_element()
        };
        let single = self.single();
        let theme = cx.theme();
        let (fg, bg, border, muted, ring) =
            (theme.foreground, theme.background, theme.border, theme.muted_foreground, theme.ring);
        let reasoning = (!self.form).then(|| {
            let body = if self.efforts.is_empty() {
                div()
                    .px_2()
                    .pb_2()
                    .text_xs()
                    .text_color(muted)
                    .child(no_reasoning_note(self.provider.label()))
                    .into_any_element()
            } else {
                h_flex()
                    .track_focus(&self.focus)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.query.focus_handle(cx).focus(window, cx);
                            window.prevent_default();
                        }),
                    )
                    .flex_wrap()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .children(
                        std::iter::once(String::new()).chain(self.efforts.clone()).enumerate().map(
                            |(ix, level)| {
                                let on = level == self.effort;
                                let label = effort_label(&level);
                                div()
                                    .id(("selector-effort", ix))
                                    .h(px(26.))
                                    .px(px(9.))
                                    .flex()
                                    .items_center()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if on { fg } else { border })
                                    .text_size(px(12.))
                                    .text_color(if on { bg } else { muted })
                                    .cursor_pointer()
                                    .tab_index(0)
                                    .focus_visible(move |s| s.border_color(ring))
                                    .role(Role::Button)
                                    .aria_label(label.clone())
                                    .aria_toggled(if on { Toggled::True } else { Toggled::False })
                                    .when(on, |p| p.bg(fg))
                                    .when(!on, |p| p.hover(|p| p.text_color(fg)))
                                    // the panel binds Enter to picking a model; a focused pill keeps it
                                    .on_action(cx.listener({
                                        let level = level.clone();
                                        move |_, _: &SelectorConfirm, _, cx| {
                                            cx.emit(AgentSelectorEvent::Effort(level.clone()))
                                        }
                                    }))
                                    .on_click(cx.listener(
                                        move |this, ev: &ClickEvent, window, cx| {
                                            if !ev.is_keyboard() {
                                                this.query.focus_handle(cx).focus(window, cx);
                                            }
                                            cx.emit(AgentSelectorEvent::Effort(level.clone()))
                                        },
                                    ))
                                    .child(label)
                            },
                        ),
                    )
                    .into_any_element()
            };
            v_flex()
                .border_t_1()
                .border_color(border)
                .child(div().px_2().pt_2().pb_1().text_xs().text_color(muted).child("Reasoning"))
                .child(body)
        });
        let lock_row = self.locked.then(|| {
            h_flex()
                .gap_1p5()
                .px_2()
                .py_2()
                .border_t_1()
                .border_color(border)
                .text_size(px(11.5))
                .text_color(muted)
                .child(Icon::new(gpui_kit::assets::IconName::Lock).size(px(12.)).flex_none())
                .child(lock_note(self.provider.label()))
        });
        panel
            .w(px(if self.form {
                280.
            } else if single {
                410.
            } else {
                460.
            }))
            .h(px(height))
            .flex()
            .when(!single, |p| p.child(self.render_rail(cx)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div().p_2().border_b_1().border_color(cx.theme().border).child(
                            field(&self.query)
                                .small()
                                .icon(IconName::Search)
                                .aria_label("Search models"),
                        ),
                    )
                    .child(
                        div()
                            .id("selector-list")
                            .role(Role::ListBox)
                            .aria_label("Models")
                            .w_full()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(rows)
                            .vertical_scrollbar(&self.scroll),
                    )
                    .children(reasoning)
                    .children(lock_row),
            )
    }
}

/// Composer chip showing the chat's provider and model; opens the selector.
#[derive(IntoElement)]
pub struct AgentSelector {
    state: Entity<AgentSelectorState>,
    locked: bool,
    disabled: bool,
    resting: bool,
}

impl AgentSelector {
    pub fn new(state: &Entity<AgentSelectorState>) -> Self {
        Self { state: state.clone(), locked: false, disabled: false, resting: false }
    }

    /// Smaller trigger for the resting composer.
    pub fn resting(mut self, resting: bool) -> Self {
        self.resting = resting;
        self
    }

    /// Only the chat's own provider can be picked.
    pub fn locked(mut self, locked: bool) -> Self {
        self.locked = locked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for AgentSelector {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let (open, provider, name) = (state.open, state.provider, state.current_name());
        let effort = state.shown_effort();
        let (form, bounds) = (state.form, state.trigger.clone());
        let locked = self.locked;
        let entity = self.state.clone();
        let trigger = if form {
            let fill = overlay_strong(cx);
            crate::components::button("agent-selector")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(field_fill(cx))
                        .foreground(cx.theme().foreground)
                        .hover(fill)
                        .active(fill),
                )
                .bg(field_fill(cx))
                .cursor_pointer()
                .w(px(280.))
                .h(px(40.))
                .px(px(12.))
                .corner(px(10.))
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .child(provider_icon(provider).small())
                        .child(div().min_w_0().truncate().child(name.clone())),
                )
                .dropdown_caret(true)
        } else {
            let muted = cx.theme().muted_foreground;
            let chip = control_chip("agent-selector", false, cx);
            let chip = if self.resting { chip.h(px(22.)).text_size(px(11.5)) } else { chip };
            chip.child(
                h_flex()
                    .min_w_0()
                    .gap(px(6.))
                    .child(provider_icon(provider).small())
                    .child(div().min_w_0().truncate().child(name.clone()))
                    .children(effort.map(|e| div().flex_none().text_color(muted).child(e))),
            )
            .max_w(px(280.))
            .tooltip(if self.disabled {
                "Can't change while the agent is running"
            } else {
                "Choose agent and model"
            })
        };
        let trigger = trigger
            .accessibility_label(format!("{}, {name}", provider.label()))
            .disabled(self.disabled)
            .selected(open)
            // open only: the panel's outside mouse-down already closed it, a click would reopen
            .when(!open, |b| {
                b.on_click(move |_, window, cx| {
                    entity.update(cx, |s, cx| s.open(locked, window, cx));
                })
            });
        anchored_popup(("agent-selector-popup", self.state.entity_id()), trigger, &bounds, form)
            .when(open, |popup| popup.content(self.state.clone()))
    }
}

fn effort_shown(efforts: &[String], effort: &str) -> Option<String> {
    (!efforts.is_empty() && !effort.is_empty()).then(|| effort_label(effort))
}

#[cfg(test)]
#[path = "../../tests/unit/chat/agent_selector.rs"]
mod tests;
