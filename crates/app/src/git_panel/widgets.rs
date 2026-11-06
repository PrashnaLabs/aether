use aether_git::{ChangeKind, Stat};
use aether_ui::theme::Corners;
use aether_ui::{tabular, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme, Icon, StyledExt, h_flex, v_flex};
use gpui_kit::{AnyElement, App, Div, Hsla, SharedString, div, prelude::*, px, rems};

/// A row's trailing details with its actions laid over them. The actions take no width
/// until hover or keyboard focus, so names keep the room; `invisible` also drops their clicks.
pub(super) fn hover_actions(
    group: SharedString,
    show: bool,
    details: impl IntoElement,
    actions: impl IntoElement,
    cx: &App,
) -> Div {
    let fill = aether_ui::row_hover(cx);
    div()
        .relative()
        .flex_none()
        .child(
            h_flex()
                .items_center()
                .when(show, |d| d.invisible())
                .when(!show, |d| d.group_hover(group.clone(), |s| s.invisible()))
                .child(details),
        )
        .child(
            h_flex()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .items_center()
                .pl_1()
                .bg(fill)
                .when(!show, |d| d.invisible().group_hover(group, |s| s.visible()))
                .child(actions),
        )
}

pub(super) fn tone(kind: ChangeKind, cx: &App) -> Hsla {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => theme::success_text(cx),
        ChangeKind::Deleted | ChangeKind::Conflicted => theme::danger_text(cx),
        ChangeKind::Renamed | ChangeKind::Copied => theme::info_text(cx),
        ChangeKind::Modified | ChangeKind::TypeChanged => theme::mango_text(cx),
    }
}

/// Icon, title and a line of text, centered in what's left of the panel.
pub(crate) fn note(
    icon: Lucide,
    color: Hsla,
    title: impl Into<SharedString>,
    text: impl Into<SharedString>,
    cx: &App,
) -> gpui_kit::Div {
    v_flex()
        .flex_1()
        .min_h_0()
        .px_6()
        .gap_1()
        .items_center()
        .justify_center()
        .text_center()
        .child(Icon::new(icon).size(rems(1.25)).text_color(color).mb_1())
        .child(div().text_sm().font_medium().child(title.into()))
        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(text.into()))
}

/// The status letter on a tint of its color; the letter carries the meaning.
pub(super) fn letter_chip(kind: ChangeKind, cx: &App) -> gpui_kit::Div {
    let color = tone(kind, cx);
    div()
        .flex_none()
        .size(rems(1.))
        .corner(px(6.))
        .bg(color.opacity(0.12))
        .text_color(color)
        .text_xs()
        .font_semibold()
        .flex()
        .items_center()
        .justify_center()
        .child(kind.letter().to_string())
}

/// `+N −M` in add/remove colors with their signs, or "binary".
pub(super) fn stat_element(stat: Stat, cx: &App) -> AnyElement {
    let row = h_flex().flex_none().gap_1().text_xs().map(tabular);
    match stat {
        Stat::Lines(added, removed) => row
            .when(added > 0, |r| {
                r.child(div().text_color(theme::success_text(cx)).child(format!("+{added}")))
            })
            .when(removed > 0, |r| {
                r.child(div().text_color(theme::danger_text(cx)).child(format!("−{removed}")))
            })
            .into_any_element(),
        Stat::Binary => {
            row.text_color(cx.theme().muted_foreground).child("binary").into_any_element()
        }
        Stat::Unknown => row.into_any_element(),
    }
}
