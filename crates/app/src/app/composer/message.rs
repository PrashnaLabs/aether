use std::path::Path;
use std::rc::Rc;

use aether_core::{Role, Step, ToolDetail, ToolStatus, is_image};
use aether_ui::theme::CONTENT_WIDTH;
use aether_ui::{
    ActivityFacts, ActivityGroup, AttachmentView, Block, ChangesCard, Markdown, OnOpen, OnOpenDiff,
    ReadGroup, SearchRow, SkillRow, SubagentCard, ThoughtRow, ToolRow, TranscriptRow, UserMessage,
    WorkGroup, activity_group_id, activity_group_open, block_step, block_steps, gap_before,
    read_group_id, relative_path, reply_edits, tool_key, work_label,
};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::{
    AnyElement, App, ClipboardItem, MouseButton, SharedString, Window, div, prelude::*, px,
};

use super::quote::offer_selection;
use crate::app::ChatApp;
use crate::app::chat::row_reflow;
use crate::app::format::format_time;
use crate::app::orchestrate::spawned_child;
use crate::hitl::asks_for;

impl ChatApp {
    pub(crate) fn render_row(
        &self,
        ix: usize,
        view: &crate::app::SessionHandle,
        cx: &App,
    ) -> AnyElement {
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let Some(block) = self.render_block(ix, row, view, cx) else {
            return div().into_any_element();
        };
        let gap = gap_before(ix.checked_sub(1).and_then(|prev| self.rows.get(prev)), row);
        div()
            .w_full()
            .max_w(CONTENT_WIDTH)
            .mx_auto()
            .pt(px(gap))
            .child(
                // Guard the item, leaving row gaps and outside margins available
                // to the pane's background menu. Child menus run before this guard.
                div()
                    .id(("chat-item", ix))
                    .w_full()
                    .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                    .child(block),
            )
            .into_any_element()
    }

    /// `None` when the row has nothing to show.
    fn render_block(
        &self,
        ix: usize,
        row: &TranscriptRow,
        view: &crate::app::SessionHandle,
        cx: &App,
    ) -> Option<AnyElement> {
        let chat = &self.chats[self.active];
        let msg = row.message;
        let message = chat.messages.get(msg)?;
        let key = format!("{}-{msg}", chat.session);
        let (cwd, reflow) = (self.transcript_cwd(), row_reflow(view, ix));
        let call = |i: &usize| match message.steps.get(*i) {
            Some(Step::Tool(call)) => Some(call.clone()),
            _ => None,
        };
        Some(match &row.block {
            Block::User => self.user_message(msg, view),
            Block::SteerAsUser(i) => match message.steps.get(*i) {
                Some(Step::Steer(text)) => {
                    let copy = text.clone();
                    UserMessage::new(key, text.clone())
                        .on_copy(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        })
                        .into_any_element()
                }
                _ => return None,
            },
            Block::Text(range) => {
                let text = message.text.get(range.clone())?;
                let later_text = self.rows[ix + 1..]
                    .iter()
                    .take_while(|r| r.message == msg)
                    .any(|r| matches!(r.block, Block::Text(_)));
                let live = chat.cancel.is_some() && msg + 1 == chat.messages.len() && !later_text;
                let save_note = view.clone();
                let note_text = message.text.clone();
                let notes_on = self.settings.notes_enabled;
                div()
                    .id(SharedString::from(format!("{key}-note-{}", range.start)))
                    .on_mouse_up(MouseButton::Left, offer_selection(view))
                    .child(
                        Markdown::new(
                            format!("{key}-t{}", range.start),
                            SharedString::from(text.to_string()),
                        )
                        .cwd(cwd)
                        .live(live)
                        .on_reflow(reflow),
                    )
                    .context_menu(move |menu, _, _| {
                        if !notes_on {
                            return menu;
                        }
                        let save_note = save_note.clone();
                        let note_text = note_text.clone();
                        menu.item(PopupMenuItem::new("Save to note").on_click(
                            move |_, window, cx| {
                                save_note.update(cx, |this, cx| {
                                    this.save_chat_note(note_text.clone(), window, cx);
                                });
                            },
                        ))
                    })
                    .into_any_element()
            }
            Block::Thought(i) => {
                let (text, ms, live) = match message.steps.get(*i) {
                    Some(Step::Thinking { text, ms: Some(ms) }) => (text, Some(*ms), false),
                    Some(Step::Thinking { text, ms: None })
                        if is_live_thought(&message.steps, *i) =>
                    {
                        let ms = message.live_thinking_ms();
                        (text, ms, ms.is_some())
                    }
                    Some(Step::Thinking { text, ms: None }) => (text, None, false),
                    Some(Step::Thought(text)) => (text, None, false),
                    _ => return None,
                };
                ThoughtRow::new(format!("{key}-s{i}"), text.clone().into(), ms, live)
                    .cwd(cwd)
                    .on_reflow(reflow)
                    .into_any_element()
            }
            Block::Tool(i) => {
                let call = call(i)?;
                let finished = matches!(call.status, ToolStatus::Done | ToolStatus::Unknown);
                let on_diff = match &call.detail {
                    ToolDetail::Edit { path, .. }
                        if self.is_agent_chat(chat) && finished && !path.is_empty() =>
                    {
                        let (view, path) = (view.clone(), relative_path(path, cwd.as_deref()));
                        Some(Rc::new(move |window: &mut Window, cx: &mut App| {
                            view.update(cx, |this, cx| {
                                this.open_reply_diff(msg, Some(path.clone()), window, cx)
                            })
                        }) as OnOpen)
                    }
                    _ => None,
                };
                let on_child = spawned_child(&call.detail)
                    .filter(|&id| self.chat_ix(id).is_some())
                    .map(|id| {
                        let view = view.clone();
                        Rc::new(move |window: &mut Window, cx: &mut App| {
                            view.update(cx, |this, cx| this.open_chat(id, window, cx))
                        }) as OnOpen
                    });
                ToolRow::new(tool_key(&chat.session, &call.id), call)
                    .cwd(cwd)
                    .on_diff(on_diff)
                    .on_child(on_child)
                    .on_reflow(reflow)
                    .into_any_element()
            }
            Block::Activity { kind, steps } => {
                let source: Vec<_> = steps
                    .iter()
                    .filter_map(block_step)
                    .filter_map(|i| message.steps.get(i))
                    .collect();
                let live = chat.cancel.is_some() && msg + 1 == chat.messages.len();
                let facts = ActivityFacts::new(*kind, &source, live);
                let group = tool_key(&chat.session, &format!("{msg}-{}", activity_group_id(steps)));
                let open = activity_group_open(cx, &group);
                let children = steps
                    .iter()
                    .filter(|_| open)
                    .filter_map(|block| {
                        self.render_block(
                            ix,
                            &TranscriptRow { message: msg, block: block.clone() },
                            view,
                            cx,
                        )
                    })
                    .collect();
                ActivityGroup::new(group, facts, children).on_reflow(reflow).into_any_element()
            }
            Block::Worked(steps) => {
                let first = steps.iter().flat_map(block_steps).next().unwrap_or_default();
                let key = tool_key(&chat.session, &format!("{msg}-worked-{first}"));
                let open = activity_group_open(cx, &key);
                let children = steps
                    .iter()
                    .filter(|_| open)
                    .filter_map(|block| {
                        self.render_block(
                            ix,
                            &TranscriptRow { message: msg, block: block.clone() },
                            view,
                            cx,
                        )
                    })
                    .collect();
                let count = steps.iter().map(|b| block_steps(b).len()).sum();
                WorkGroup::new(key, work_label(message, steps), count, children)
                    .on_reflow(reflow)
                    .into_any_element()
            }
            Block::Reads(reads) => {
                let calls: Vec<_> = reads.iter().filter_map(call).collect();
                let first = calls.first().map_or("", |c| c.id.as_str());
                let group = tool_key(&chat.session, &read_group_id(first));
                ReadGroup::new(group, calls).cwd(cwd).on_reflow(reflow).into_any_element()
            }
            Block::Search(i) => match message.steps.get(*i) {
                Some(Step::Search { query, results, .. }) => {
                    SearchRow::new(query.clone(), results.clone()).into_any_element()
                }
                _ => return None,
            },
            Block::Skill(i) => match message.steps.get(*i) {
                Some(Step::Skill(name)) => SkillRow::new(name.clone()).into_any_element(),
                _ => return None,
            },
            Block::Subagent { step, children } => {
                let task = call(step)?;
                let ask = view.clone();
                let rows: Vec<String> = children.iter().filter_map(call).map(|c| c.id).collect();
                let name = match &task.detail {
                    ToolDetail::Task { agent, .. } if !agent.is_empty() => agent.clone(),
                    _ => task.title.clone(),
                };
                let show_ask: OnOpen = Rc::new(move |window, cx| {
                    ask.update(cx, |this, cx| {
                        this.focus_ask_where(|r| asks_for(&r.ask, &rows, &name), window, cx)
                    });
                });
                SubagentCard::new(
                    tool_key(&chat.session, &task.id),
                    task,
                    children.iter().filter_map(call).collect(),
                )
                .session(chat.session.clone())
                .cwd(cwd)
                .on_reflow(reflow)
                .on_show_ask(Some(show_ask))
                .into_any_element()
            }
            Block::Notice(i) => self.notice_row(msg, *i, ix, view),
            Block::Startup(steps) => self.startup_row(msg, steps, view),
            Block::Steer(i) => self.steer_row(msg, *i, view),
            Block::Request(i) => self.render_request(message.requests.get(*i)?, view, cx),
            Block::Fatal => self.fatal_row(msg, ix, view),
            Block::Stopped => self.stopped_row(msg, view),
            Block::Handoff => match &message.handoff {
                Some(h) => aether_ui::HandoffMarker::new(h.clone()).into_any_element(),
                None => div().into_any_element(),
            },
            Block::Fold => self.fold_row(msg, view),
            Block::Changes => {
                let files = reply_edits(&message.steps, cwd.as_deref());
                let open_diff = view.clone();
                let open: OnOpenDiff = Rc::new(move |file, window, cx| {
                    open_diff.update(cx, |this, cx| this.open_reply_diff(msg, file, window, cx));
                });
                ChangesCard::new(format!("{key}-changes").into(), files, open, reflow)
                    .cwd(cwd)
                    .into_any_element()
            }
            Block::Live => return None,
            Block::Footer => return self.footer_row(msg, view),
        })
    }

    fn user_message(&self, ix: usize, view: &crate::app::SessionHandle) -> AnyElement {
        let chat = &self.chats[self.active];
        let message = &chat.messages[ix];
        let last_user = chat.messages[ix + 1..].iter().all(|m| m.role != Role::User);
        let (edit, copy, save_note) = (view.clone(), view.clone(), view.clone());
        let note_text = message.text.clone();
        let views = message
            .attachments
            .iter()
            .map(|a| AttachmentView {
                name: a.name.clone().into(),
                path: a.path.clone(),
                image: is_image(&a.mime),
                size: a.size,
                missing: self.missing_attachments.contains(&a.path),
            })
            .collect();
        let viewer = view.clone();
        let open: Rc<dyn Fn(&Path, &mut Window, &mut App)> = Rc::new(move |path, window, cx| {
            if viewer.update(cx, |app, cx| app.preview_image(path, window, cx)) {
                return;
            }
            cx.open_with_system(path);
        });
        UserMessage::new(format!("{}-{ix}", chat.session), message.text.clone())
            .attachments(views, open)
            .copied(self.copied == Some((self.active, ix)))
            .time(message.sent_at_ms.and_then(format_time))
            .on_copy(move |_, _, cx| copy.update(cx, |this, cx| this.copy_message(ix, cx)))
            .when(self.settings.notes_enabled, |row| {
                row.on_save_note(move |_, window, cx| {
                    save_note.update(cx, |this, cx| {
                        this.save_chat_note(note_text.clone(), window, cx);
                    });
                })
            })
            .when(last_user && self.editing.is_none(), |row| {
                row.on_edit(move |_, window, cx| {
                    edit.update(cx, |this, cx| this.edit_message(ix, window, cx));
                })
            })
            .into_any_element()
    }
}

fn is_live_thought(steps: &[Step], i: usize) -> bool {
    !steps[i + 1..].iter().any(|s| matches!(s, Step::Thinking { ms: None, .. }))
}

#[cfg(test)]
#[path = "../../../tests/unit/app/composer/message.rs"]
mod tests;
