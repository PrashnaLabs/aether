use crate::theme::Corners;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::SelectableText;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::rems;
use gpui_kit::{AnyElement, App, ClickEvent, ElementId, SharedString, Window, div, prelude::*, px};
use markdown::{ParseOptions, mdast::Node};

use super::diff_view::{DiffView, parse, patch_path};
use super::tool_row::{cached_rows, tabular};
use crate::components::ClickHandler;
use crate::theme;

/// Which pause a card is: an approval (mango) or a question or form (info).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestTone {
    Approval,
    Question,
}

/// A paused run waiting on the user: a header, a free-form body, key-value facts,
/// a row of actions with a hint at its right edge, and a muted footer line.
#[derive(IntoElement)]
pub struct RequestCard {
    tone: RequestTone,
    icon: Icon,
    title: SharedString,
    source: Option<SharedString>,
    aside: Option<SharedString>,
    focused: bool,
    body: Vec<AnyElement>,
    facts: Vec<(SharedString, SharedString)>,
    actions: Vec<AnyElement>,
    hint: Option<SharedString>,
    footer: Option<SharedString>,
}

impl RequestCard {
    pub fn new(tone: RequestTone, icon: impl Into<Icon>, title: impl Into<SharedString>) -> Self {
        Self {
            tone,
            icon: icon.into(),
            title: title.into(),
            source: None,
            aside: None,
            focused: false,
            body: Vec::new(),
            facts: Vec::new(),
            actions: Vec::new(),
            hint: None,
            footer: None,
        }
    }

    /// Muted text right after the title, such as the subagent that asks.
    pub fn source(mut self, text: Option<String>) -> Self {
        self.source = text.map(Into::into);
        self
    }

    /// Muted text at the header's right edge.
    pub fn aside(mut self, text: Option<String>) -> Self {
        self.aside = text.map(Into::into);
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// A labelled value under the body; skipped when the value is empty.
    pub fn fact(mut self, label: &'static str, value: &str) -> Self {
        if !value.trim().is_empty() {
            self.facts.push((label.into(), value.trim().to_string().into()));
        }
        self
    }

    pub fn action(mut self, element: impl IntoElement) -> Self {
        self.actions.push(element.into_any_element());
        self
    }

    pub fn hint(mut self, text: Option<&'static str>) -> Self {
        self.hint = text.map(Into::into);
        self
    }

    pub fn footer(mut self, text: Option<String>) -> Self {
        self.footer = text.map(Into::into);
        self
    }
}

impl ParentElement for RequestCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl RenderOnce for RequestCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (tone, ink) = match self.tone {
            RequestTone::Approval => (theme::mango(), theme::mango_text(cx)),
            RequestTone::Question => (theme.info, theme::info_text(cx)),
        };
        let has_actions = !self.actions.is_empty() || self.hint.is_some();
        v_flex()
            .gap_2()
            .p_3()
            .corner(px(12.))
            .border_1()
            .border_color(tone.opacity(if self.focused { 1. } else { 0.45 }))
            .bg(theme.secondary.opacity(0.5))
            .text_sm()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.icon.small().text_color(ink))
                    .child(div().min_w_0().font_semibold().child(self.title))
                    .children(
                        self.source.map(|s| div().flex_none().text_xs().text_color(muted).child(s)),
                    )
                    .child(div().flex_1())
                    .children(self.aside.map(|text| {
                        div().flex_none().text_xs().text_color(muted).map(tabular).child(text)
                    })),
            )
            .children(self.body)
            .when(!self.facts.is_empty(), |card| {
                card.child(v_flex().gap_1().text_xs().children(self.facts.into_iter().map(
                    |(label, value)| {
                        h_flex()
                            .items_start()
                            .gap_3()
                            .child(div().flex_none().w(px(72.)).text_color(muted).child(label))
                            .child(div().flex_1().min_w_0().child(value))
                    },
                )))
            })
            .when(has_actions, |card| {
                card.child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .children(self.actions)
                        .child(div().flex_1())
                        .children(self.hint.map(|h| div().text_xs().text_color(muted).child(h))),
                )
            })
            .children(self.footer.map(|text| div().text_xs().text_color(muted).child(text)))
    }
}

/// One answer to pick: a round box (pick one) or a square box (pick any), the
/// label, and a muted description. Selected rows take the info tint.
#[derive(IntoElement)]
pub struct RequestOption {
    id: ElementId,
    label: SharedString,
    description: Option<SharedString>,
    multi: bool,
    selected: bool,
    highlighted: bool,
    on_click: Option<ClickHandler>,
}

impl RequestOption {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, multi: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            multi,
            selected: false,
            highlighted: false,
            on_click: None,
        }
    }

    pub fn description(mut self, text: &str) -> Self {
        self.description = (!text.trim().is_empty()).then(|| text.trim().to_string().into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// The keyboard cursor is on this row.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for RequestOption {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let icon = match (self.multi, self.selected) {
            (false, false) => Lucide::Circle,
            (false, true) => Lucide::CircleDot,
            (true, false) => Lucide::Square,
            (true, true) => Lucide::SquareCheck,
        };
        let ink = if self.selected { theme::info_text(cx) } else { theme.muted_foreground };
        let border = match (self.highlighted, self.selected) {
            (true, _) => theme.ring,
            (false, true) => ink,
            (false, false) => theme.border,
        };
        let (wash, hover, pressed) =
            (theme.info.opacity(0.12), theme.secondary_hover, theme.secondary_active);
        let (wash_hover, wash_pressed) = (theme.info.opacity(0.18), theme.info.opacity(0.24));
        h_flex()
            .id(self.id)
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .corner(px(6.))
            .border_1()
            .border_color(border)
            .cursor_pointer()
            .map(|row| match self.selected {
                true => row.bg(wash).hover(|s| s.bg(wash_hover)).active(|s| s.bg(wash_pressed)),
                false => row.hover(|s| s.bg(hover)).active(|s| s.bg(pressed)),
            })
            .child(
                div()
                    .flex_none()
                    .h_5()
                    .flex()
                    .items_center()
                    .child(Icon::new(icon).small().text_color(ink)),
            )
            .child(
                v_flex().flex_1().min_w_0().child(self.label).children(
                    self.description
                        .map(|d| div().text_xs().text_color(theme.muted_foreground).child(d)),
                ),
            )
            .when_some(self.on_click, |row, handler| row.on_click(handler))
    }
}

/// A proposed edit as a diff box; rows are parsed once per change of the diff.
#[derive(IntoElement)]
pub struct RequestDiff {
    id: SharedString,
    diff: SharedString,
}

impl RequestDiff {
    pub fn new(id: impl Into<SharedString>, diff: impl Into<SharedString>) -> Self {
        Self { id: id.into(), diff: diff.into() }
    }
}

impl RenderOnce for RequestDiff {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let rows = cached_rows(cx, &self.id, &self.diff, || parse(&self.diff));
        let path = patch_path(&self.diff).unwrap_or_else(|| "Proposed edit".into());
        DiffView::new(self.id, path.into(), rows, self.diff)
    }
}

/// A command or path a request is about, in a compact mono block; every line is
/// reachable by scrolling.
#[derive(IntoElement)]
pub struct RequestCode {
    id: ElementId,
    text: SharedString,
}

impl RequestCode {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self { id: id.into(), text: text.into() }
    }
}

impl RenderOnce for RequestCode {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let text_id = SharedString::from(format!("{}-text", self.id));
        v_flex()
            .id(self.id)
            .max_h(px(260.))
            .overflow_y_scroll()
            .px_2p5()
            .py_1p5()
            .corner(rems(0.375))
            .bg(theme.muted.opacity(0.5))
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .child(SelectableText::new(text_id, self.text.trim_end().to_string()))
    }
}

/// One numbered answer the model offered; clicking it replies with it.
#[derive(IntoElement)]
pub struct RequestChoice {
    id: SharedString,
    number: usize,
    label: SharedString,
    description: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl RequestChoice {
    /// `label` may carry a `" — "` description, shown muted under it.
    pub fn new(id: impl Into<SharedString>, number: usize, label: impl Into<SharedString>) -> Self {
        let label: SharedString = label.into();
        let (label, description) = match label.split_once(" — ") {
            Some((head, tail)) if !tail.trim().is_empty() => {
                (head.trim().to_string().into(), Some(tail.trim().to_string().into()))
            }
            _ => (label, None),
        };
        Self { id: id.into(), number, label, description, on_click: None }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for RequestChoice {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let on_click = self.on_click;
        let mut btn = crate::components::button(self.id)
            .outline()
            .w_full()
            .h_auto()
            .py_2()
            .px_3()
            .cursor_pointer();

        if let Some(handler) = on_click {
            btn = btn.on_click(handler);
        }

        btn.child(
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .flex_none()
                        .size_5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .corner(rems(0.375))
                        .bg(theme.muted)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(self.number.to_string()),
                )
                .child(
                    v_flex().flex_1().min_w_0().text_left().text_sm().child(self.label).children(
                        self.description
                            .map(|d| div().text_xs().text_color(theme.muted_foreground).child(d)),
                    ),
                )
                .child(
                    Icon::new(IconName::ChevronRight).xsmall().text_color(theme.muted_foreground),
                ),
        )
    }
}

/// How an answered request turned out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Approved,
    Declined,
    /// The run ended before the user answered.
    Missed,
}

/// One muted line recording how a request was answered.
#[derive(IntoElement)]
pub struct ResolvedRequest {
    outcome: Outcome,
    text: SharedString,
}

impl ResolvedRequest {
    pub fn new(outcome: Outcome, text: impl Into<SharedString>) -> Self {
        Self { outcome, text: plain_text(&text.into()).into() }
    }
}

impl RenderOnce for ResolvedRequest {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (icon, color) = match self.outcome {
            Outcome::Approved => (IconName::CircleCheck, theme::success_text(cx)),
            Outcome::Declined => (IconName::CircleX, theme::danger_text(cx)),
            Outcome::Missed => (IconName::Ban, theme.muted_foreground),
        };
        h_flex()
            .min_h(gpui_kit::rems(1.5))
            .px_2()
            .gap_2()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(Icon::new(icon).xsmall().text_color(color))
            .child(div().truncate().child(self.text))
    }
}

/// Markdown flattened to one line of plain text.
fn plain_text(text: &str) -> String {
    fn walk(node: &Node, out: &mut String) {
        match node {
            Node::Text(t) => out.push_str(&t.value),
            Node::InlineCode(code) => out.push_str(&code.value),
            Node::Code(code) => out.push_str(&code.value),
            _ => {}
        }
        for child in node.children().into_iter().flatten() {
            walk(child, out);
        }
        if matches!(
            node,
            Node::Paragraph(_)
                | Node::Heading(_)
                | Node::ListItem(_)
                | Node::Code(_)
                | Node::Break(_)
                | Node::TableCell(_)
        ) {
            out.push(' ');
        }
    }
    let mut out = String::new();
    match markdown::to_mdast(text, &ParseOptions::gfm()) {
        Ok(root) => walk(&root, &mut out),
        Err(_) => out.push_str(text),
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "../../tests/unit/chat/request.rs"]
mod tests;
