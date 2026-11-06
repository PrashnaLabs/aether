use aether_ui::QuoteChip;
use aether_ui::theme::Corners;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::TextSelection;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, h_flex};
use gpui_kit::rems;
use gpui_kit::{
    Anchor, App, ClipboardItem, Context, MouseUpEvent, Pixels, Point, Window, anchored, deferred,
    point, prelude::*, px,
};

use crate::app::{ChatApp, Queued, SessionHandle};

fn blockquote(quote: &str) -> String {
    quote.lines().map(|line| format!("> {line}")).collect::<Vec<_>>().join("\n")
}

/// What Send sends: the quote as a blockquote, a blank line, then the typed text.
pub(in crate::app) fn quoted(quote: Option<&str>, text: &str) -> String {
    match quote {
        Some(quote) if text.is_empty() => blockquote(quote),
        Some(quote) => format!("{}\n\n{text}", blockquote(quote)),
        None => text.to_string(),
    }
}

fn explain(selection: &str) -> String {
    format!("Explain this:\n\n{}", blockquote(selection))
}

/// Mouse-up on reply text: offers the toolbar for what is selected now.
pub(in crate::app) fn offer_selection(
    view: &SessionHandle,
) -> impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |ev, window, cx| {
        let (view, at) = (view.clone(), ev.position);
        // the selection layer ends the drag after this listener runs
        window.defer(cx, move |window, cx| {
            let text = TextSelection::selected_text(window, cx);
            view.update(cx, |this, cx| this.show_selection_bar(at, text.trim(), cx));
        });
    }
}

impl ChatApp {
    fn show_selection_bar(&mut self, at: Point<Pixels>, text: &str, cx: &mut Context<Self>) {
        let chat = self.chats[self.active].id;
        self.selection_bar = (!text.is_empty()).then(|| (chat, at, text.to_string().into()));
        cx.notify();
    }

    pub(crate) fn hide_selection_bar(&mut self, cx: &mut Context<Self>) {
        if self.selection_bar.take().is_some() {
            cx.notify();
        }
    }

    fn quote_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((chat, _, text)) = self.selection_bar.take() else { return };
        self.quotes.insert(chat, text);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Sends it at once, queued behind a running turn like a typed message.
    fn explain_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, _, text)) = self.selection_bar.take() else { return };
        cx.notify();
        let ix = self.active;
        if self.locked(ix) || self.worktree_blocks_send(ix).is_some() {
            return;
        }
        let text = explain(&text);
        if self.chats[ix].busy() {
            let queued = Queued {
                text,
                attachments: Vec::new(),
                delivering: false,
                from_children: false,
                deliveries: vec![],
            };
            return self.chats[ix].queue.push(queued);
        }
        self.begin_setup_or_submit(ix, text, Vec::new(), window, cx);
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some((_, _, text)) = self.selection_bar.take() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            cx.notify();
        }
    }

    pub(in crate::app) fn quote_chip(&self, cx: &Context<Self>) -> Option<QuoteChip> {
        let chat = self.chats[self.active].id;
        let text = self.quotes.get(&chat)?.clone();
        Some(QuoteChip::new(
            text,
            Box::new(self.session_listener(cx, move |this, _, window, cx| {
                this.quotes.remove(&chat);
                this.focus_composer(window, cx);
                cx.notify();
            })),
        ))
    }

    /// Quote, Explain and Copy over the active chat's reply selection.
    pub(crate) fn render_selection_bar(
        &self,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (chat, at, _) = self.selection_bar.as_ref()?;
        if *chat != self.chats[self.active].id {
            return None;
        }
        let theme = cx.theme();
        let bar = h_flex()
            .id("selection-bar")
            .occlude()
            .gap_1()
            .p_1()
            .corner(rems(0.5))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_lg()
            .on_mouse_down_out(
                self.session_listener(cx, |this, _, _, cx| this.hide_selection_bar(cx)),
            )
            .child(
                aether_ui::button("selection-quote")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::TextQuote)
                    .label("Quote")
                    .on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.quote_selection(window, cx)
                    })),
            )
            .child(
                aether_ui::button("selection-explain")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Lightbulb)
                    .label("Explain")
                    .on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.explain_selection(window, cx)
                    })),
            )
            .child(
                aether_ui::button("selection-copy")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Copy)
                    .label("Copy")
                    .on_click(self.session_listener(cx, |this, _, _, cx| this.copy_selection(cx))),
            );
        Some(deferred(
            anchored()
                .position(point(at.x, at.y - px(8.)))
                .anchor(Anchor::BottomCenter)
                .snap_to_window_with_margin(px(8.))
                .child(bar),
        ))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/composer/quote.rs"]
mod tests;
