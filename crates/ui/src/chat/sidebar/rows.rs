use crate::theme::Corners;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, button::ButtonVariants, h_flex,
    input::InputState, spinner::Spinner, v_flex,
};
use gpui_kit::{
    AnyElement, App, Div, Entity, Hsla, KeyDownEvent, Role, SharedString, Window, div, prelude::*,
    px, rems,
};

use super::chat_row::render_chat;
use super::chrome::{nest, reveal_on_hover, row_hover, row_style, tabular, tooltip};
use super::crew::{CrewMember, CrewSummary, Tier, order, preview, starts_collapsed};
use super::menu::track_menu;
use super::project_row::ProjectRow;
use super::state::{SidebarState, navigate};
use super::{
    AgentState, ChatItem, ChatLine, ChatSidebar, CollapseHandler, OutsideLine, OutsideState,
    ProjectHeader, ProjectItem, RowHandler, ToolsFactory, WorktreeItem,
};
use crate::components::provider_icon;
use aether_core::{Provider, SidebarStyle};
use std::collections::HashMap;
use std::rc::Rc;

/// One sidebar body row. Custom rows so chat items match the bottom
/// navigation's sizing instead of the fixed-height menu item, and so each
/// chat gets a right-click menu.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(super) enum ChatRow {
    Header {
        title: SharedString,
        on_add: Option<RowHandler>,
        collapse: Option<(bool, CollapseHandler)>,
        tools: Option<ToolsFactory>,
    },
    Agent {
        ix: usize,
        title: SharedString,
        provider: Provider,
        detail: SharedString,
        project: Option<SharedString>,
        state: AgentState,
        on_click: RowHandler,
        on_stop: Option<RowHandler>,
    },
    Outside {
        line: OutsideLine,
        on_click: RowHandler,
        on_stop: RowHandler,
    },
    Link {
        label: SharedString,
        on_click: RowHandler,
    },
    Project {
        id: usize,
        header: ProjectHeader,
        /// Not the first group, so it gets space above.
        spaced: bool,
        /// Nested rows follow, so the guide starts at the folder icon.
        guide: bool,
        /// Under a project group, hanging off its guide.
        nested: bool,
        on_collapse: CollapseHandler,
        on_new: RowHandler,
        on_files: RowHandler,
        on_hide: RowHandler,
        on_page: RowHandler,
        now: i64,
    },
    Worktree {
        id: usize,
        header: ProjectHeader,
        on_collapse: CollapseHandler,
        on_new: RowHandler,
        now: i64,
    },
    Chat {
        chat: ChatItem,
        nested: bool,
        rename_editor: Option<Entity<InputState>>,
        style: SidebarStyle,
        crew: Option<CrewSummary>,
        open: Option<bool>,
        members: usize,
        parent: Option<Provider>,
        tier: Tier,
        carded: bool,
        git: bool,
    },
    CardHeader {
        lead: i64,
        card: i64,
        title: SharedString,
        count: usize,
        depth: u8,
        nested: bool,
    },
    CrewMore {
        lead: i64,
        hidden: CrewSummary,
        all: bool,
        total: usize,
        depth: u8,
        nested: bool,
        tier: Tier,
    },
    SectionButton {
        icon: Icon,
        label: SharedString,
        aria: SharedString,
        count: Option<usize>,
        on_click: RowHandler,
    },
}

impl ChatRow {
    pub(super) fn key(&self) -> SharedString {
        match self {
            Self::Header { title, .. } => format!("caption:{title}").into(),
            Self::Agent { ix, .. } => format!("agent:{ix}").into(),
            Self::Outside { line, .. } => format!("outside:{}", line.id).into(),
            Self::Link { label, .. } => format!("link:{label}").into(),
            Self::Project { header, .. } => format!("project:{}", header.path).into(),
            Self::Worktree { header, .. } => format!("worktree:{}", header.path).into(),
            Self::Chat { chat, .. } if chat.pin => format!("pin:{}", chat.id).into(),
            Self::Chat { chat, .. } => chat_key(chat.id),
            Self::CardHeader { lead, card, .. } => format!("card:{lead}:{card}").into(),
            Self::CrewMore { lead, .. } => format!("crew-more:{lead}").into(),
            Self::SectionButton { label, .. } => format!("section-btn:{label}").into(),
        }
    }

    pub(super) fn focusable(&self) -> bool {
        match self {
            Self::Header { collapse, .. } => collapse.is_some(),
            Self::Chat { rename_editor, .. } => rename_editor.is_none(),
            Self::CrewMore { .. } | Self::CardHeader { .. } => false,
            _ => true,
        }
    }

    pub(super) fn render(
        self,
        key: SharedString,
        state: &Entity<SidebarState>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let focus = state.read(cx).focus.get(&key).cloned();
        let nav = {
            let (state, key) = (state.clone(), key.clone());
            move |ev: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                if navigate(&state, &key, ev, window, cx) {
                    cx.stop_propagation();
                }
            }
        };
        let ring = cx.theme().ring;
        match self {
            Self::Header { title, on_add, collapse, tools } => h_flex()
                .id("sidebar-heading")
                .flex_shrink_0()
                .px_2()
                .text_xs()
                .text_color(cx.theme().sidebar_foreground.opacity(0.7))
                .h_8()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .items_center()
                        .child(div().truncate().child(title.clone())),
                )
                .when_some(collapse, |row, (collapsed, on_collapse)| {
                    let click = on_collapse.clone();
                    row.role(Role::Button)
                        .when(collapsed, |row| row.font_bold())
                        .aria_label(title)
                        .aria_expanded(!collapsed)
                        .when_some(focus, |row, focus| row.track_focus(&focus))
                        .cursor_pointer()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(gpui_kit::transparent_black())
                        .focus_visible(move |s| s.border_color(ring))
                        .on_click(move |_, window, cx| click(&!collapsed, window, cx))
                        .on_key_down(move |event, window, cx| {
                            if event.keystroke.modifiers.modified() {
                                return;
                            }
                            let next = match event.keystroke.key.as_str() {
                                "enter" | "space" => !collapsed,
                                "left" => true,
                                "right" => false,
                                _ => return nav(event, window, cx),
                            };
                            cx.stop_propagation();
                            on_collapse(&next, window, cx);
                        })
                })
                .when_some(tools, |row, tools| row.child(tools(window, cx)))
                .when_some(on_add, |row, on_add| {
                    row.child(
                        crate::components::button("add-project")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .accessibility_label("Add project")
                            .tooltip("Add project")
                            .on_click(move |ev, window, cx| {
                                cx.stop_propagation();
                                on_add(ev, window, cx)
                            }),
                    )
                })
                .into_any_element(),
            Self::Agent { ix, title, provider, detail, project, state, on_click, on_stop } => {
                let label = SharedString::from(state.label());
                let stop_label = agent_stop_label(&title);
                row_style(
                    h_flex()
                        .id(("agent", ix))
                        .when_some(focus, |el, focus| el.track_focus(&focus))
                        .role(Role::Button)
                        .aria_label(format!("{title}, {label}")),
                    false,
                    cx,
                )
                .w_full()
                .mb_0p5()
                .gap_2()
                .px_2()
                .py_1p5()
                .text_sm()
                .child(agent_lines(
                    title,
                    provider,
                    detail,
                    project,
                    label,
                    state.color(cx),
                    matches!(state, AgentState::Working(_)),
                    cx,
                ))
                .when_some(on_stop.filter(|_| state.busy()), |row, on_stop| {
                    row.child(
                        crate::components::button(("agent-stop", ix))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Square)
                            .accessibility_label(stop_label)
                            .tooltip(agent_stop_tooltip())
                            .on_click(move |ev, window, cx| {
                                cx.stop_propagation();
                                on_stop(ev, window, cx)
                            }),
                    )
                })
                .on_click(move |ev, window, cx| on_click(ev, window, cx))
                .on_key_down(nav)
                .into_any_element()
            }
            Self::Outside { line, on_click, on_stop } => {
                let OutsideLine { title, provider, detail, state, label, tip, .. } = line;
                let aria = format!("{title}, {}, {detail}, {tip}", provider.label());
                let keyboard = focus.as_ref().is_some_and(|f| f.contains_focused(window, cx))
                    && window.last_input_was_keyboard();
                let stop = reveal_on_hover(div().flex_none(), key.clone(), keyboard).child(
                    crate::components::button("outside-stop")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Square)
                        .accessibility_label(agent_stop_label(&title))
                        .tooltip(agent_stop_tooltip())
                        .on_click(move |ev, window, cx| {
                            cx.stop_propagation();
                            on_stop(ev, window, cx)
                        }),
                );
                row_style(
                    h_flex()
                        .id("outside-agent")
                        .when_some(focus, |el, focus| el.track_focus(&focus))
                        .role(Role::Button)
                        .aria_label(aria),
                    false,
                    cx,
                )
                .group(key)
                .w_full()
                .mb_0p5()
                .gap_2()
                .px_2()
                .py_1p5()
                .text_sm()
                .tooltip(tooltip(tip))
                .child(agent_lines(
                    title,
                    provider,
                    detail,
                    None,
                    label,
                    state.color(cx),
                    state == OutsideState::Working,
                    cx,
                ))
                .child(stop)
                .on_click(move |ev, window, cx| on_click(ev, window, cx))
                .on_key_down(nav)
                .into_any_element()
            }
            Self::Link { label, on_click } => row_style(
                h_flex()
                    .id("link")
                    .when_some(focus, |el, focus| el.track_focus(&focus))
                    .role(Role::Button)
                    .aria_label(label.clone()),
                false,
                cx,
            )
            .w_full()
            .mb_0p5()
            .px_2()
            .py_1p5()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(label)
            .on_click(move |ev, window, cx| on_click(ev, window, cx))
            .on_key_down(nav)
            .into_any_element(),
            Self::Project {
                id,
                header,
                spaced,
                guide,
                nested,
                on_collapse,
                on_new,
                on_files,
                on_hide,
                on_page,
                now,
            } => {
                let menu_open = state.read(cx).open_menu.as_ref() == Some(&key);
                let keyboard = focus.as_ref().is_some_and(|f| f.contains_focused(window, cx))
                    && window.last_input_was_keyboard();
                ProjectRow {
                    id,
                    header,
                    spaced,
                    guide,
                    nested,
                    worktree: false,
                    on_collapse,
                    on_new,
                    on_files: Some(on_files),
                    on_hide: Some(on_hide),
                    on_page: Some(on_page),
                    now,
                }
                .render(
                    focus,
                    keyboard || menu_open,
                    track_menu(state, &key),
                    nav,
                    key,
                    cx,
                )
            }
            Self::Worktree { id, header, on_collapse, on_new, now } => {
                let menu_open = state.read(cx).open_menu.as_ref() == Some(&key);
                let keyboard = focus.as_ref().is_some_and(|f| f.contains_focused(window, cx))
                    && window.last_input_was_keyboard();
                ProjectRow {
                    id,
                    header,
                    spaced: false,
                    guide: true,
                    nested: true,
                    worktree: true,
                    on_collapse,
                    on_new,
                    on_files: None,
                    on_hide: None,
                    on_page: None,
                    now,
                }
                .render(
                    focus,
                    keyboard || menu_open,
                    track_menu(state, &key),
                    nav,
                    key,
                    cx,
                )
            }
            Self::Chat {
                chat,
                nested,
                rename_editor,
                style,
                crew,
                open,
                members,
                parent,
                tier,
                carded,
                git,
            } => {
                let menu_open = state.read(cx).open_menu.as_ref() == Some(&key);
                let keyboard = focus.as_ref().is_some_and(|f| f.contains_focused(window, cx))
                    && window.last_input_was_keyboard();
                let show = chat.active || keyboard || menu_open;
                let depth = chat.depth.min(2);
                let lead = chat.id;
                let toggle = open.map(|_| {
                    let state = state.clone();
                    move |next: bool, window: &mut Window, cx: &mut App| {
                        state.update(cx, |s, cx| {
                            s.crew_open.insert(lead, next);
                            cx.notify();
                        });
                        window.refresh();
                    }
                });
                let fill = if chat.active { cx.theme().sidebar_accent } else { row_hover(cx) };
                let chevron = open.zip(toggle.clone()).map(|(open, toggle)| {
                    let chevron = div()
                        .id(("crew-chevron", chat.ix))
                        .debug_selector(move || format!("crew-chevron-{lead}"))
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .bg(fill)
                        .role(Role::Button)
                        .aria_label(format!(
                            "{} crew, {}",
                            if open { "Collapse" } else { "Expand" },
                            crew.unwrap_or_default().words(Tier::Wide)
                        ))
                        .aria_expanded(open)
                        .cursor_pointer()
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            toggle(!open, window, cx)
                        })
                        .child(
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(rems(0.75))
                            .text_color(cx.theme().foreground),
                        );
                    reveal_on_hover(chevron, key.clone(), keyboard).into_any_element()
                });
                let all = open == Some(true) && state.read(cx).crew_all.contains(&lead);
                let show_all = (members > 5).then(|| {
                    let state = state.clone();
                    let toggle: RowHandler = Rc::new(move |_, window, cx| {
                        state.update(cx, |s, cx| {
                            if !s.crew_all.remove(&lead) || !all {
                                s.crew_all.insert(lead);
                                s.crew_open.insert(lead, true);
                            }
                            cx.notify();
                        });
                        window.refresh();
                    });
                    (show_all_label(all, members), toggle)
                });
                let nav = move |ev: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                    let next = match ev.keystroke.key.as_str() {
                        "left" => false,
                        "right" => true,
                        _ => return nav(ev, window, cx),
                    };
                    if let Some(toggle) = &toggle
                        && !ev.keystroke.modifiers.modified()
                    {
                        cx.stop_propagation();
                        toggle(next, window, cx);
                    }
                };
                let row = render_chat(
                    chat,
                    nested,
                    focus,
                    show,
                    track_menu(state, &key),
                    nav,
                    key,
                    rename_editor,
                    style,
                    chevron,
                    crew.zip(open).map(|(summary, open)| (summary, open, members)),
                    show_all,
                    parent,
                    tier,
                    carded,
                    git,
                    cx,
                );
                (0..depth).fold(nest(row, nested, cx), |row, _| nest(row, true, cx))
            }
            Self::CardHeader { card, title, count, depth, nested, .. } => {
                let row = h_flex()
                    .w_full()
                    .px_2()
                    .pt_1p5()
                    .pb_0p5()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .justify_between()
                    .child(div().min_w_0().truncate().child(format!("#{card} {title}")))
                    .child(div().flex_none().font_features(tabular()).child(count.to_string()));
                (0..depth).fold(nest(row, nested, cx), |row, _| nest(row, true, cx))
            }
            Self::CrewMore { lead, hidden, all, total, depth, nested, tier } => {
                let state = state.clone();
                let row = h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .justify_between()
                    .child(div().min_w_0().truncate().when(!all, |el| {
                        el.child(if tier == Tier::Wide {
                            hidden
                                .parts(Tier::Wide)
                                .iter()
                                .map(|(b, n)| format!("+{n} {}", b.word()))
                                .collect::<Vec<_>>()
                                .join(", ")
                        } else {
                            format!("+{} more", hidden.total())
                        })
                    }))
                    .child(
                        div()
                            .id("crew-more")
                            .debug_selector(move || format!("crew-more-{lead}"))
                            .flex_none()
                            .role(Role::Button)
                            .cursor_pointer()
                            .hover(|s| s.text_color(cx.theme().sidebar_foreground))
                            .child(if all {
                                "Show fewer".to_string()
                            } else {
                                format!("Show all {total}")
                            })
                            .on_click(move |_, window, cx| {
                                state.update(cx, |s, cx| {
                                    if !s.crew_all.remove(&lead) {
                                        s.crew_all.insert(lead);
                                    }
                                    cx.notify();
                                });
                                window.refresh();
                            }),
                    );
                (0..depth).fold(nest(row, nested, cx), |row, _| nest(row, true, cx))
            }
            Self::SectionButton { icon, label, aria, count, on_click } => {
                let muted = cx.theme().muted_foreground;
                row_style(
                    h_flex()
                        .id("chats-section-button")
                        .when_some(focus, |el, focus| el.track_focus(&focus))
                        .role(Role::Button)
                        .aria_label(aria),
                    false,
                    cx,
                )
                .w_full()
                .mb_0p5()
                .px_2()
                .py_1p5()
                .gap_2()
                .items_center()
                .text_sm()
                .child(icon.size(rems(0.875)).text_color(muted).flex_none())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().sidebar_foreground)
                        .child(label),
                )
                .when_some(count, |row, count| row.child(count_chip(count, cx)))
                .child(
                    Icon::new(IconName::ChevronRight)
                        .size(rems(0.75))
                        .text_color(muted)
                        .flex_none(),
                )
                .on_click(move |ev, window, cx| on_click(ev, window, cx))
                .on_key_down(nav)
                .into_any_element()
            }
        }
    }
}

pub fn agent_stop_label(title: &str) -> String {
    format!("Stop {title}")
}

pub fn agent_stop_tooltip() -> &'static str {
    "Stop execution (Esc)"
}

/// Title over provider icon, detail and state words: Agents and Outside aether rows.
fn agent_lines(
    title: SharedString,
    provider: Provider,
    detail: SharedString,
    project: Option<SharedString>,
    label: SharedString,
    color: Hsla,
    working: bool,
    cx: &App,
) -> Div {
    v_flex().flex_1().min_w_0().gap_0p5().child(div().truncate().child(title)).child(
        h_flex()
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .when(project.is_none(), |row| {
                row.child(provider_icon(provider).size(rems(0.75)).flex_none())
                    .child(div().flex_1().min_w_0().truncate().child(detail))
            })
            .when_some(project, |row, project| {
                row.child(super::project_mark(Some(&project), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_medium()
                            .text_color(super::project_color(&project, cx))
                            .child(project),
                    )
                    .child(
                        div()
                            .id("agent-provider")
                            .flex_none()
                            .tooltip(tooltip(provider.label().into()))
                            .child(provider_icon(provider).size(rems(0.75))),
                    )
            })
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .px_1p5()
                    .py_0p5()
                    .text_color(color)
                    .font_features(tabular())
                    .when(working, |row| row.child(Spinner::new().xsmall().color(color)))
                    .child(div().flex_shrink_0().font_medium().child(label)),
            ),
    )
}

pub(super) fn count_chip(count: usize, cx: &App) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .py_0p5()
        .corner(px(4.))
        .text_xs()
        .font_features(tabular())
        .bg(cx.theme().muted.opacity(0.4))
        .text_color(cx.theme().muted_foreground)
        .child(count.to_string())
}

pub fn outside_aria(count: usize) -> String {
    let agents = if count == 1 { "1 agent".to_string() } else { format!("{count} agents") };
    format!("Outside aether, {agents}")
}

fn show_all_label(all: bool, total: usize) -> SharedString {
    if all { "Show fewer agents".into() } else { format!("Show all {total} agents").into() }
}

pub(super) fn chat_key(id: i64) -> SharedString {
    format!("chat:{id}").into()
}

fn families(chats: Vec<ChatItem>) -> Vec<(ChatItem, Vec<ChatItem>)> {
    let mut out: Vec<(ChatItem, Vec<ChatItem>)> = Vec::new();
    for chat in chats {
        match out.last_mut() {
            Some((head, rest)) if chat.depth > head.depth => rest.push(chat),
            _ => out.push((chat, Vec::new())),
        }
    }
    out
}

fn tally(list: &[ChatItem], out: &mut HashMap<i64, CrewSummary>) {
    for (i, lead) in list.iter().enumerate().filter(|(_, c)| c.lead && !c.pin) {
        let mut summary = CrewSummary::default();
        for chat in list[i + 1..].iter().take_while(|c| c.depth > lead.depth) {
            summary.add(chat.status);
        }
        out.insert(lead.id, summary);
    }
}

fn git_differs(a: &ChatLine, b: &ChatLine) -> bool {
    (&a.place, a.added, a.deleted) != (&b.place, b.added, b.deleted)
}

fn summarize(chats: &[ChatItem]) -> CrewSummary {
    let mut summary = CrewSummary::default();
    for chat in chats {
        summary.add(chat.status);
    }
    summary
}

impl ChatSidebar {
    fn crew_is_open(&self, lead: i64, children: usize) -> bool {
        self.crew_open.get(&lead).copied().unwrap_or_else(|| !starts_collapsed(children))
    }

    fn chat_row(
        &self,
        mut chat: ChatItem,
        nested: bool,
        crew: Option<CrewSummary>,
        open: Option<bool>,
        members: usize,
        parent: Option<Provider>,
        carded: bool,
        git: bool,
    ) -> ChatRow {
        chat.now = self.now;
        let rename_editor =
            self.rename_ix.filter(|ix| *ix == chat.ix).and(self.rename_input.clone());
        ChatRow::Chat {
            chat,
            nested,
            rename_editor,
            style: self.style,
            crew,
            open,
            members,
            parent,
            tier: self.tier,
            carded,
            git,
        }
    }

    fn crew_rows(&self, chats: Vec<ChatItem>, nested: bool) -> Vec<ChatRow> {
        let mut out = Vec::new();
        for (head, rest) in families(chats) {
            self.family(head, rest, nested, None, false, &mut out);
        }
        out
    }

    fn family(
        &self,
        head: ChatItem,
        rest: Vec<ChatItem>,
        nested: bool,
        parent: Option<&ChatLine>,
        carded: bool,
        out: &mut Vec<ChatRow>,
    ) {
        let git = self.style == SidebarStyle::Default
            && head.child()
            && matches!((parent, &head.line), (Some(a), Some(b)) if git_differs(a, b));
        let parent = parent.map(|line| line.provider);
        if !head.lead || rest.is_empty() || head.pin {
            out.push(self.chat_row(head, nested, None, None, 0, parent, carded, git));
            out.extend(self.crew_rows(rest, nested));
            return;
        }
        let lead = head.id;
        let line = head.line.clone();
        let depth = head.depth.saturating_add(1).min(2);
        let summary = summarize(&rest);
        let members = families(rest);
        let open = self.crew_is_open(lead, members.len());
        let all = open && self.crew_all.contains(&lead);
        let mut crew: Vec<CrewMember> = members
            .iter()
            .enumerate()
            .map(|(ix, (chat, rest))| CrewMember {
                ix,
                status: chat.status,
                activity: chat.id,
                keep: std::iter::once(chat)
                    .chain(rest)
                    .any(|c| c.active || self.rename_ix == Some(c.ix)),
                summary: (chat.lead && !rest.is_empty()).then(|| summarize(rest)),
            })
            .collect();
        order(&mut crew);
        let shown = preview(&crew, open, all);
        out.push(self.chat_row(
            head,
            nested,
            Some(summary),
            Some(open),
            crew.len(),
            parent,
            carded,
            false,
        ));
        let mut members: Vec<_> = members.into_iter().map(Some).collect();
        let mut visible: Vec<usize> = crew
            .iter()
            .zip(&shown.visible)
            .filter(|(_, v)| **v)
            .map(|(member, _)| member.ix)
            .collect();
        let cards: Vec<Option<&(i64, SharedString)>> = visible
            .iter()
            .map(|&ix| {
                let grouped = all && self.style == SidebarStyle::Default;
                grouped
                    .then(|| members[ix].as_ref())
                    .flatten()
                    .and_then(|(chat, _)| self.cards.get(&chat.id))
            })
            .collect();
        let mut seen: Vec<i64> = Vec::new();
        let mut paired: Vec<_> = visible.iter().copied().zip(cards).collect();
        for (_, card) in &paired {
            if let Some((id, _)) = card
                && !seen.contains(id)
            {
                seen.push(*id);
            }
        }
        paired.sort_by_key(|(_, card)| {
            card.map_or(usize::MAX, |(id, _)| seen.iter().position(|s| s == id).unwrap_or(0))
        });
        let (ixs, cards): (Vec<usize>, Vec<_>) = paired.into_iter().unzip();
        visible = ixs;
        for (n, &ix) in visible.iter().enumerate() {
            let card = cards[n];
            if let Some((id, title)) = card
                && n.checked_sub(1).and_then(|p| cards[p]).is_none_or(|(prev, _)| prev != id)
            {
                out.push(ChatRow::CardHeader {
                    lead,
                    card: *id,
                    title: title.clone(),
                    count: cards[n..]
                        .iter()
                        .take_while(|c| c.is_some_and(|(i, _)| i == id))
                        .count(),
                    depth,
                    nested,
                });
            }
            if let Some((chat, rest)) = members[ix].take() {
                self.family(chat, rest, nested, line.as_ref(), card.is_some(), out);
            }
        }
        if open && (shown.hidden.total() > 0 || (all && crew.len() > 5)) {
            out.push(ChatRow::CrewMore {
                lead,
                hidden: shown.hidden,
                all,
                total: crew.len(),
                depth,
                nested,
                tier: self.tier,
            });
        }
    }

    fn tally_pinned(&mut self) {
        let mut tallies = HashMap::new();
        tally(&self.chats, &mut tallies);
        for project in &self.projects {
            tally(&project.chats, &mut tallies);
            for worktree in &project.worktrees {
                tally(&worktree.chats, &mut tallies);
            }
        }
        for (_, list) in &self.custom_sections {
            tally(list, &mut tallies);
        }
        let pins =
            self.pinned.iter_mut().chain(self.custom_sections.iter_mut().flat_map(|(_, l)| l));
        for chat in pins.filter(|c| c.pin && c.lead) {
            chat.tally = tallies.get(&chat.id).copied().filter(|s| s.total() > 0);
        }
    }

    /// Body rows in display order.
    pub(super) fn rows(&mut self) -> Vec<ChatRow> {
        let now = self.now;
        self.tally_pinned();
        let header = |title: &str| ChatRow::Header {
            tools: None,
            title: SharedString::from(title.to_string()),
            on_add: None,
            collapse: None,
        };
        let mut rows: Vec<ChatRow> = self
            .top
            .drain(..)
            .map(|b| ChatRow::SectionButton {
                icon: b.icon,
                aria: b.label.clone(),
                label: b.label,
                count: b.count,
                on_click: b.on_click,
            })
            .collect();
        if !self.agents.is_empty() {
            let collapsed = self.agents_collapse.as_ref().is_some_and(|(folded, _)| *folded);
            rows.push(ChatRow::Header {
                tools: None,
                title: "Agents".into(),
                on_add: None,
                collapse: self.agents_collapse.take(),
            });
            rows.extend(self.agents.drain(..).filter(|_| !collapsed).map(|a| ChatRow::Agent {
                ix: a.ix,
                title: a.title,
                provider: a.provider,
                detail: a.detail,
                project: a.project,
                state: a.state,
                on_click: a.on_click,
                on_stop: a.on_stop,
            }));
        }
        rows.extend(self.outside.drain(..).map(|a| ChatRow::Outside {
            line: a.line,
            on_click: a.on_click,
            on_stop: a.on_stop,
        }));
        if !self.pinned.is_empty() {
            let collapsed = self.pinned_collapse.as_ref().is_some_and(|(folded, _)| *folded);
            rows.push(ChatRow::Header {
                tools: None,
                title: "Pinned".into(),
                on_add: None,
                collapse: self.pinned_collapse.take(),
            });
            let pinned = std::mem::take(&mut self.pinned).into_iter().filter(|_| !collapsed);
            rows.extend(self.crew_rows(pinned.collect(), false));
        }
        let projects: Vec<ProjectItem> = std::mem::take(&mut self.projects)
            .into_iter()
            .filter(|project| project.header.count > 0)
            .collect();
        let projects_collapsed = self.projects_collapse.as_ref().is_some_and(|(folded, _)| *folded);
        if !projects.is_empty()
            || self.hidden.is_some()
            || self.on_add_project.is_some()
            || self.projects_collapse.is_some()
        {
            rows.push(ChatRow::Header {
                tools: None,
                title: "Projects".into(),
                on_add: self.on_add_project.take(),
                collapse: self.projects_collapse.take(),
            });
        }
        // Element ids stay unique across project and worktree headers.
        let mut id = 0;
        for (k, project) in projects.into_iter().filter(|_| !projects_collapsed).enumerate() {
            let collapsed = project.header.collapsed;
            // a folded group still shows the open chat, so the selection never vanishes
            let chats: Vec<ChatItem> =
                project.chats.into_iter().filter(|chat| !collapsed || chat.active).collect();
            let mut held: Vec<(WorktreeItem, Vec<ChatItem>)> = Vec::new();
            for mut worktree in project.worktrees {
                let folded = collapsed || worktree.header.collapsed;
                let chats: Vec<ChatItem> = std::mem::take(&mut worktree.chats)
                    .into_iter()
                    .filter(|chat| !folded || chat.active)
                    .collect();
                if !chats.is_empty() {
                    held.push((worktree, chats));
                }
            }
            rows.push(ChatRow::Project {
                id,
                header: project.header,
                spaced: k > 0,
                guide: !chats.is_empty() || !held.is_empty(),
                nested: false,
                on_collapse: project.on_collapse,
                on_new: project.on_new,
                on_files: project.on_files,
                on_hide: project.on_hide,
                on_page: project.on_page,
                now,
            });
            id += 1;
            rows.extend(self.crew_rows(chats, true));
            for (worktree, chats) in held {
                rows.push(ChatRow::Worktree {
                    id,
                    header: worktree.header,
                    on_collapse: worktree.on_collapse,
                    on_new: worktree.on_new,
                    now,
                });
                id += 1;
                rows.extend(self.crew_rows(chats, true));
            }
        }
        rows.extend(
            self.hidden
                .take()
                .filter(|_| !projects_collapsed)
                .map(|(label, on_click)| ChatRow::Link { label, on_click }),
        );
        if !self.chats.is_empty() {
            rows.push(header("Chats"));
            let chats = std::mem::take(&mut self.chats);
            rows.extend(self.crew_rows(chats, false));
        }
        let mut tools = self.section_tools.take();
        for (section_title, chats) in std::mem::take(&mut self.custom_sections) {
            if !chats.is_empty() {
                rows.push(ChatRow::Header {
                    title: section_title,
                    on_add: None,
                    collapse: None,
                    tools: tools.take(),
                });
                rows.extend(self.crew_rows(chats, false));
            }
        }
        if tools.is_some() {
            rows.push(ChatRow::Header {
                title: "Chats".into(),
                on_add: None,
                collapse: None,
                tools,
            });
        }
        let in_place = rows
            .iter()
            .any(|r| matches!(r, ChatRow::Chat { chat, rename_editor: Some(_), .. } if !chat.pin));
        if in_place {
            for row in &mut rows {
                if let ChatRow::Chat { chat, rename_editor, .. } = row
                    && chat.pin
                {
                    *rename_editor = None;
                }
            }
        }
        if rows.is_empty() {
            rows.extend(self.empty.take().map(|title| ChatRow::Header {
                tools: None,
                title,
                on_add: None,
                collapse: None,
            }));
        }
        rows
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/rows.rs"]
mod tests;
