use crate::theme::Corners;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt,
    checkbox::Checkbox,
    button::ButtonVariants,
    h_flex,
    hover_card::HoverCard,
    input::InputState,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu},
    v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, ClickEvent, Div, Entity, FocusHandle, Hsla, KeyDownEvent, MouseButton,
    Pixels, Role, SharedString, Window, div, hsla, prelude::*, px, rems,
};

use super::chrome::{reveal_on_hover, row_style, swallow_click, tabular, tooltip};
use super::menu::chat_menu;
use super::{ChatItem, ChatLine, ChatStatus, LONG_BRANCH, LONG_TITLE, Pick, RowPlace};
use aether_core::SidebarStyle;
use crate::arrow_count;
use crate::components::provider_icon;

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

fn project_label(project: Option<&SharedString>) -> SharedString {
    project.cloned().unwrap_or_else(|| "No project".into())
}

/// Row label for screen readers; also carries the hover card's lines.
pub(super) fn aria(title: &str, status: ChatStatus, lead: bool, line: Option<&ChatLine>) -> String {
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

fn card(title: SharedString, line: ChatLine, cx: &App) -> impl IntoElement + use<> {
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
    let place = line.place.as_ref().map(|place| {
        let text = match place {
            RowPlace::Branch { name, .. } => name.clone(),
            RowPlace::Folder(_) => line.path.clone(),
            RowPlace::NoGit => "Not a git repository".into(),
        };
        entry(small(Icon::new(place.icon())), text)
    });
    v_flex()
        .max_w(rems(20.))
        .gap_1p5()
        .text_sm()
        .child(div().font_semibold().child(title))
        .child(entry(project_mark(line.project.as_ref(), cx), project_label(line.project.as_ref())))
        .children(place)
        .child(entry(small(provider_icon(line.provider)), line.model.clone()))
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
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ix = chat.ix;
    let title = chat.title.clone();
    let (pick, pick_group, pick_id) = (chat.pick.clone(), group.clone(), chat.id);
    let slot = |mark: AnyElement| pick_slot(mark, pick.as_ref(), &pick_group, pick_id, &title);
    let status = chat.status;
    let now = chat.now;
    let aria = aria(&title, status, chat.lead, chat.line.as_ref());
    let removed = chat.line.as_ref().is_some_and(|line| line.removed);
    let menu = {
        let chat = chat.clone();
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| chat_menu(menu, &chat)
    };
    let context = {
        let chat = chat.clone();
        move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| chat_menu(menu, &chat)
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
    let lead = chat.lead;
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
                    .when_some(tip, |el, tip| el.tooltip(tooltip(tip)))
                    .child(title);
                if !lead {
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
    .group(group)
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
    let Some(line) = line else {
        let title_row = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                slot(
                    Icon::new(Lucide::MessageCircle)
                        .size(rems(0.875))
                        .text_color(muted)
                        .flex_none()
                        .into_any_element(),
                ),
            )
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
                .children(approve_action)
                .child(actions);
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
                .when(status == ChatStatus::Idle && !line.time.is_empty(), |el| {
                    el.child(div().flex_none().font_features(tabular()).child(line.time.clone()))
                })
                .children(background)
                .child(provider_icon(line.provider).size(rems(0.8125)).flex_none());
            return row.gap_0p5().child(top).child(meta).context_menu(context).into_any_element();
        }
        SidebarStyle::Focus => {
            let trailing = badge.unwrap_or_else(|| {
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .font_features(tabular())
                    .child(line.time.clone())
                    .into_any_element()
            });
            let one = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(slot(project_mark(line.project.as_ref(), cx)))
                .child(title_cell(title.clone()))
                .child(trailing)
                .children(approve_action)
                .child(actions);
            return row.child(one).context_menu(context).into_any_element();
        }
        SidebarStyle::Default => {}
    }
    let corner = line.time.clone();
    let (top, title_line) = if nested {
        // Nested in project folder: omit top project label line, keep row compact (~32px)
        let line_elem = h_flex()
            .w_full()
            .gap_1p5()
            .items_center()
            .child(title_cell(title.clone()))
            .when(status != ChatStatus::Idle, |r| {
                r.child(status.badge(("chat-status-badge", ix), Some(now), cx))
            })
            .child(actions);
        (None, line_elem)
    } else {
        // Un-nested (pinned or direct): keep project tag
        let top_elem = h_flex()
            .w_full()
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(muted)
            .child(slot(project_mark(line.project.as_ref(), cx)))
            .child(div().flex_1().min_w_0().truncate().child(project_label(line.project.as_ref())))
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .font_features(tabular())
                    .when(status != ChatStatus::Idle, |el| {
                        el.child(status.badge(("chat-status-badge", ix), Some(now), cx))
                    })
                    .when(status == ChatStatus::Idle && !corner.is_empty(), |el| {
                        el.child(corner.clone())
                    }),
            );
        let line_elem = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(title_cell(title.clone()))
            .child(actions);
        (Some(top_elem), line_elem)
    };
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
        let short = match place {
            RowPlace::Branch { name, .. } => middle_truncate(name, LONG_BRANCH),
            RowPlace::Folder(name) => name.to_string(),
            RowPlace::NoGit => "No git".to_string(),
        };
        h_flex()
            .min_w_0()
            .gap_1()
            .items_center()
            .child(Icon::new(place.icon()).size(rems(0.75)).flex_none())
            .child(div().min_w_0().truncate().child(short))
            .when(line.removed, |el| el.child(div().flex_none().child("· removed")))
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
        .gap_1p5()
        .items_center()
        .text_xs()
        .text_color(muted)
        .children(place)
        .children(git)
        .child(div().flex_1())
        .children(approve_action)
        .when(nested && status == ChatStatus::Idle && !corner.is_empty(), |el| {
            el.child(div().flex_none().font_features(tabular()).child(corner))
        })
        .children(background)
        .child(provider_icon(line.provider).size(rems(0.8125)).flex_none());
    let row = row.gap_0p5().children(top).child(title_line).child(bottom).context_menu(context);
    HoverCard::new(("chat-card", ix))
        .anchor(Anchor::LeftCenter)
        .ml_2()
        .trigger(row)
        .content(move |_, _, cx| card(title.clone(), line.clone(), cx))
        .into_any_element()
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/chat_row.rs"]
mod tests;
