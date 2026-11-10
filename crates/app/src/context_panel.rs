//! Context panel: what fills the chat's context window, plus its MCP connectors and changes.

use std::collections::HashSet;
use std::path::PathBuf;

use aether_agent::inventory::{self, Inventory, Item};
use aether_core::{ChatLayout, Message, NoticeKind, Provider, Step, home_dir};
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

/// Cached scan, keyed by the provider and folder it was read for.
pub(crate) type InventoryCache = Option<((Provider, Option<PathBuf>), Inventory)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Messages,
    Memory,
    Skills,
    Agents,
    Free,
}

impl Part {
    fn label(self) -> &'static str {
        match self {
            Self::Messages => "Messages & system",
            Self::Memory => "Memory files",
            Self::Skills => "Skills",
            Self::Agents => "Custom agents",
            Self::Free => "Free space",
        }
    }

    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Messages => theme::info_text(cx),
            Self::Memory => theme::mango_text(cx),
            Self::Skills => theme::success_text(cx),
            Self::Agents => theme::warning_text(cx),
            Self::Free => cx.theme().muted,
        }
    }
}

fn tokens(items: &[Item]) -> u64 {
    items.iter().map(|i| i.tokens).sum()
}

/// Splits the window into parts. Before the first reply `used` is 0, so the loaded files'
/// estimate stands in; after it, messages get whatever the estimates don't cover.
pub(crate) fn breakdown(used: u64, total: u64, inv: &Inventory) -> Vec<(Part, u64)> {
    let (memory, skills, agents) = (tokens(&inv.memory), tokens(&inv.skills), tokens(&inv.agents));
    let used = used.max(memory + skills + agents);
    vec![
        (Part::Messages, used - memory - skills - agents),
        (Part::Memory, memory),
        (Part::Skills, skills),
        (Part::Agents, agents),
        (Part::Free, total.saturating_sub(used)),
    ]
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
    pub(crate) fn context_inventory(&mut self, fresh: bool) -> &Inventory {
        let chat = &self.chats[self.active];
        let key = (chat.provider, chat.folder(&self.settings.workspace_root));
        if fresh || self.inventory.as_ref().is_none_or(|(k, _)| *k != key) {
            let inv = inventory::scan(key.0, key.1.as_deref(), &home_dir(), &self.skills);
            self.inventory = Some((key, inv));
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
        self.context_inventory(true);
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
        let inv = self.context_inventory(false).clone();
        let chat = &self.chats[self.active];
        let used = chat.context_used();
        let total = self.context_limit(chat).max(1);
        let parts = breakdown(used, total, &inv);
        let shown_used = total - parts.last().map_or(0, |p| p.1).min(total);
        let percent = shown_used as f32 / total as f32 * 100.;
        let sign_in = needs_sign_in(&chat.messages);
        let muted = cx.theme().muted_foreground;

        let bar = h_flex()
            .w_full()
            .h(px(6.))
            .rounded(px(3.))
            .overflow_hidden()
            .bg(cx.theme().muted)
            .children(parts.iter().filter(|(part, n)| *part != Part::Free && *n > 0).map(
                |(part, n)| div().h_full().w(relative(*n as f32 / total as f32)).bg(part.color(cx)),
            ));
        let rows = parts
            .iter()
            .map(|(part, n)| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(div().size(px(8.)).rounded(px(2.)).flex_none().bg(part.color(cx)))
                    .child(div().flex_1().text_color(muted).child(part.label()))
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
            .child(
                div().text_xs().text_color(muted).child(
                    "Files and skills are estimates. MCP tool schemas count under messages.",
                ),
            )
            .children(lists);

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
                        .when(sign_in.contains(&server.name), |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(theme::mango_text(cx))
                                    .child("Sign in needed"),
                            )
                        })
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
