//! Context panel: what fills the chat's context window, plus its MCP connectors and changes.

use std::collections::HashSet;
use std::path::PathBuf;

use aether_agent::inventory::{self, Inventory, Item, Status};
use aether_core::{
    ChatLayout, Message, NoticeKind, Provider, Step, ToolCall, ToolDetail, ToolStatus, home_dir,
};
use aether_ui::{format_tokens, icon_chip, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Icon, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Hsla, SharedString, Window, div, prelude::*, px, relative,
};

use crate::app::ChatApp;
use crate::side_panel::Surface;

const SHOWN_CONNECTORS: usize = 3;
const SHOWN_AGENTS: usize = 5;

/// Cached scan, keyed by the provider and folder it was read for.
pub(crate) type InventoryCache = Option<((Provider, Option<PathBuf>), Inventory)>;

pub(crate) const MESSAGES: &str = "Messages";
pub(crate) const BUFFER: &str = "Autocompact buffer";
pub(crate) const FREE: &str = "Free space";

fn tokens(items: &[Item]) -> u64 {
    items.iter().map(|i| i.tokens).sum()
}

/// Splits the window into parts: the CLI's exact fixed parts when it reported them, else
/// estimates from config files. Messages get what the fixed parts don't cover; before the
/// first reply `used` is 0, so the fixed parts stand in.
pub(crate) fn breakdown(used: u64, total: u64, inv: &Inventory) -> Vec<(String, u64)> {
    let fixed: Vec<(String, u64)> = if inv.exact() {
        inv.parts.clone()
    } else {
        [
            ("MCP tools", tokens(&inv.mcp)),
            ("Memory files", tokens(&inv.memory)),
            ("Skills", tokens(&inv.skills)),
            ("Custom agents", tokens(&inv.agents)),
        ]
        .into_iter()
        .map(|(name, n)| (name.to_string(), n))
        .collect()
    };
    let sum: u64 = fixed.iter().map(|p| p.1).sum();
    let used = used.max(sum);
    let messages = if inv.exact() { MESSAGES } else { "Messages & system" };
    let mut parts = vec![(messages.to_string(), used - sum)];
    parts.extend(fixed);
    if inv.buffer > 0 {
        parts.push((BUFFER.into(), inv.buffer));
    }
    parts.push((FREE.into(), total.saturating_sub(used + inv.buffer)));
    parts
}

fn part_color(ix: usize, name: &str, cx: &App) -> Hsla {
    if name == FREE || name == BUFFER {
        return cx.theme().muted;
    }
    let palette = [
        theme::info_text(cx),
        theme::mango_text(cx),
        theme::success_text(cx),
        theme::warning_text(cx),
        theme::danger_text(cx),
        cx.theme().primary,
        cx.theme().muted_foreground,
    ];
    palette[ix % palette.len()]
}

/// The connector row's state label and color, if it isn't plainly connected.
fn status_label(
    status: Option<Status>,
    signed_out: bool,
    cx: &App,
) -> Option<(&'static str, Hsla)> {
    let muted = cx.theme().muted_foreground;
    match status {
        _ if signed_out => Some(("Sign in needed", theme::mango_text(cx))),
        Some(Status::NeedsAuth) => Some(("Sign in needed", theme::mango_text(cx))),
        Some(Status::Failed) => Some(("Failed", theme::danger_text(cx))),
        Some(Status::Pending) => Some(("Connecting…", muted)),
        Some(Status::Disabled) => Some(("Off", muted)),
        Some(Status::Connected) | None => None,
    }
}

/// MCP servers the chat was told need a sign-in, by name; a later success clears one.
pub(crate) fn needs_sign_in(messages: &[Message]) -> HashSet<String> {
    let mut names = HashSet::new();
    let notices = messages.iter().flat_map(|m| &m.steps).filter_map(|s| match s {
        Step::Notice(n) if n.kind == NoticeKind::McpAuth => Some(n),
        _ => None,
    });
    for notice in notices {
        let Some(name) = notice.id.strip_prefix("mcp-") else { continue };
        if notice.text.starts_with("Signed in") {
            names.remove(name);
        } else {
            names.insert(name.to_string());
        }
    }
    names
}

/// Every subagent the chat spawned, live ones first, then newest first. Rows restored from a
/// past run can't still be working, whatever status they were saved with.
pub(crate) fn subagents(messages: &[Message]) -> Vec<(&ToolCall, bool)> {
    let mut rows: Vec<(&ToolCall, bool)> = messages
        .iter()
        .rev()
        .flat_map(|m| m.steps.iter().rev().map(move |s| (m.restored, s)))
        .filter_map(|(restored, s)| match s {
            Step::Tool(call) if matches!(call.detail, ToolDetail::Task { .. }) => {
                Some((call, is_live(call.status) && !restored))
            }
            _ => None,
        })
        .collect();
    rows.sort_by_key(|(_, live)| !live);
    rows
}

fn is_live(status: ToolStatus) -> bool {
    matches!(status, ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background)
}

fn status_text(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Running | ToolStatus::Waiting => "Running",
        ToolStatus::Background => "Background",
        ToolStatus::Done => "Done",
        ToolStatus::Failed => "Failed",
        ToolStatus::Stopped | ToolStatus::Denied | ToolStatus::Unknown => "Stopped",
    }
}

fn agent_state(label: &'static str, live: bool, cx: &App) -> impl IntoElement {
    let color = if live { theme::info_text(cx) } else { cx.theme().muted_foreground };
    div().text_xs().text_color(color).child(label)
}

fn heading(title: &'static str, cx: &App) -> impl IntoElement {
    div().text_xs().font_medium().text_color(cx.theme().muted_foreground).child(title)
}

fn line(icon: Lucide, label: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .gap_2()
        .items_center()
        .min_w_0()
        .text_sm()
        .child(Icon::new(icon).small().text_color(muted))
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
}

fn tip(text: String) -> impl Fn(&mut gpui_kit::Window, &mut App) -> gpui_kit::AnyView {
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx)
}

impl ChatApp {
    /// The active chat's inventory, rescanned when its provider or folder changed or `fresh`.
    /// Shows the disk scan at once, then asks the CLI in the background and swaps its answer in.
    pub(crate) fn context_inventory(&mut self, fresh: bool, cx: &mut Context<Self>) -> &Inventory {
        let chat = &self.chats[self.active];
        let key = (chat.provider, chat.folder(&self.settings.workspace_root));
        if fresh || self.inventory.as_ref().is_none_or(|(k, _)| *k != key) {
            let inv = inventory::scan(key.0, key.1.as_deref(), &home_dir(), &self.skills);
            self.inventory = Some((key.clone(), inv.clone()));
            let live = !cfg!(test)
                && matches!(key.0, Provider::Claude | Provider::Codex | Provider::OpenCode);
            if let (true, Some(cwd)) = (live, key.1.clone()) {
                self.probing = Some(key.clone());
                let settings = self.settings.clone();
                let provider = key.0;
                let task = cx.background_executor().spawn(async move {
                    let mut inv = inv;
                    aether_agent::probe::probe(provider, &settings, &cwd, &mut inv);
                    inv
                });
                cx.spawn(async move |this, cx| {
                    let inv = task.await;
                    this.update(cx, |this, cx| {
                        if this.inventory.as_ref().is_some_and(|(k, _)| *k == key) {
                            this.inventory = Some((key.clone(), inv));
                        }
                        if this.probing.as_ref() == Some(&key) {
                            this.probing = None;
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
        }
        &self.inventory.as_ref().expect("just filled").1
    }

    /// The Workspace panel shows: the Tabbed sidebar, or the right panel's Workspace tab.
    pub(crate) fn workspace_shown(&self) -> bool {
        self.side_open()
            && match self.settings.chat_layout {
                ChatLayout::Tabbed => self.workspace_open && !self.btw_open,
                layout => {
                    self.right_surface == Some(Surface::Workspace)
                        && !(layout == ChatLayout::Sidebar && self.settings.right_panel_collapsed)
                }
            }
    }

    pub(crate) fn open_workspace_panel(&mut self, cx: &mut Context<Self>) {
        self.workspace_pop.dismiss();
        self.load_workspace_info(cx);
        self.context_inventory(true, cx);
    }

    pub(crate) fn toggle_workspace_pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.workspace_shown();
        match self.settings.chat_layout {
            ChatLayout::Tabbed => {
                self.workspace_open = !shown;
                self.btw_open = false;
                if !shown {
                    self.open_workspace_panel(cx);
                }
            }
            ChatLayout::Sidebar if shown => self.toggle_right_panel(cx),
            _ => {
                if self.settings.right_panel_collapsed {
                    self.toggle_right_panel(cx);
                }
                self.select_surface(Surface::Workspace, window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn workspace_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.workspace_shown();
        let toggle =
            icon_chip("workspace-toggle", open, cx)
                .selected(open)
                .toggled(open)
                .icon(Lucide::LayoutList)
                .accessibility_label("Workspace")
                .tooltip("Workspace")
                .on_click(self.session_listener(cx, |this, _, window, cx| {
                    this.toggle_workspace_pop(window, cx)
                }));
        div().debug_selector(|| "workspace-toggle".into()).child(toggle).into_any_element()
    }

    pub(crate) fn render_context_panel(
        &mut self,
        controls: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let environment = self.render_workspace_section(window, cx);
        let inv = self.context_inventory(false, cx).clone();
        let provider = self.chats[self.active].provider;
        let note = if self.probing.is_some() {
            format!("Asking {}…", provider.label())
        } else if inv.exact() {
            format!("Reported by {}.", provider.label())
        } else {
            "Estimated from config files.".to_string()
        };
        let chat = &self.chats[self.active];
        let used = chat.context_used();
        let total = self.context_limit(chat).max(1);
        let parts = breakdown(used, total, &inv);
        let shown_used = total.saturating_sub(
            parts.iter().filter(|p| p.0 == FREE || p.0 == BUFFER).map(|p| p.1).sum(),
        );
        let percent = shown_used as f32 / total as f32 * 100.;
        let sign_in = needs_sign_in(&chat.messages);
        let muted = cx.theme().muted_foreground;

        let bar = h_flex()
            .w_full()
            .h(px(6.))
            .rounded(px(3.))
            .overflow_hidden()
            .bg(cx.theme().muted)
            .children(
                parts.iter().enumerate().filter(|(_, (name, n))| name != FREE && *n > 0).map(
                    |(ix, (name, n))| {
                        div()
                            .h_full()
                            .w(relative(*n as f32 / total as f32))
                            .bg(part_color(ix, name, cx))
                    },
                ),
            );
        let rows = parts
            .iter()
            .enumerate()
            .map(|(ix, (name, n))| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(
                        div().size(px(8.)).rounded(px(2.)).flex_none().bg(part_color(ix, name, cx)),
                    )
                    .child(div().flex_1().text_color(muted).child(name.clone()))
                    .child(div().child(format_tokens(*n)))
                    .child(
                        div()
                            .w(px(44.))
                            .text_right()
                            .text_color(muted)
                            .child(format!("{:.1}%", *n as f32 / total as f32 * 100.)),
                    )
            })
            .collect::<Vec<_>>();
        let lists = [
            ("Memory files", &inv.memory),
            ("Custom agents", &inv.agents),
            ("Skills", &inv.skills),
        ]
        .into_iter()
        .map(|(title, items)| self.context_list(title, items, cx))
        .collect::<Vec<_>>();
        let context = v_flex()
            .gap_2()
            .child(heading("Context", cx))
            .child(h_flex().justify_between().text_sm().child(format!("{percent:.0}% used")).child(
                div().text_color(muted).child(format!(
                    "{} / {}",
                    format_tokens(shown_used),
                    format_tokens(total)
                )),
            ))
            .child(bar)
            .children(rows)
            .child(div().text_xs().text_color(muted).child(note))
            .children(lists);

        let chat_id = self.chats[self.active].id;
        let messages = &self.chats[self.active].messages;
        let crew: Vec<(usize, bool)> = self
            .chats
            .iter()
            .enumerate()
            .filter(|(_, c)| c.parent == Some(chat_id) && !c.archived)
            .map(|(ix, c)| (ix, c.busy()))
            .collect();
        let mut rows: Vec<(bool, AnyElement)> = crew
            .into_iter()
            .map(|(ix, busy)| {
                let child = &self.chats[ix];
                let row = line(Lucide::Bot, child.title.clone(), cx)
                    .id(SharedString::from(format!("crew-agent-{}", child.id)))
                    .cursor_pointer()
                    .child(div().text_xs().text_color(muted).child(child.provider.label()))
                    .child(agent_state(if busy { "Running" } else { "Chat" }, busy, cx))
                    .on_click(cx.listener(move |this, _, window, cx| this.select(ix, window, cx)));
                (busy, row.into_any_element())
            })
            .collect();
        rows.extend(subagents(messages).into_iter().filter_map(|(call, live)| {
            let ToolDetail::Task { description, agent, tools, .. } = &call.detail else {
                return None;
            };
            let detail = match (agent.is_empty(), *tools) {
                (true, n) => format!("{n} tools"),
                (false, n) => format!("{agent} · {n} tools"),
            };
            let status =
                if !live && is_live(call.status) { "Stopped" } else { status_text(call.status) };
            let background = live && call.status == ToolStatus::Background;
            let row = line(Lucide::Bot, description.clone(), cx)
                .id(SharedString::from(format!("subagent-{}", call.id)))
                .debug_selector({
                    let id = format!("subagent-{}", call.id);
                    move || id
                })
                .child(div().text_xs().text_color(muted).child(detail))
                .child(agent_state(status, live, cx))
                .when(background, |row| {
                    row.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                        this.background_hidden.remove(&chat_id);
                        cx.notify();
                    }))
                });
            Some((live, row.into_any_element()))
        }));
        rows.sort_by_key(|(live, _)| !live);
        let total_agents = rows.len();
        let more = self.subagents_all;
        let running = v_flex()
            .gap_2()
            .child(heading("Subagents", cx))
            .when(rows.is_empty(), |list| {
                list.child(div().text_xs().text_color(muted).child("No subagents yet"))
            })
            .children(
                rows.into_iter()
                    .take(if more { usize::MAX } else { SHOWN_AGENTS })
                    .map(|(_, row)| row),
            )
            .when(total_agents > SHOWN_AGENTS, |list| {
                list.child(
                    aether_ui::button("subagents-more")
                        .ghost()
                        .xsmall()
                        .label(if more { "Show less" } else { "Show more" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.subagents_all = !this.subagents_all;
                            cx.notify();
                        })),
                )
            });

        let all = self.connectors_all;
        let connectors = v_flex()
            .gap_2()
            .child(heading("Connectors", cx))
            .when(inv.mcp.is_empty(), |list| {
                list.child(div().text_xs().text_color(muted).child("No MCP servers configured"))
            })
            .children(inv.mcp.iter().take(if all { usize::MAX } else { SHOWN_CONNECTORS }).map(
                |server| {
                    line(Lucide::Plug, server.name.clone(), cx)
                        .id(SharedString::from(format!("connector-{}", server.name)))
                        .tooltip(tip(server.path.display().to_string()))
                        .when(server.tokens > 0, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format_tokens(server.tokens)),
                            )
                        })
                        .when_some(
                            status_label(server.status, sign_in.contains(&server.name), cx),
                            |row, (label, color)| {
                                row.child(div().text_xs().text_color(color).child(label))
                            },
                        )
                },
            ))
            .when(inv.mcp.len() > SHOWN_CONNECTORS, |list| {
                list.child(
                    aether_ui::button("connectors-more")
                        .ghost()
                        .xsmall()
                        .label(if all { "Show less" } else { "Show more" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.connectors_all = !this.connectors_all;
                            cx.notify();
                        })),
                )
            });

        let body = v_flex()
            .id("context-scroll")
            .debug_selector(|| "workspace-panel".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_3()
            .py_3()
            .gap_5()
            .child(v_flex().gap_1().child(heading("Environment", cx)).child(environment))
            .child(context)
            .child(running)
            .child(connectors);

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .when(self.settings.chat_layout == ChatLayout::Tabbed, |panel| {
                let bar = h_flex()
                    .id("context-title-bar")
                    .flex_none()
                    .h(px(38.))
                    .px_3()
                    .items_center()
                    .text_sm()
                    .when(!controls, |bar| bar.border_b_1().border_color(cx.theme().sidebar_border))
                    .child("Workspace");
                panel.child(if controls {
                    self.window_bar(bar.into_any_element(), 38., false, true, cx)
                } else {
                    bar.into_any_element()
                })
            })
            .child(body)
            .into_any_element()
    }

    fn context_list(
        &self,
        title: &'static str,
        items: &[Item],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.context_expanded.contains(title);
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .id(title)
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .cursor_pointer()
                    .child(
                        Icon::new(if open { Lucide::ChevronDown } else { Lucide::ChevronRight })
                            .xsmall()
                            .text_color(muted),
                    )
                    .child(div().flex_1().child(title))
                    .child(div().text_color(muted).child(items.len().to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.context_expanded.remove(title) {
                            this.context_expanded.insert(title);
                        }
                        cx.notify();
                    })),
            )
            .when(open, |list| {
                list.children(items.iter().enumerate().map(|(ix, item)| {
                    h_flex()
                        .id(SharedString::from(format!("{title}-{ix}")))
                        .pl_4()
                        .gap_2()
                        .text_xs()
                        .tooltip(tip(item.path.display().to_string()))
                        .child(div().flex_1().min_w_0().truncate().child(item.name.clone()))
                        .when(item.tokens > 0, |row| {
                            row.child(div().text_color(muted).child(format_tokens(item.tokens)))
                        })
                }))
            })
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../tests/unit/context_panel.rs"]
mod tests;
