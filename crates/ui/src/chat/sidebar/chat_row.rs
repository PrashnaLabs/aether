use crate::theme::Corners;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    checkbox::Checkbox,
    h_flex,
    hover_card::HoverCard,
    input::InputState,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, ClickEvent, Div, Entity, FocusHandle, Hsla, KeyDownEvent, MouseButton,
    Pixels, Role, SharedString, Window, div, hsla, prelude::*, px, relative, rems,
};

use super::chrome::{reveal_on_hover, row_style, swallow_click, tabular, tooltip};
use super::crew::{Bucket, CrewSummary, Tier};
use super::menu::chat_menu;
use super::{ChatItem, ChatLine, ChatStatus, LONG_BRANCH, LONG_TITLE, Pick, RowHandler, RowPlace};
use crate::arrow_count;
use crate::components::{provider_icon, provider_tint};
use crate::theme::{danger_text, info_text, mango_text};
use aether_core::{Provider, SidebarStyle};

/// Keeps both ends of a long name, which is where branch names differ.
pub fn middle_truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let head = (max - 1) / 2;
    let tail = max - 1 - head;
    chars[..head].iter().chain(['…'].iter()).chain(&chars[chars.len() - tail..]).collect()
}

/// Two letters for a project mark: word initials, else the name's first two.
pub(super) fn initials(name: &str) -> String {
    let words: Vec<&str> =
        name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let letters: String = match words.as_slice() {
        [first, second, ..] => first.chars().take(1).chain(second.chars().take(1)).collect(),
        [only] => only.chars().take(2).collect(),
        [] => String::new(),
    };
    letters.to_uppercase()
}

/// Stable hue in `0..1` for a project name, so its mark keeps its tint.
pub(super) fn project_hue(name: &str) -> f32 {
    let name = name.trim().to_lowercase();
    match name.as_str() {
        "chrus" => return 145. / 360.,
        "aether" => return 215. / 360.,
        _ => {}
    }
    (project_hash(&name) >> 32) as f32 / 4_294_967_296.
}

/// FNV-1a: stable across toolchains, and its high bits spread short names over the wheel.
fn project_hash(name: &str) -> u64 {
    name.bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

pub fn project_mark(project: Option<&SharedString>, cx: &App) -> AnyElement {
    let Some(name) = project else {
        return div()
            .flex_none()
            .size(rems(0.875))
            .border_1()
            .border_dashed()
            .border_color(cx.theme().muted_foreground)
            .corner(px(3.))
            .into_any_element();
    };
    mark(name, cx)
        .size(rems(1.))
        .corner(px(4.))
        .text_size(rems(0.5625))
        .child(initials(name))
        .into_any_element()
}

pub fn project_mark_sized(name: &str, size: Pixels, cx: &App) -> AnyElement {
    mark(name, cx)
        .size(size)
        .rounded(size * 0.25)
        .text_size(size * 0.5625)
        .child(initials(name))
        .into_any_element()
}

/// Shared identity color for project badges, names, and folder icons.
pub fn project_color(name: &str, cx: &App) -> Hsla {
    let hash = project_hash(&name.trim().to_lowercase());
    let saturation = [0.55, 0.65, 0.75][(hash >> 8) as usize % 3];
    let shift = [-0.05, 0., 0.05][(hash >> 16) as usize % 3];
    let lightness = if cx.theme().is_dark() { 0.72 } else { 0.30 } + shift;
    hsla(project_hue(name), saturation, lightness, 1.)
}

fn mark(name: &str, cx: &App) -> Div {
    let tint = project_color(name, cx);
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .bg(tint.opacity(0.18))
        .text_color(tint)
        .font_bold()
}

/// Provider icon on a rounded square tinted with the provider's colour.
fn glyph_tile(provider: Provider, size: f32) -> Div {
    let tint = provider_tint(provider);
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(if size >= 16. { 5. } else { 4. }))
        .bg(tint.opacity(0.16))
        .child(provider_icon(provider).size(px(size * 0.72)).text_color(tint))
}

/// Up to three overlapping provider tiles: who works in a crew.
fn avatars(providers: &[Provider], cx: &App) -> AnyElement {
    let ring = cx.theme().sidebar;
    let shown = providers.len().min(3);
    div()
        .relative()
        .flex_none()
        .w(px(14. + 10. * shown.saturating_sub(1) as f32))
        .h(px(14.))
        .children(providers.iter().take(3).enumerate().map(|(i, &p)| {
            glyph_tile(p, 14.)
                .absolute()
                .top_0()
                .left(px(10. * i as f32))
                .rounded_full()
                .border_1()
                .border_color(ring)
                .bg(ring.blend(provider_tint(p).opacity(0.16)))
        }))
        .into_any_element()
}

/// Status in a 12 px slot; an idle child counts as done, so it gets a dim check.
pub(super) fn status_mark(status: ChatStatus, child: bool, cx: &App) -> AnyElement {
    let info = info_text(cx);
    let dot =
        |color: Hsla, size: f32| div().size(px(size)).rounded_full().bg(color).into_any_element();
    let icon = |name: Lucide, color: Hsla| {
        Icon::new(name).size(px(11.)).text_color(color).into_any_element()
    };
    let mark = match status {
        ChatStatus::Waiting => dot(mango_text(cx), 8.),
        ChatStatus::Failed => icon(Lucide::CircleX, danger_text(cx)),
        ChatStatus::Done => icon(Lucide::Check, info),
        ChatStatus::Idle if child => icon(Lucide::Check, info.opacity(0.6)),
        ChatStatus::Idle => dot(cx.theme().muted_foreground, 4.),
        ChatStatus::Quiet { .. } => {
            Spinner::new().xsmall().color(cx.theme().muted_foreground).into_any_element()
        }
        _ => Spinner::new().xsmall().color(cx.theme().foreground).into_any_element(),
    };
    div()
        .flex_none()
        .size(px(12.))
        .flex()
        .items_center()
        .justify_center()
        .child(mark)
        .into_any_element()
}

/// A status that draws the given bucket's mark.
fn bucket_status(bucket: Bucket) -> ChatStatus {
    match bucket {
        Bucket::NeedsYou => ChatStatus::Waiting,
        Bucket::Failed => ChatStatus::Failed,
        Bucket::Working => ChatStatus::Starting,
        Bucket::Done => ChatStatus::Idle,
    }
}

fn bucket_label(bucket: Bucket) -> &'static str {
    match bucket {
        Bucket::NeedsYou => "Needs you",
        Bucket::Failed => "Failed",
        Bucket::Working => "Working",
        Bucket::Done => "Done",
    }
}

fn background_chip(count: usize, ix: usize, cx: &App) -> Option<AnyElement> {
    (count > 0).then(|| {
        h_flex()
            .id(("chat-background", ix))
            .flex_none()
            .gap_0p5()
            .items_center()
            .text_color(crate::theme::info_text(cx))
            .font_features(tabular())
            .tooltip(tooltip(format!("{count} running in background").into()))
            .child(Icon::new(Lucide::Layers).size(rems(0.75)))
            .child(count.to_string())
            .into_any_element()
    })
}

/// How long a working chat has run, for rows that already show its spinner.
pub(super) fn run_clock(status: ChatStatus, now_ms: i64) -> Option<String> {
    match status {
        ChatStatus::Working { since_ms } => super::live::since(Some(since_ms), now_ms),
        _ => None,
    }
}

fn project_label(project: Option<&SharedString>) -> SharedString {
    project.cloned().unwrap_or_else(|| "No project".into())
}

/// Row label for screen readers; also carries the hover card's lines.
pub(super) fn aria(
    title: &str,
    status: ChatStatus,
    lead: bool,
    line: Option<&ChatLine>,
    crew: Option<(CrewSummary, bool, usize)>,
) -> String {
    if let Some((summary, open, members)) = crew {
        let mut aria = vec!["Lead".to_string(), title.to_string()];
        if status != ChatStatus::Idle {
            aria.push(status.label().into());
        }
        aria.push(format!("{members} spawned agents"));
        aria.push(summary.words(Tier::Wide));
        aria.push(if open { "expanded" } else { "collapsed" }.into());
        return aria.join(", ");
    }
    let mut aria = vec![title.to_string()];
    if status != ChatStatus::Idle {
        aria.push(status.label().into());
    }
    if lead {
        aria.push("Lead".into());
    }
    let Some(line) = line else {
        return aria.join(", ");
    };
    aria.push(project_label(line.project.as_ref()).into());
    aria.extend(line.place.as_ref().map(RowPlace::describe));
    aria.push(format!("{} {}", line.provider.label(), line.model));
    if line.removed {
        aria.push("worktree removed".into());
    }
    if line.changed > 0 {
        aria.push(format!("{} lines added, {} lines deleted", line.added, line.deleted));
    }
    match line.ahead_behind.unwrap_or((0, 0)) {
        (0, 0) => {}
        (ahead, 0) => aria.push(format!("{ahead} ahead")),
        (0, behind) => aria.push(format!("{behind} behind")),
        (ahead, behind) => aria.push(format!("{ahead} ahead, {behind} behind")),
    }
    if !line.time.is_empty() {
        aria.push(line.time.to_string());
    }
    aria.join(", ")
}

pub(super) fn rollup(summary: CrewSummary, tier: Tier, cx: &App) -> AnyElement {
    let color = |bucket: Bucket| match bucket {
        Bucket::NeedsYou => mango_text(cx),
        Bucket::Failed => danger_text(cx),
        Bucket::Working => cx.theme().foreground,
        Bucket::Done => cx.theme().muted_foreground,
    };
    let glyph = |bucket: Bucket| {
        let icon = |name: Lucide, color: Hsla| {
            Icon::new(name).size(rems(0.75)).text_color(color).into_any_element()
        };
        match bucket {
            Bucket::NeedsYou => icon(Lucide::CircleDot, color(bucket)),
            Bucket::Failed => icon(Lucide::CircleX, color(bucket)),
            Bucket::Working => Spinner::new().xsmall().color(color(bucket)).into_any_element(),
            Bucket::Done => icon(Lucide::Check, info_text(cx)),
        }
    };
    let words = tier != Tier::Narrow;
    h_flex()
        .min_w_0()
        .overflow_hidden()
        .gap_2()
        .items_center()
        .text_xs()
        .font_features(tabular())
        .children(summary.parts(tier).into_iter().map(|(bucket, n)| {
            h_flex()
                .flex_none()
                .gap_1()
                .items_center()
                .text_color(color(bucket))
                .when(bucket == Bucket::NeedsYou, |el| el.font_semibold())
                .child(glyph(bucket))
                .child(if words { format!("{n} {}", bucket.word()) } else { n.to_string() })
        }))
        .into_any_element()
}

pub(super) fn status_bar(summary: CrewSummary, cx: &App) -> AnyElement {
    let total = summary.total().max(1) as f32;
    let segment = |n: usize, color: Hsla| {
        (n > 0).then(|| div().h_full().min_w(px(2.)).w(relative(n as f32 / total)).bg(color))
    };
    h_flex()
        .w_full()
        .h(px(3.))
        .gap(px(1.))
        .rounded(px(2.))
        .overflow_hidden()
        .bg(cx.theme().foreground.opacity(0.05))
        .children(segment(summary.done, info_text(cx).opacity(0.55)))
        .children(segment(summary.working, cx.theme().foreground.opacity(0.7)))
        .children(segment(summary.needs_you, mango_text(cx)))
        .children(segment(summary.failed, danger_text(cx)))
        .into_any_element()
}

/// What the hover card shows beyond the row's line.
#[derive(Clone, Copy)]
struct CardState {
    status: ChatStatus,
    now: i64,
    crew: Option<CrewSummary>,
}

fn with_card(
    row: impl IntoElement + 'static,
    ix: usize,
    title: SharedString,
    line: Option<ChatLine>,
    state: CardState,
) -> AnyElement {
    match line {
        Some(line) => HoverCard::new(("chat-card", ix))
            .anchor(Anchor::LeftCenter)
            .ml_2()
            .trigger(row)
            .content(move |_, _, cx| card(title.clone(), line.clone(), state, cx))
            .into_any_element(),
        None => row.into_any_element(),
    }
}

/// The row's trailing time or icon, giving its place to the actions on hover or while
/// the menu is open, so no row keeps an empty actions gap.
fn hover_swap(content: impl IntoElement, actions: Div, group: &SharedString, show: bool) -> Div {
    div()
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .min_w(rems(1.25))
        .h(rems(1.25))
        .child(
            div()
                .flex()
                .items_center()
                .when(show, |d| d.opacity(0.))
                .when(!show, |d| d.group_hover(group.clone(), |s| s.opacity(0.)))
                .child(content),
        )
        .child(div().absolute().top_0().bottom_0().right_0().flex().items_center().child(actions))
}

/// A row's place text; worktrees show only their icon, the hover card names them.
fn place_label(place: &RowPlace) -> Option<String> {
    match place {
        RowPlace::Branch { worktree: true, .. } => None,
        RowPlace::Branch { name, .. } => Some(middle_truncate(name, LONG_BRANCH)),
        RowPlace::Folder(name) => Some(name.to_string()),
        RowPlace::NoGit => Some("No git".to_string()),
    }
}

fn swap(mark: AnyElement, chevron: Option<AnyElement>) -> AnyElement {
    div()
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(rems(1.))
        .child(mark)
        .children(chevron)
        .into_any_element()
}

fn card(
    title: SharedString,
    line: ChatLine,
    state: CardState,
    cx: &App,
) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    let entry = |icon: AnyElement, text: SharedString| {
        h_flex()
            .gap_2()
            .items_center()
            .text_color(muted)
            .child(icon)
            .child(div().min_w_0().truncate().child(text))
    };
    let small = |icon: Icon| icon.size(rems(0.875)).flex_none().into_any_element();
    let gone = line.removed;
    let struck = |icon: AnyElement, text: SharedString| {
        h_flex()
            .gap_2()
            .items_center()
            .text_color(muted)
            .child(icon)
            .child(div().min_w_0().truncate().when(gone, |d| d.line_through()).child(text))
    };
    let worktree = matches!(line.place, Some(RowPlace::Branch { worktree: true, .. }));
    let place = line.place.as_ref().map(|place| {
        let text = match place {
            RowPlace::Branch { name, .. } => name.clone(),
            RowPlace::Folder(_) => line.path.clone(),
            RowPlace::NoGit => "Not a git repository".into(),
        };
        struck(small(Icon::new(place.icon())), text)
    });
    let tree_path = worktree.then(|| {
        struck(small(Icon::new(Lucide::Folder)), super::project_row::tilde_path(&line.path).into())
    });
    let tree_gone = (worktree && gone).then(|| {
        entry(small(Icon::new(Lucide::TriangleAlert)), "Worktree removed or unavailable".into())
    });
    let thinking = match line.effort.is_empty() {
        true => "Default thinking".into(),
        false => format!("{} thinking", capitalize(&line.effort)).into(),
    };
    let activity: Option<SharedString> = match state.status {
        ChatStatus::Idle | ChatStatus::Background(_) if line.time.is_empty() => None,
        ChatStatus::Idle | ChatStatus::Background(_) => {
            Some(format!("Last active {}", line.time).into())
        }
        status => Some(status.text(state.now).into()),
    };
    let changes = (line.changed > 0).then(|| {
        let files = if line.changed == 1 { "file" } else { "files" };
        let mut text = format!("+{} −{} in {} {files}", line.added, line.deleted, line.changed);
        if let Some((ahead, behind)) = line.ahead_behind.filter(|&(a, b)| a + b > 0) {
            text.push_str(&format!(" · {ahead}↑ {behind}↓"));
        }
        entry(small(Icon::new(Lucide::FilePen)), text.into())
    });
    let crew = state.crew.filter(|c| c.total() > 0).map(|summary| {
        let agents = if summary.total() == 1 {
            "1 agent".into()
        } else {
            format!("{} agents", summary.total())
        };
        h_flex()
            .gap_2()
            .items_center()
            .text_color(muted)
            .child(small(Icon::new(Lucide::Users)))
            .child(agents)
            .child(rollup(summary, Tier::Wide, cx))
    });
    v_flex()
        .max_w(rems(22.))
        .gap_1p5()
        .text_sm()
        .child(div().font_semibold().child(title))
        .children(activity.map(|text| entry(small(Icon::new(Lucide::Clock)), text)))
        .child(entry(project_mark(line.project.as_ref(), cx), project_label(line.project.as_ref())))
        .children(place)
        .children(tree_path)
        .children(tree_gone)
        .children(changes)
        .child(entry(small(provider_icon(line.provider)), line.model.clone()))
        .child(entry(small(Icon::new(Lucide::Gauge)), thinking))
        .children(crew)
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The row's leading mark; with bulk select on, a checkbox takes its place on hover
/// or while any row is picked.
fn pick_slot(
    mark: AnyElement,
    pick: Option<&Pick>,
    group: &SharedString,
    id: i64,
    title: &SharedString,
) -> AnyElement {
    let Some(pick) = pick else { return mark };
    let shown = pick.picked || pick.picking;
    let toggle = pick.on_toggle.clone();
    div()
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .min_w(rems(1.))
        .child(
            div()
                .when(shown, |d| d.opacity(0.))
                .when(!shown, |d| d.group_hover(group.clone(), |s| s.opacity(0.)))
                .child(mark),
        )
        .child(
            div()
                .id(("chat-pick-slot", id as u64))
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .when(!shown, |d| d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    Checkbox::new(("chat-pick", id as u64))
                        .checked(pick.picked)
                        .tab_stop(false)
                        .aria_label(SharedString::from(format!("Select {title}")))
                        .on_click(move |_: &bool, window, cx| toggle(id, window, cx)),
                ),
        )
        .into_any_element()
}

pub(super) fn render_chat(
    chat: ChatItem,
    nested: bool,
    focus: Option<FocusHandle>,
    show_menu: bool,
    on_menu: impl Fn(&bool, &mut Window, &mut App) + 'static,
    nav: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    group: SharedString,
    rename: Option<Entity<InputState>>,
    style: SidebarStyle,
    chevron: Option<AnyElement>,
    crew: Option<(CrewSummary, bool, usize)>,
    show_all: Option<(SharedString, RowHandler)>,
    tier: Tier,
    git: bool,
    big: bool,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ix = chat.ix;
    let title = chat.title.clone();
    let (pick, pick_group, pick_id) = (chat.pick.clone(), group.clone(), chat.id);
    let slot = |mark: AnyElement| pick_slot(mark, pick.as_ref(), &pick_group, pick_id, &title);
    let status = chat.status;
    let now = chat.now;
    let child = chat.child();
    let aria = aria(&title, status, chat.lead, chat.line.as_ref().filter(|_| !child), crew);
    let removed = chat.line.as_ref().is_some_and(|line| line.removed);
    let menu = {
        let (chat, show_all) = (chat.clone(), show_all.clone());
        move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
            chat_menu(menu, &chat, show_all.clone(), window, cx)
        }
    };
    let context = {
        let chat = chat.clone();
        move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
            chat_menu(menu, &chat, show_all.clone(), window, cx)
        }
    };
    let actions = reveal_on_hover(div().flex_none().size(rems(1.25)), group.clone(), show_menu)
        .child(swallow_click(
            ("chat-menu-slot", ix),
            crate::components::button(("chat-menu", ix))
                .ghost()
                .xsmall()
                .icon(IconName::Ellipsis)
                .accessibility_label(format!("Actions for {title}"))
                .tooltip("Chat actions")
                .dropdown_menu_with_anchor(Anchor::TopRight, menu)
                .on_open_change(on_menu),
        ));
    let line = chat.line.clone();
    let users = chat.lead && crew.is_none() && !child && line.is_none();
    let tip = match line {
        Some(_) => None,
        None => (title.chars().count() > LONG_TITLE).then(|| title.clone()),
    };
    // Renaming swaps only the title for the field; the rest of the row stays put.
    let title_cell = |title: SharedString| -> AnyElement {
        match rename {
            Some(editor) => div()
                .flex_1()
                .min_w_0()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(crate::components::field(&editor).small())
                .into_any_element(),
            None => {
                let title = div()
                    .id(("chat-title", ix))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(removed, |el| el.text_color(muted))
                    .when(crew.is_some() && !child, |el| el.font_semibold())
                    .when(child && style != SidebarStyle::Focus, |el| {
                        el.flex_auto().min_w(rems(2.25))
                    })
                    .when_some(tip, |el, tip| el.tooltip(tooltip(tip)))
                    .child(title);
                if !users {
                    return title.into_any_element();
                }
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .id(("chat-lead", ix))
                            .debug_selector(|| format!("chat-lead-{ix}"))
                            .flex_none()
                            .tooltip(tooltip("Lead".into()))
                            .child(Icon::new(Lucide::Users).size(rems(0.75)).text_color(muted)),
                    )
                    .child(title)
                    .into_any_element()
            }
        }
    };
    let (on_click, on_rename) = (chat.on_click.clone(), chat.on_rename.clone());
    let row = row_style(
        v_flex()
            .id(("chat", ix))
            .when_some(focus, |el, focus| el.track_focus(&focus))
            .role(Role::Button)
            .aria_label(aria),
        chat.active,
        cx,
    )
    .group(group.clone())
    .w_full()
    .px_2()
    .py_1()
    .text_sm()
    .on_click(move |ev, window, cx| on_click(ev, window, cx))
    .on_key_down(move |ev, window, cx| {
        if ev.keystroke.key == "f2" && !ev.keystroke.modifiers.modified() {
            cx.stop_propagation();
            let click = ClickEvent::default();
            return on_rename(&click, window, cx);
        }
        nav(ev, window, cx)
    });
    let approve_action = (status == ChatStatus::Waiting).then(|| {
        let on_click = chat.on_click.clone();
        crate::components::button(("chat-approve", ix))
            .primary()
            .xsmall()
            .label("Approve ↵")
            .accessibility_label("Approve pending action")
            .tooltip("Approve and jump to chat")
            .on_click(move |ev, window, cx| {
                cx.stop_propagation();
                on_click(ev, window, cx);
            })
    });
    let card_line = line.clone();
    let state = CardState { status, now, crew: crew.map(|(summary, ..)| summary) };
    let time = line.as_ref().map(|line| line.time.clone()).unwrap_or_default();
    let time_text = |time: SharedString| {
        div().flex_none().text_xs().text_color(muted).font_features(tabular()).child(time)
    };
    if child && style != SidebarStyle::Focus {
        let mark = status_mark(status, true, cx);
        let name = title.clone();
        let chip = line.as_ref().filter(|_| git && tier != Tier::Narrow).and_then(|line| {
            let place = line.place.as_ref().map(|place| {
                h_flex()
                    .min_w_0()
                    .gap_0p5()
                    .items_center()
                    .child(Icon::new(place.icon()).size(rems(0.75)).flex_none())
                    .children(place_label(place).map(|s| div().min_w_0().truncate().child(s)))
            });
            let diff = line.changed > 0 && (line.added > 0 || line.deleted > 0);
            (place.is_some() || diff).then(|| {
                h_flex()
                    .id(("chat-child-git", ix))
                    .min_w_0()
                    .max_w(rems(9.))
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .font_features(tabular())
                    .children(place)
                    .when(diff, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_color(crate::theme::success_text(cx))
                                .child(format!("+{}", line.added)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(crate::theme::danger_text(cx))
                                .child(format!("−{}", line.deleted)),
                        )
                    })
            })
        });
        let lead_icon = match line.as_ref() {
            Some(line) => glyph_tile(line.provider, 14.).into_any_element(),
            None => Icon::new(Lucide::MessageCircle)
                .size(rems(0.75))
                .text_color(muted)
                .into_any_element(),
        };
        let one = h_flex()
            .w_full()
            .gap_1p5()
            .items_center()
            .child(slot(swap(lead_icon, chevron)))
            .child(title_cell(name))
            .children(chip)
            .child(mark)
            .children(crew.map(|(summary, ..)| rollup(summary, Tier::Narrow, cx)))
            .when(tier == Tier::Wide, |el| el.children(approve_action))
            .child(hover_swap(
                div().when(tier != Tier::Narrow && !time.is_empty(), |d| d.child(time_text(time))),
                actions,
                &group,
                show_menu,
            ));
        let stub = (style == SidebarStyle::Minimal).then(|| {
            div()
                .absolute()
                .left(px(-6.))
                .top(relative(0.5))
                .w(px(6.))
                .h(px(1.))
                .bg(cx.theme().sidebar_border.opacity(0.6))
        });
        let row = row.relative().py(px(1.)).text_size(px(12.)).children(stub).child(one);
        return with_card(row.context_menu(context), ix, title, card_line, state);
    }
    if let Some((summary, ..)) = crew {
        let lead_status = match summary.worst() {
            Some(bucket) if bucket < Bucket::of(status) => bucket_status(bucket),
            _ => status,
        };
        let icon = match (style, line.as_ref()) {
            (SidebarStyle::Focus, _) => status_mark(lead_status, false, cx),
            (SidebarStyle::Default, Some(line)) => {
                glyph_tile(line.provider, 16.).into_any_element()
            }
            (_, Some(line)) => provider_icon(line.provider).size(rems(0.875)).into_any_element(),
            (_, None) => {
                Icon::new(Lucide::Users).size(rems(0.875)).text_color(muted).into_any_element()
            }
        };
        if style == SidebarStyle::Focus {
            let one = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(slot(swap(icon, chevron)))
                .child(title_cell(title.clone()))
                .children(run_clock(status, now).map(|clock| time_text(clock.into())))
                .child(
                    div()
                        .flex_none()
                        .debug_selector(move || format!("chat-rollup-{}", chat.id))
                        .child(rollup(summary, Tier::Narrow, cx)),
                )
                .children(approve_action)
                .child(match line.as_ref() {
                    Some(l) => hover_swap(
                        provider_icon(l.provider).size(rems(0.8125)),
                        actions,
                        &group,
                        show_menu,
                    ),
                    None => actions,
                });
            let row = row.py_0p5().child(one).context_menu(context);
            return with_card(row, ix, title, card_line, state);
        }
        let idle = matches!(status, ChatStatus::Idle | ChatStatus::Background(_));
        if style == SidebarStyle::Minimal {
            let top = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(title_cell(title.clone()))
                .children((!idle).then(|| status.badge(("chat-status-badge", ix), Some(now), cx)))
                .children(approve_action);
            let folder = Icon::new(Lucide::Folder)
                .size(rems(0.75))
                .flex_none()
                .when_some(line.as_ref().and_then(|l| l.project.as_ref()), |icon, name| {
                    icon.text_color(project_color(name, cx))
                })
                .into_any_element();
            let meta = h_flex()
                .w_full()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(muted)
                .child(slot(swap(folder, chevron)))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(project_label(line.as_ref().and_then(|l| l.project.as_ref()))),
                )
                .when(!chat.providers.is_empty(), |el| el.child(avatars(&chat.providers, cx)))
                .child(rollup(summary, tier, cx))
                .child(div().flex_1())
                .child(Icon::new(Lucide::Users).size(rems(0.75)).flex_none())
                .when(idle && !time.is_empty(), |el| {
                    el.child(div().flex_none().font_features(tabular()).child(time.clone()))
                })
                .child(match line.as_ref() {
                    Some(l) => hover_swap(
                        provider_icon(l.provider).size(rems(0.8125)),
                        actions,
                        &group,
                        show_menu,
                    ),
                    None => actions,
                });
            let row = row.gap_0p5().child(top).child(meta).context_menu(context);
            return with_card(row, ix, title, card_line, state);
        }
        let trailing = match idle {
            true => time_text(time).into_any_element(),
            false => status.badge(("chat-status-badge", ix), Some(now), cx),
        };
        let top = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(slot(swap(icon, chevron)))
            .child(Icon::new(Lucide::Users).size(rems(0.75)).text_color(muted).flex_none())
            .child(title_cell(title.clone()))
            .children(approve_action)
            .child(hover_swap(trailing, actions, &group, show_menu));
        if !big {
            return with_card(row.child(top).context_menu(context), ix, title, card_line, state);
        }
        let second = h_flex()
            .pl(rems(1.5))
            .gap_1p5()
            .items_center()
            .when(!chat.providers.is_empty(), |el| el.child(avatars(&chat.providers, cx)))
            .child(rollup(summary, tier, cx));
        let row = row
            .gap_0p5()
            .child(top)
            .child(second)
            .when(style == SidebarStyle::Default, |row| {
                row.child(div().pl(rems(1.5)).pt_1().pb_0p5().child(status_bar(summary, cx)))
            })
            .context_menu(context);
        return with_card(row, ix, title, card_line, state);
    }
    let Some(line) = line else {
        let title_row = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(slot(
                Icon::new(Lucide::MessageCircle)
                    .size(rems(0.875))
                    .text_color(muted)
                    .flex_none()
                    .into_any_element(),
            ))
            .child(title_cell(title.clone()))
            .when(status != ChatStatus::Idle, |row| {
                row.child(status.badge(("chat-status", ix), None, cx)).child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .font_features(tabular())
                        .child(status.text(now)),
                )
            })
            .children(approve_action)
            .child(actions);
        return row.child(title_row).context_menu(context).into_any_element();
    };
    let tally = chat.tally.map(|summary| rollup(summary, Tier::Narrow, cx));
    let background = background_chip(line.background, ix, cx);
    // The chip carries the background count, so the badge would repeat it.
    let status = match status {
        ChatStatus::Background(_) if style != SidebarStyle::Focus => ChatStatus::Idle,
        _ => status,
    };
    let badge = (status != ChatStatus::Idle)
        .then(|| status.badge(("chat-status-badge", ix), Some(now), cx));
    match style {
        SidebarStyle::Minimal => {
            let top = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(title_cell(title.clone()))
                .children(badge)
                .children(approve_action);
            let meta = h_flex()
                .w_full()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(muted)
                .child(slot(
                    Icon::new(Lucide::Folder)
                        .size(rems(0.75))
                        .flex_none()
                        .when_some(line.project.as_ref(), |icon, name| {
                            icon.text_color(project_color(name, cx))
                        })
                        .into_any_element(),
                ))
                .child(
                    div().flex_1().min_w_0().truncate().child(project_label(line.project.as_ref())),
                )
                .when(
                    status == ChatStatus::Idle && !line.time.is_empty() && tally.is_none(),
                    |el| {
                        el.child(
                            div().flex_none().font_features(tabular()).child(line.time.clone()),
                        )
                    },
                )
                .children(tally)
                .children(background)
                .child(hover_swap(
                    provider_icon(line.provider).size(rems(0.8125)),
                    actions,
                    &group,
                    show_menu,
                ));
            let row = row.gap_0p5().child(top).child(meta).context_menu(context);
            return with_card(row, ix, title, Some(line), state);
        }
        SidebarStyle::Focus => {
            let words = if child {
                bucket_label(Bucket::of(status)).into()
            } else if status == ChatStatus::Idle {
                line.time.clone()
            } else {
                status.text(now).into()
            };
            let trailing = div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .font_features(tabular())
                .child(words)
                .into_any_element();
            let one = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(slot(status_mark(status, child, cx)))
                .child(title_cell(title.clone()))
                .child(tally.unwrap_or(trailing))
                .children(approve_action)
                .child(hover_swap(
                    provider_icon(line.provider).size(rems(0.8125)),
                    actions,
                    &group,
                    show_menu,
                ));
            let row = row.py_0p5().child(one).context_menu(context);
            return with_card(row, ix, title, Some(line), state);
        }
        SidebarStyle::Default => {}
    }
    let trailing =
        tally.or(badge).unwrap_or_else(|| time_text(line.time.clone()).into_any_element());
    let one = h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(slot(glyph_tile(line.provider, 14.).into_any_element()))
        .child(title_cell(title.clone()))
        .children(background)
        .children(approve_action)
        .child(hover_swap(trailing, actions, &group, show_menu));
    let own_place =
        line.removed || matches!(line.place, Some(RowPlace::Branch { worktree: true, .. }));
    if !(nested && own_place) {
        let row = row.child(one).context_menu(context);
        return with_card(row, ix, title, Some(line), state);
    }
    let clickable_chip = line.status_clickable();
    let clickable = clickable_chip.then(|| chat.on_status.clone()).flatten();
    let (ahead, behind) = line.ahead_behind.unwrap_or((0, 0));
    let chip_label: SharedString = {
        let mut label =
            format!("Show changes, {} lines added, {} lines deleted", line.added, line.deleted);
        if ahead > 0 {
            label.push_str(&format!(", {ahead} ahead"));
        }
        if behind > 0 {
            label.push_str(&format!(", {behind} behind"));
        }
        label.into()
    };
    let place = line.place.as_ref().map(|place| {
        h_flex()
            .min_w_0()
            .gap_1()
            .items_center()
            .child(Icon::new(place.icon()).size(rems(0.75)).flex_none())
            .children(place_label(place).map(|s| div().min_w_0().truncate().child(s)))
            .when(line.removed, |el| el.child(div().flex_none().child("removed")))
    });
    let git = clickable_chip.then(|| {
        let chips = h_flex()
            .gap_1()
            .items_center()
            .font_features(tabular())
            .when(line.changed > 0, |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_color(crate::theme::success_text(cx))
                                .child(format!("+{}", line.added)),
                        )
                        .child(
                            div()
                                .text_color(crate::theme::danger_text(cx))
                                .child(format!("−{}", line.deleted)),
                        ),
                )
            })
            .when(ahead > 0, |el| el.child(arrow_count(Lucide::ArrowUp, ahead)))
            .when(behind > 0, |el| el.child(arrow_count(Lucide::ArrowDown, behind)));
        match clickable {
            Some(open) => div()
                .id(("chat-status", ix))
                .flex_none()
                .cursor_pointer()
                .role(Role::Button)
                .aria_label(chip_label)
                .tooltip(tooltip("Show changes".into()))
                .child(chips)
                .on_click(move |ev, window, cx| {
                    cx.stop_propagation();
                    open(ev, window, cx)
                })
                .into_any_element(),
            None => div().flex_none().child(chips).into_any_element(),
        }
    });
    let bottom = h_flex()
        .w_full()
        .pl(rems(1.5))
        .gap_1p5()
        .items_center()
        .text_xs()
        .text_color(muted)
        .children(place)
        .children(git);
    let row = row.gap_0p5().child(one).child(bottom).context_menu(context);
    with_card(row, ix, title, Some(line), state)
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/chat_row.rs"]
mod tests;
