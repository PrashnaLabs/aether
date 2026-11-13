use aether_ui::ChatStatus;
use std::path::Path;

use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, Hsla, Role, SharedString, Window, div, prelude::*, px,
    rgb,
};

use gpui_kit::assets::IconName;

use crate::actions::ToggleSidebar;
use crate::app::{Chat, ChatApp, Page, display_path, now_ms};
use crate::board::model::Col;
use crate::org::model::{Decision, Head, NeedsYou, Org, role_badge, role_color};
use crate::org::{OrgOverlay, model::HireSpec};
use crate::project_page::view::pill;

const BLUE: u32 = 0x58a6ff;
const AMBER: u32 = 0xf5a524;
const RED: u32 = 0xf85149;
const GREEN: u32 = 0x3fb950;

/// First non-empty line of a reply, for a one-line "doing now".
pub(crate) fn activity_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string()
}

pub(crate) fn money(usd: f64) -> String {
    format!("${usd:.2}")
}

/// Pass or fail pill for a hiring rule.
pub(crate) fn check_pill(text: &str, ok: bool) -> impl IntoElement {
    let (mark, color) = if ok { ("✓", GREEN) } else { ("✗", RED) };
    let fg: Hsla = rgb(color).into();
    pill(format!("{mark} {text}"), fg.opacity(0.14), fg)
}

/// Shows an org operation's error as a notification.
pub(crate) fn report<T>(result: Result<T, String>, window: &mut Window, cx: &mut App) {
    if let Err(e) = result {
        window.push_notification(Notification::error(e), cx);
    }
}

/// Role badge in the role's colour; `size` 22 small, 30 normal, 44 large.
pub(crate) fn avatar(role: &str, size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.27))
        .bg(rgb(role_color(role)))
        .text_color(rgb(avatar_foreground(role_color(role))))
        .text_size(px(size * 0.34))
        .font_weight(FontWeight::BOLD)
        .child(role_badge(role))
}

/// WCAG relative luminance, after converting sRGB channels to linear light.
fn luminance(color: u32) -> f64 {
    let linear = |shift: u32| {
        let channel = f64::from((color >> shift) & 0xff_u32) / 255.;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0)
}

/// Choose whichever of black or white has the greater measured contrast.
fn avatar_foreground(background: u32) -> u32 {
    let light = luminance(background);
    if (light + 0.05) / 0.05 >= 1.05 / (light + 0.05) { 0x000000 } else { 0xffffff }
}

fn status_visual(status: ChatStatus) -> (IconName, String, Option<u32>) {
    match status {
        ChatStatus::Idle => (IconName::Circle, "Idle".into(), None),
        ChatStatus::Starting => (IconName::LoaderCircle, "Starting".into(), Some(BLUE)),
        ChatStatus::Working { .. } => (IconName::Play, "Working".into(), Some(BLUE)),
        ChatStatus::Waiting => (IconName::CircleAlert, "Needs you".into(), Some(AMBER)),
        ChatStatus::Retrying => (IconName::RefreshCw, "Retrying".into(), Some(AMBER)),
        ChatStatus::Quiet { .. } => (IconName::Pause, "No output".into(), Some(BLUE)),
        ChatStatus::Background(n) => (IconName::Layers, format!("{n} tasks in background"), None),
        ChatStatus::Done => (IconName::CircleCheck, "New reply".into(), None),
        ChatStatus::Failed => (IconName::CircleX, "Failed".into(), Some(RED)),
    }
}

/// Stable, clock-free status name shared by roster row/cell and icon semantics.
pub(super) fn status_label(status: ChatStatus) -> String {
    status_visual(status).1
}

/// A fixed-size, static status icon for the org roster and chart.
pub(crate) fn status_dot(status: ChatStatus, cx: &App) -> impl IntoElement {
    let (icon, label, color) = status_visual(status);
    let color = color.map_or(cx.theme().muted_foreground, |c| rgb(c).into());
    div()
        .id("org-status")
        .debug_selector(|| "org-status".into())
        .role(Role::Image)
        .aria_label(label)
        .size(px(12.))
        .flex_none()
        .child(Icon::new(icon).size(px(12.)).text_color(color))
}

/// Why Hire worker can't open: workers report to a head other than the CEO.
fn hire_blocked(org: &Org) -> Option<&'static str> {
    org.heads
        .iter()
        .all(|h| h.role == "CEO")
        .then_some("Add a head first; workers report to a head")
}

/// Busy heads and workers in this org only; archived chats no longer belong to its live roster.
fn org_busy_counts(org: &Org, chats: &[Chat]) -> (usize, usize) {
    chats.iter().filter(|chat| !chat.archived && chat.busy()).fold(
        (0, 0),
        |(heads, workers), chat| {
            if org.head_of_chat(chat.id).is_some() {
                (heads + 1, workers)
            } else if chat.parent.is_some_and(|parent| org.head_of_chat(parent).is_some()) {
                (heads, workers + 1)
            } else {
                (heads, workers)
            }
        },
    )
}

fn org_header_status(org: &Org, chats: &[Chat], cx: &App) -> impl IntoElement {
    let (heads, workers) = org_busy_counts(org, chats);
    let busy = heads + workers;
    // org_pause stops turns; there is no persisted paused state to display.
    let label = if busy == 0 { "idle".into() } else { format!("{busy} busy") };
    let worker_word = if workers == 1 { "worker" } else { "workers" };
    let head_word = if heads == 1 { "head" } else { "heads" };
    let description = format!("{workers} {worker_word} busy · {heads} {head_word} busy");
    let (icon, color) = if busy == 0 {
        (IconName::Circle, cx.theme().muted_foreground)
    } else {
        (IconName::LoaderCircle, aether_ui::theme::success_text(cx))
    };
    h_flex()
        .id("org-header-status")
        .debug_selector(|| "org-header-status".into())
        .role(Role::Status)
        .aria_label(format!("Organization {label}"))
        .aria_description(description.clone())
        .aria_value(label.clone())
        .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
        .gap(px(5.))
        .px(px(7.))
        .rounded(px(5.))
        .text_size(px(11.))
        .text_color(color)
        .child(Icon::new(icon).size(px(12.)).flex_none())
        .child(label)
}

fn bar(fraction: f64, fill: Hsla, cx: &App) -> impl IntoElement {
    div()
        .h(px(4.))
        .w_full()
        .rounded(px(2.))
        .overflow_hidden()
        .bg(cx.theme().border)
        .child(div().h_full().w(gpui_kit::relative(fraction.clamp(0., 1.) as f32)).bg(fill))
}

/// Context used; amber once at the rotate threshold.
pub(crate) fn context_bar(percent: u8, rotate_at: u8, cx: &App) -> impl IntoElement {
    let fill = if percent >= rotate_at { AMBER } else { BLUE };
    bar(f64::from(percent) / 100., rgb(fill).into(), cx)
}

impl ChatApp {
    pub(crate) fn org_head_status(&self, head: &Head) -> ChatStatus {
        let chat = head.chat.and_then(|id| self.chats.iter().find(|c| c.id == id));
        chat.map_or(ChatStatus::Idle, |c| c.status(now_ms()))
    }

    /// Percent of the head chat's context window in use.
    pub(crate) fn org_head_context(&self, head: &Head) -> u8 {
        let Some(chat) = head.chat.and_then(|id| self.chats.iter().find(|c| c.id == id)) else {
            return 0;
        };
        let limit = self.context_limit(chat).max(1);
        (chat.context_used() * 100 / limit).min(100) as u8
    }

    /// Title, project, current org activity and the page buttons; `None` org shows Settings only.
    pub(crate) fn org_header(
        &self,
        org: Option<&Org>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.org_header_with(org, None, window, cx)
    }

    /// `org_header` with `extra` (a layout's own control) before the buttons.
    pub(crate) fn org_header_with(
        &self,
        org: Option<&Org>,
        extra: Option<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lights = aether_ui::traffic_lights_gap(self.settings.sidebar_collapsed, window, cx);
        let narrow = window.viewport_size().width < px(1100.);
        let expand = self.settings.sidebar_collapsed.then(|| {
            aether_ui::button("expand-sidebar")
                .ghost()
                .small()
                .icon(IconName::PanelLeftOpen)
                .accessibility_label("Expand sidebar")
                .tooltip_with_action("Expand sidebar", &ToggleSidebar, None)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))
        });
        let muted = cx.theme().muted_foreground;
        let project = self.org_project().map(|dir| {
            let path = display_path(&dir, &aether_core::home_dir());
            div()
                .id("org-project")
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(muted)
                .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                .child(self.project_name(Path::new(&dir)))
        });
        let status = org.map(|org| org_header_status(org, &self.chats, cx));
        let buttons = org.map(|org| {
            let blocked = hire_blocked(org);
            let idle = org_busy_counts(org, &self.chats) == (0, 0);
            h_flex()
                .flex_none()
                .gap(px(6.))
                .items_center()
                .child(
                    aether_ui::button("org-hire")
                        .primary()
                        .small()
                        .icon(IconName::UserPlus)
                        .when(!narrow, |b| b.label("Hire worker"))
                        .accessibility_label("Hire worker")
                        .tooltip(blocked.unwrap_or("Hire worker"))
                        .disabled(blocked.is_some())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.org_open_hire(None);
                            cx.notify();
                        })),
                )
                .child(
                    aether_ui::button("org-add-head")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .when(!narrow, |b| b.label("Add head"))
                        .accessibility_label("Add head")
                        .tooltip("Add head")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.org.overlay = OrgOverlay::HeadEditor(String::new());
                            cx.notify();
                        })),
                )
                .child(
                    aether_ui::button("org-pause")
                        .ghost()
                        .small()
                        .icon(IconName::Pause)
                        .when(!narrow, |b| b.label("Pause"))
                        .accessibility_label("Pause organization")
                        .tooltip(if idle { "Nothing is running" } else { "Pause organization" })
                        .disabled(idle)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.org_confirm_pause(window, cx)),
                        ),
                )
        });
        h_flex()
            .flex_none()
            .h(px(36.))
            .px(px(16.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(lights)
            .children(expand)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .id("org-title")
                            .role(Role::Heading)
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Organization"),
                    )
                    .children(project)
                    .children(status),
            )
            .children(extra)
            .children(buttons)
            .child(
                aether_ui::button("org-settings")
                    .ghost()
                    .small()
                    .icon(IconName::Settings)
                    .accessibility_label("Organization settings")
                    .tooltip("Organization settings")
                    .on_click(cx.listener(|this, _, _, cx| this.org_open_settings(cx))),
            )
            .child(aether_ui::window_controls_gap(window))
    }

    fn org_confirm_pause(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.org_project().map(|dir| self.project_name(Path::new(&dir)));
        let text = format!(
            "Stops every running head and worker turn in {}. Queued messages stay queued; nothing is deleted.",
            project.unwrap_or_default()
        );
        let view = cx.entity();
        aether_ui::theme::open_alert(window, cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title("Pause the organization?")
                .description(SharedString::from(text.clone()))
                .ok_text("Pause")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancel")
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| this.org_pause(cx));
                    true
                })
        });
    }

    /// Goal input, target (CEO or a head) and Send.
    pub(crate) fn org_goal_row(&self, org: &Org, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.org.goal_target.clone();
        let label = match target.as_deref().and_then(|r| org.head(r)) {
            Some(h) => format!("directly to {}", h.title),
            None => "to CEO".into(),
        };
        let heads: Vec<(String, String)> = org
            .heads
            .iter()
            .filter(|h| h.role != "CEO")
            .map(|h| (h.role.clone(), h.title.clone()))
            .collect();
        let app = cx.entity();
        let pick = aether_ui::button("org-goal-target")
            .small()
            .ghost()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let options = std::iter::once((None, "to CEO".to_string())).chain(
                    heads.iter().map(|(r, t)| (Some(r.clone()), format!("directly to {t}"))),
                );
                for (role, text) in options {
                    let app = app.clone();
                    let checked = role == target;
                    menu = menu.item(PopupMenuItem::new(text).checked(checked).on_click(
                        move |_, _, cx| {
                            app.update(cx, |this, cx| {
                                this.org.goal_target = role.clone();
                                cx.notify();
                            })
                        },
                    ));
                }
                menu
            });
        let empty = self.org.goal.read(cx).value().trim().is_empty();
        div().w_full().px(px(20.)).pt(px(8.)).pb(px(14.)).child(
            div().w_full().max_w(px(860.)).mx_auto().child(
                aether_ui::Composer::new(&self.org.goal).leading(pick).action(
                    aether_ui::SendButton::new().disabled(empty).on_send(
                        cx.listener(|this, _, window, cx| this.org_goal_submit(window, cx)),
                    ),
                ),
            ),
        )
    }

    pub(crate) fn org_goal_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.org.goal.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let target = self.org.goal_target.clone();
        let sent = self.org_send_goal(text, target, window, cx);
        if sent.is_ok() {
            self.org.goal.update(cx, |s, cx| s.set_value("", window, cx));
        }
        report(sent, window, cx);
    }

    /// Hire requests, waiting chats and cards in review, each with its actions.
    pub(crate) fn org_needs_you_list(&self, org: &Org, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.org_needs_you();
        if items.is_empty() {
            return v_flex()
                .py(px(20.))
                .items_center()
                .text_color(cx.theme().muted_foreground)
                .child("Nothing needs you.");
        }
        v_flex().gap(px(8.)).children(
            items
                .into_iter()
                .enumerate()
                .map(|(ix, item)| self.org_needs_you_item(org, ix, item, cx)),
        )
    }

    fn org_needs_you_item(
        &self,
        org: &Org,
        ix: usize,
        item: NeedsYou,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, panel) = (theme.muted_foreground, theme.border, theme.secondary);
        let open_chat = |chat: i64| {
            aether_ui::button(("org-ny-chat", ix)).small().label("Open chat").on_click(
                cx.listener(move |this, _, window, cx| this.org_open_chat(chat, window, cx)),
            )
        };
        let open_card = |card: i64| {
            aether_ui::button(("org-ny-card", ix)).small().label("Open card").on_click(
                cx.listener(move |this, _, window, cx| this.org_open_card(card, window, cx)),
            )
        };
        let (role, title, why, checks, actions): (String, String, String, Vec<_>, Vec<AnyElement>) =
            match item {
                NeedsYou::Hire(id) => {
                    let Some(r) = org.requests.iter().find(|r| r.id == id) else {
                        return div().into_any_element();
                    };
                    let decide = |label: &'static str, key: &'static str, how: Decision| {
                        aether_ui::button((key, ix)).small().label(label).on_click(cx.listener(
                            move |this, _, window, cx| {
                                let done = this.org_decide(id, how, window, cx);
                                report(done, window, cx);
                            },
                        ))
                    };
                    let mut actions = vec![
                        decide("Approve", "org-ny-approve", Decision::Approve)
                            .primary()
                            .into_any_element(),
                        decide("Queue", "org-ny-queue", Decision::Queue).into_any_element(),
                        decide("Deny", "org-ny-deny", Decision::Deny).danger().into_any_element(),
                    ];
                    actions.extend(r.card.map(|c| open_card(c).into_any_element()));
                    let why = format!("{} · {} / {}", r.why, r.provider.key(), r.model);
                    (r.head.clone(), r.title.clone(), why, r.checks.clone(), actions)
                }
                NeedsYou::Waiting { chat } => {
                    let c = self.chats.iter().find(|c| c.id == chat);
                    let role = c.and_then(|c| org.head_of_chat(c.parent.unwrap_or(c.id)));
                    (
                        role.map(|h| h.role.clone()).unwrap_or_default(),
                        c.map_or(format!("Chat {chat}"), |c| c.title.to_string()),
                        format!(
                            "Waiting for you to {}",
                            c.map_or("answer", |c| c.wait_label()).to_lowercase()
                        ),
                        vec![],
                        vec![open_chat(chat).into_any_element()],
                    )
                }
                NeedsYou::Review { card } => {
                    let title = self.cards.iter().find(|c| c.id == card).map(|c| &c.title);
                    (
                        "CEO".into(),
                        format!("#{card} {}", title.map_or("", |t| t.as_str())),
                        "In review, waiting for you to mark it Done".into(),
                        vec![],
                        vec![open_card(card).into_any_element()],
                    )
                }
            };
        v_flex()
            .gap(px(6.))
            .px(px(12.))
            .py(px(10.))
            .rounded(px(10.))
            .border_1()
            .border_color(border)
            .bg(panel)
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(avatar(&role, 22.))
                    .child(div().min_w_0().child(title)),
            )
            .child(div().text_size(px(12.5)).text_color(muted).child(why))
            .when(!checks.is_empty(), |d| {
                d.child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .children(checks.iter().map(|(t, ok)| check_pill(t, *ok))),
                )
            })
            .child(h_flex().flex_wrap().gap(px(6.)).children(actions))
            .into_any_element()
    }

    /// One-line "doing now": the wait kind while it waits on you, else its latest reply.
    pub(crate) fn org_chat_doing(&self, chat: &Chat) -> String {
        if matches!(chat.status(now_ms()), ChatStatus::Waiting) {
            return format!("Waiting for you to {}", chat.wait_label().to_lowercase());
        }
        let reply = chat
            .messages
            .iter()
            .rev()
            .filter(|m| m.role == aether_core::Role::Assistant)
            .map(|m| activity_line(&m.text))
            .find(|l| !l.is_empty());
        reply.unwrap_or_else(|| if chat.busy() { "Working".into() } else { "Idle".into() })
    }

    pub(crate) fn org_head_doing(&self, head: &Head) -> String {
        match head.chat.and_then(|id| self.chats.iter().find(|c| c.id == id)) {
            Some(chat) => self.org_chat_doing(chat),
            None => "No chat yet".into(),
        }
    }

    /// Live worker chats under the head chat `chat`.
    pub(crate) fn org_workers_of(&self, chat: Option<i64>) -> Vec<&Chat> {
        self.chats
            .iter()
            .filter(|c| c.parent.is_some() && c.parent == chat && !c.archived)
            .collect()
    }

    /// Open cards linked to `chat`.
    pub(crate) fn org_chat_cards(&self, chat: i64) -> Vec<i64> {
        self.cards
            .iter()
            .filter(|c| c.col != Col::Done && c.chats.contains(&chat))
            .map(|c| c.id)
            .collect()
    }

    pub(crate) fn org_open_chat(&mut self, chat: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.board_open_chat(chat, window, cx);
    }

    pub(crate) fn org_open_card(&mut self, card: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.open_page(Page::Board, cx);
        self.board_pick(card, window, cx);
    }

    /// Hire dialog for `head`, else the first head that isn't the CEO.
    pub(crate) fn org_open_hire(&mut self, head: Option<String>) {
        let head = head.or_else(|| {
            let org = self.org_current()?;
            let first = org.heads.iter().find(|h| h.role != "CEO").or(org.heads.first());
            first.map(|h| h.role.clone())
        });
        let installed: Vec<_> = crate::app::CLIS
            .into_iter()
            .filter(|p| self.probes.get(p).is_none_or(|probe| probe.installed))
            .collect();
        let (provider, model) =
            crate::org::view::dialogs::hire_models(&installed).into_iter().next().unzip();
        let draft = HireSpec {
            head: head.unwrap_or_default(),
            provider: provider.unwrap_or_default(),
            model: model.map(str::to_string).unwrap_or_default(),
            worktree: true,
            ..HireSpec::default()
        };
        self.org.overlay = OrgOverlay::Hire(draft);
    }

    pub(crate) fn org_open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_tab = crate::settings_view::SettingsTab::Organization;
        self.open_page(Page::Settings, cx);
    }

    /// Read-only summary of the org settings with a button to change them.
    pub(crate) fn org_rules_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        let muted = cx.theme().muted_foreground;
        let pool = if s.org_pool.is_some() { "set" } else { "auto" };
        let rows: [(&str, String); 6] = [
            ("Hiring", s.org_hire.label().into()),
            ("Worker pool", format!("{} at once ({pool})", self.org_pool_cap())),
            ("Builds", format!("{} at a time", s.org_builds)),
            ("Per head", format!("{} workers", s.org_workers_per_head)),
            ("Rotate session", format!("at {}% context", s.org_rotate_at)),
            ("Done", "Only you".into()),
        ];
        v_flex()
            .gap(px(8.))
            .text_size(px(12.5))
            .children(rows.into_iter().map(|(k, v)| {
                h_flex()
                    .gap(px(8.))
                    .child(div().w(px(110.)).flex_none().text_color(muted).child(k))
                    .child(div().min_w_0().child(v))
            }))
            .child(
                div().pt(px(6.)).child(
                    aether_ui::button("org-rules-settings")
                        .small()
                        .label("Change in Settings")
                        .on_click(cx.listener(|this, _, _, cx| this.org_open_settings(cx))),
                ),
            )
    }

    /// Recent messages between org chats, newest first, at most `limit`.
    pub(crate) fn org_message_log(
        &self,
        org: &Org,
        limit: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let name = |chat: i64| {
            org.head_of_chat(chat).map(|h| h.role.clone()).unwrap_or_else(|| {
                self.chats
                    .iter()
                    .find(|c| c.id == chat)
                    .map_or(format!("#{chat}"), |c| c.title.to_string())
            })
        };
        let messages = self.org_messages(org);
        if messages.is_empty() {
            return v_flex().py(px(20.)).items_center().text_color(muted).child("No messages yet.");
        }
        v_flex().gap(px(10.)).children(messages.into_iter().take(limit).map(|m| {
            v_flex()
                .gap(px(3.))
                .text_size(px(12.5))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .text_size(px(11.5))
                        .text_color(muted)
                        .child(format!("{} → {}", name(m.from), name(m.to))),
                )
                .child(m.text)
        }))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/org/shared.rs"]
mod tests;
