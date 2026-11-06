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
use super::chrome::{nest, reveal_on_hover, row_style, tabular, tooltip};
use super::menu::track_menu;
use super::project_row::ProjectRow;
use super::state::{SidebarState, navigate};
use super::{
    AgentState, ChatItem, ChatSidebar, CollapseHandler, OutsideLine, OutsideState, ProjectHeader,
    ProjectItem, RowHandler, ToolsFactory, WorktreeItem,
};
use crate::components::provider_icon;
use aether_core::{Provider, SidebarStyle};

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
    },
    CrewToggle {
        lead: i64,
        hidden: usize,
        expanded: bool,
        nested: bool,
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
            Self::Chat { chat, .. } => chat_key(chat.id),
            Self::CrewToggle { lead, .. } => format!("crew-toggle:{lead}").into(),
            Self::SectionButton { label, .. } => format!("section-btn:{label}").into(),
        }
    }

    pub(super) fn focusable(&self) -> bool {
        match self {
            Self::Header { collapse, .. } => collapse.is_some(),
            Self::Chat { rename_editor, .. } => rename_editor.is_none(),
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
            Self::Chat { chat, nested, rename_editor, style } => {
                let menu_open = state.read(cx).open_menu.as_ref() == Some(&key);
                let keyboard = focus.as_ref().is_some_and(|f| f.contains_focused(window, cx))
                    && window.last_input_was_keyboard();
                let show = chat.active || keyboard || menu_open;
                let child = chat.child;
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
                    cx,
                );
                let row = nest(row, nested, cx);
                if child { nest(row, true, cx) } else { row }
            }
            Self::CrewToggle { lead, hidden, expanded, nested } => {
                let label =
                    if expanded { "Show less".to_string() } else { format!("+{hidden} more") };
                let toggle = {
                    let state = state.clone();
                    move |expanded: bool, window: &mut Window, cx: &mut App| {
                        state.update(cx, |s, cx| {
                            if expanded {
                                s.collapsed_crews.remove(&lead);
                            } else {
                                s.collapsed_crews.insert(lead);
                            }
                            cx.notify();
                        });
                        window.refresh();
                    }
                };
                let click = toggle.clone();
                let row = row_style(
                    h_flex()
                        .id("crew-toggle")
                        .debug_selector(move || format!("crew-toggle-{lead}"))
                        .role(Role::Button)
                        .aria_label(if expanded {
                            "Show fewer spawned agents".to_string()
                        } else {
                            format!("Show {hidden} more spawned agents")
                        })
                        .aria_expanded(expanded)
                        .when_some(focus, |el, focus| el.track_focus(&focus)),
                    false,
                    cx,
                )
                .w_full()
                .px_2()
                .py_1p5()
                .gap_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    Icon::new(if expanded { IconName::ChevronUp } else { IconName::ChevronDown })
                        .xsmall(),
                )
                .child(label)
                .on_click(move |_, window, cx| click(!expanded, window, cx))
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let next = match event.keystroke.key.as_str() {
                        "enter" | "space" => !expanded,
                        "left" => false,
                        "right" => true,
                        _ => return nav(event, window, cx),
                    };
                    cx.stop_propagation();
                    toggle(next, window, cx);
                });
                nest(nest(row.into_any_element(), nested, cx), true, cx)
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

pub(super) fn chat_key(id: i64) -> SharedString {
    format!("chat:{id}").into()
}

/// Keep ongoing work and the selected/renamed chat visible, then fill the three
/// preview slots with the newest children. Running crews may exceed three rows.
fn collapsed_crew(children: &[ChatItem], rename_ix: Option<usize>) -> Vec<bool> {
    let mut visible: Vec<bool> = children
        .iter()
        .map(|chat| {
            chat.status.is_busy()
                || matches!(
                    chat.status,
                    super::ChatStatus::Quiet { .. } | super::ChatStatus::Background(_)
                )
                || chat.active
                || rename_ix == Some(chat.ix)
        })
        .collect();
    let mut slots = 3usize.saturating_sub(visible.iter().filter(|&&show| show).count());
    for show in visible.iter_mut().rev() {
        if slots == 0 {
            break;
        }
        if !*show {
            *show = true;
            slots -= 1;
        }
    }
    visible
}

impl ChatSidebar {
    /// Body rows in display order.
    pub(super) fn rows(&mut self) -> Vec<ChatRow> {
        let rename = self.rename_ix.zip(self.rename_input.clone());
        let now = self.now;
        let style = self.style;
        let row = |mut chat: ChatItem, nested: bool| {
            chat.now = now;
            let rename_editor =
                rename.clone().filter(|(ix, _)| *ix == chat.ix).map(|(_, input)| input);
            ChatRow::Chat { chat, nested, rename_editor, style }
        };
        let collapsed_crews = &self.collapsed_crews;
        let rename_ix = self.rename_ix;
        let crew_rows = |chats: Vec<ChatItem>, nested: bool| {
            let mut out = Vec::new();
            let mut chats = chats.into_iter().peekable();
            while let Some(chat) = chats.next() {
                let lead = chat.id;
                let crew = style != SidebarStyle::Focus && chat.lead;
                out.push(row(chat, nested));
                if !crew {
                    continue;
                }
                let mut children = Vec::new();
                while chats.peek().is_some_and(|chat| chat.child) {
                    children.push(chats.next().unwrap());
                }
                let visible = collapsed_crew(&children, rename_ix);
                let hidden = visible.iter().filter(|&&show| !show).count();
                let expanded = !collapsed_crews.contains(&lead);
                out.extend(
                    children
                        .into_iter()
                        .zip(visible)
                        .filter_map(|(chat, show)| (expanded || show).then(|| row(chat, nested))),
                );
                if hidden > 0 {
                    out.push(ChatRow::CrewToggle { lead, hidden, expanded, nested });
                }
            }
            out
        };
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
            rows.extend(crew_rows(
                std::mem::take(&mut self.pinned).into_iter().filter(|_| !collapsed).collect(),
                false,
            ));
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
            rows.extend(crew_rows(chats, true));
            for (worktree, chats) in held {
                rows.push(ChatRow::Worktree {
                    id,
                    header: worktree.header,
                    on_collapse: worktree.on_collapse,
                    on_new: worktree.on_new,
                    now,
                });
                id += 1;
                rows.extend(crew_rows(chats, true));
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
            rows.extend(crew_rows(std::mem::take(&mut self.chats), false));
        }
        let mut tools = self.section_tools.take();
        for (section_title, chats) in self.custom_sections.drain(..) {
            if !chats.is_empty() {
                rows.push(ChatRow::Header {
                    title: section_title,
                    on_add: None,
                    collapse: None,
                    tools: tools.take(),
                });
                rows.extend(crew_rows(chats, false));
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
