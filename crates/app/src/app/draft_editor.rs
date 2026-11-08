use crate::session_workspace::WindowId;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::input::{Editor, EditorState, Position};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, Window, div, prelude::*, px, relative};

use super::{ChatApp, chat_cwd};
use crate::actions::{DRAFT_EDITOR_CONTEXT, DraftEditorDone, DraftEditorSend};

fn end_position(text: &str) -> Position {
    let line = text.matches('\n').count();
    let last = text.rsplit('\n').next().unwrap_or("");
    Position::new(line as u32, last.encode_utf16().count() as u32)
}

impl ChatApp {
    pub(super) fn draft_editor_visible(&self, host: WindowId, cx: &gpui_kit::App) -> bool {
        (self.settings.chat_layout != aether_core::ChatLayout::Tabbed
            || !self.side_open()
            || self.side_tab_for(host) == crate::side_panel::SideTab::Agent)
            && !self.is_terminal_drawer_maximized(cx)
    }

    pub(crate) fn open_draft_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chat = &self.chats[self.active];
        if self.draft_editor.is_some()
            || !self.draft_editor_visible(self.host_of(window), cx)
            || self.switching_folder(&chat_cwd(chat, &self.settings.workspace_root))
        {
            return;
        }
        self.end_hidden_voice(window, cx);
        let text = self.composer.read(cx).value().to_string();
        let end = end_position(&text);
        let editor =
            cx.new(|cx| EditorState::new(window, cx).language("markdown").default_value(text));
        editor.update(cx, |e, cx| {
            e.set_cursor_position(end, window, cx);
            e.focus(window, cx);
        });
        self.draft_editor = Some(editor);
        cx.notify();
    }

    pub(crate) fn close_draft_editor(
        &mut self,
        apply: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.draft_editor.take() else { return };
        let text = editor.read(cx).value().to_string();
        if apply {
            self.suggestions = None;
            self.suggestions_dismissed = None;
        }
        self.composer.update(cx, |c, cx| {
            if apply {
                c.set_value(text, window, cx);
            }
            let end = end_position(&c.value());
            c.set_cursor_position(end, window, cx);
            c.focus(window, cx);
        });
        cx.notify();
    }

    fn send_from_draft_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_draft_editor(true, window, cx);
        self.send(window, cx);
    }

    pub(crate) fn render_draft_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.draft_editor.clone()?;
        let theme = cx.theme();
        Some(
            v_flex()
                .debug_selector(|| "draft-editor".into())
                .absolute()
                .inset_0()
                .occlude()
                .items_center()
                .justify_center()
                .p_4()
                .child(
                    v_flex()
                        .debug_selector(|| "draft-editor-dialog".into())
                        .w_full()
                        .max_w(px(860.))
                        .h(relative(0.8))
                        .min_h_0()
                        .rounded_xl()
                        .overflow_hidden()
                        .border_1()
                        .border_color(theme.border)
                        .shadow_lg()
                        .bg(theme.background.alpha(1.))
                        .key_context(DRAFT_EDITOR_CONTEXT)
                        .on_action(self.session_listener(
                            cx,
                            |this, _: &DraftEditorDone, window, cx| {
                                this.close_draft_editor(true, window, cx)
                            },
                        ))
                        .on_action(self.session_listener(
                            cx,
                            |this, _: &DraftEditorSend, window, cx| {
                                this.send_from_draft_editor(window, cx)
                            },
                        ))
                        .child(
                            h_flex()
                                .w_full()
                                .px_4()
                                .py_2()
                                .gap_3()
                                .items_center()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(div().text_sm().font_semibold().child("Edit draft"))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child("Esc or Ctrl+G to return · ⌘↩ to send"),
                                )
                                .child(
                                    aether_ui::button("done-draft").small().label("Done").on_click(
                                        self.session_listener(cx, |this, _, window, cx| {
                                            this.close_draft_editor(true, window, cx)
                                        }),
                                    ),
                                )
                                .child(
                                    aether_ui::button("discard-draft")
                                        .ghost()
                                        .small()
                                        .label("Discard changes")
                                        .on_click(self.session_listener(
                                            cx,
                                            |this, _, window, cx| {
                                                this.close_draft_editor(false, window, cx)
                                            },
                                        )),
                                ),
                        )
                        .child(div().flex_1().min_h_0().w_full().px_2().py_3().child(
                            Editor::new(&editor).h(relative(1.)).appearance(false).bordered(false),
                        )),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/draft_editor.rs"]
mod tests;
