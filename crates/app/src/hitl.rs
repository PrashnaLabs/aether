use std::collections::BTreeSet;
use std::iter::once;

use aether_core::{
    Answer, Approval, ApprovalKind, ApprovalOption, Ask, Attachment, Choice, FieldKind, Form, Plan,
    Question, Request, RequestStatus, ToolPolicy, field_label, field_visible, form_answer,
};
use aether_ui::{
    Block, Markdown, Outcome, RequestCard, RequestChoice, RequestCode, RequestDiff, RequestOption,
    RequestTone, ResolvedRequest, TranscriptRow, clock, field, field_area, since, theme,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::input::Paste;
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    input::{InputEvent, InputState, TextareaState},
    kbd::Kbd,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, Focusable, KeyDownEvent, Keystroke,
    Modifiers, SharedString, Window, div, prelude::*, px,
};

use crate::actions::{ASK_CONTEXT, AskDismiss, AskNext, AskPrev, AskPrimary, AskToggle};
use crate::app::{ChatApp, now_ms};

/// What a pending card keeps between frames.
pub(crate) struct RequestInput {
    window: gpui_kit::WindowId,
    entry: Entry,
    focus: FocusHandle,
    /// The question shown, of several.
    step: usize,
    /// The option or button the keyboard is on.
    cursor: usize,
}

/// The editable field a pending request offers.
enum Entry {
    Line(Entity<InputState>),
    Block(Entity<TextareaState>),
    /// One line per question or form field, rendered only where it takes typed text.
    Fields(Vec<Entity<InputState>>),
}

impl RequestInput {
    fn value(&self, cx: &App) -> String {
        match &self.entry {
            Entry::Line(input) => input.read(cx).value().to_string(),
            Entry::Block(input) => input.read(cx).value().to_string(),
            Entry::Fields(_) => String::new(),
        }
    }

    fn lines(&self) -> &[Entity<InputState>] {
        match &self.entry {
            Entry::Fields(lines) => lines,
            _ => &[],
        }
    }
}

/// Something on a pending card the keyboard can land on.
#[derive(Clone, PartialEq)]
enum Target {
    Submit(Submit),
    Pick(ApprovalOption),
    /// Option `k` of question or field `i`; one past the options is "Something else…".
    Option {
        i: usize,
        k: usize,
        multi: bool,
    },
}

#[derive(Clone, Copy)]
enum AskKey {
    Primary,
    Dismiss,
    Prev,
    Next,
    Toggle,
}

fn plan_text(plan: &Plan) -> String {
    if !plan.markdown.is_empty() {
        return plan.markdown.clone();
    }
    plan.steps
        .iter()
        .enumerate()
        .map(|(i, step)| format!("{}. {step}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn choice_value(choice: &Choice) -> &str {
    if choice.value.is_empty() { &choice.label } else { &choice.value }
}

/// Picked option labels in option order, then the typed text where "Something else…" is
/// picked or the question has no options.
pub(crate) fn questions_answer(
    questions: &[Question],
    picks: &[BTreeSet<usize>],
    typed: &[String],
) -> Answer {
    Answer::Answers(
        questions
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let picks = picks.get(i);
                let picked = picks.into_iter().flatten().filter_map(|&k| q.options.get(k));
                let other = q.other && picks.is_some_and(|p| p.contains(&q.options.len()));
                let free = other || q.options.is_empty();
                let text = typed.get(i).map(|t| t.trim()).filter(|t| free && !t.is_empty());
                picked.map(|c| c.label.clone()).chain(text.map(str::to_string)).collect()
            })
            .collect(),
    )
}

/// In agent chats the line is a note for the agent, sent only with a deny; elsewhere it's
/// the edited input.
fn approval_answer(
    approval: &Approval,
    typed: Option<String>,
    agent: bool,
    submit: Submit,
) -> Answer {
    if let Submit::Deny = submit {
        let note = typed.filter(|_| agent).unwrap_or_default().trim().to_string();
        return if note.is_empty() { Answer::Deny } else { Answer::DenyWith(note) };
    }
    let input = typed.filter(|_| !agent).unwrap_or_else(|| approval.input.clone());
    Answer::Approve { input, always: matches!(submit, Submit::AlwaysAllow) }
}

/// Only aether's own chats edit the input; a `Pick` carries none, so it would run the original.
fn input_edited(agent: bool, original: &str, typed: &str) -> bool {
    !agent && typed.trim() != original.trim()
}

/// Allow once, the driver's options as given, aether's own "Always allow" outside agent
/// chats (CLIs scope "always" through options), then Deny.
fn approval_targets(approval: &Approval, agent: bool) -> Vec<Target> {
    once(Target::Submit(Submit::Allow))
        .chain(approval.options.iter().cloned().map(Target::Pick))
        .chain((!agent).then_some(Target::Submit(Submit::AlwaysAllow)))
        .chain(once(Target::Submit(Submit::Deny)))
        .collect()
}

fn plan_targets(plan: &Plan) -> Vec<Target> {
    once(Target::Submit(Submit::ApprovePlan))
        .chain(plan.options.iter().cloned().map(Target::Pick))
        .chain(once(Target::Submit(Submit::RejectPlan)))
        .collect()
}

/// What an approval is about, once: CLIs often repeat the command or path as the title.
fn approval_subject(title: &str, input: &str) -> String {
    let (title, input) = (title.trim(), input.trim());
    let first = input.lines().next().unwrap_or_default();
    if input.is_empty() {
        title.to_string()
    } else if title.is_empty() || first.starts_with(title) {
        first.to_string()
    } else {
        format!("{title} “{first}”")
    }
}

/// Card title: the CLI's own, unless it just repeats the input.
fn approval_heading(who: &str, approval: &Approval) -> String {
    let (title, input) = (approval.title.trim(), approval.input.trim());
    if !title.is_empty() && !input.starts_with(title) {
        return title.to_string();
    }
    let verb = match approval.kind {
        ApprovalKind::Command => "run a command",
        ApprovalKind::Edit => "edit a file",
        ApprovalKind::Fetch => "fetch a URL",
        ApprovalKind::Mcp => "use a tool",
        ApprovalKind::Other | ApprovalKind::Unknown => "continue",
    };
    format!("Allow {who} to {verb}?")
}

/// A choice by its number (`"2"`), else the typed text.
fn pick_choice(value: &str, choices: &[String]) -> String {
    let value = value.trim();
    value
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| choices.get(i))
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

/// `form_answer`, refusing a number field that holds anything else.
fn form_check(form: &Form, values: &[Vec<String>]) -> Result<Answer, String> {
    let bad = form.fields.iter().zip(values).find(|(f, v)| {
        f.kind == FieldKind::Number
            && field_visible(f, &form.fields, values)
            && v.iter().any(|n| !n.parse::<f64>().is_ok_and(f64::is_finite))
    });
    match bad {
        Some((f, _)) => Err(format!("{} must be a number", field_label(f))),
        None => form_answer(form, values),
    }
}

/// A new ask moves focus to its card only in the chat on screen, and never while the user
/// is typing in the composer.
pub(crate) fn takes_focus(active_chat: bool, composer_empty: bool) -> bool {
    active_chat && composer_empty
}

/// The composer holds no draft: no text and no attachment.
fn composer_empty(text: &str, pending: &[Attachment]) -> bool {
    text.trim().is_empty() && pending.is_empty()
}

/// A key pressed on a focused card that belongs in the composer instead.
#[derive(Debug, PartialEq)]
enum Redirect {
    Type(String),
    Paste,
    /// An IME composition starts; it continues in the composer.
    Compose,
}

fn redirect(k: &Keystroke) -> Option<Redirect> {
    let m = &k.modifiers;
    if k.key == "v" && *m == Modifiers::secondary_key() {
        return Some(Redirect::Paste);
    }
    // named keys like shift-tab also come without a key_char
    if k.is_ime_in_progress() && k.key.chars().count() == 1 {
        return Some(Redirect::Compose);
    }
    let text = k.key_char.clone()?;
    (!(m.platform || m.control || m.function) && !text.chars().any(char::is_control))
        .then_some(Redirect::Type(text))
}

/// The pending approval a subagent card's "show ask" goes to: one of its rows' by tool id,
/// else one asked in its name.
pub(crate) fn asks_for(ask: &Ask, rows: &[String], name: &str) -> bool {
    match ask {
        Ask::Approval(a) => match &a.tool_id {
            Some(id) => rows.contains(id),
            None => a.source.as_deref() == Some(name),
        },
        _ => false,
    }
}

/// What Enter in a card's single line sends.
fn enter_submits(ask: &Ask, agent: bool) -> Option<Submit> {
    match ask {
        Ask::Question { .. } => Some(Submit::Reply),
        Ask::Approval(_) if agent => Some(Submit::Deny),
        Ask::Approval(_) => Some(Submit::Allow),
        Ask::Plan(_) if agent => Some(Submit::RejectPlan),
        Ask::Questions(_) | Ask::Form(_) | Ask::Plan(_) | Ask::Unknown => None,
    }
}

pub(crate) fn queue_note(others: &[&str]) -> Option<String> {
    let first = others.first()?;
    Some(match others.len() {
        1 => format!("1 more request queued: {first}"),
        n => format!("{n} more requests queued: {first}"),
    })
}

pub(crate) fn stepper(i: usize, n: usize) -> Option<String> {
    (n > 1).then(|| format!("Question {} of {n}", i + 1))
}

pub(crate) fn pick_hint(multi: bool) -> &'static str {
    if multi { "Pick any" } else { "Pick one" }
}

fn kbd(key: &str) -> Option<Kbd> {
    Keystroke::parse(key).ok().map(Kbd::new)
}

impl ChatApp {
    pub(crate) fn rebind_session_requests(
        &mut self,
        chat_id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chats.iter().find(|c| c.id == chat_id) else { return };
        let agent = chat.is_agent();
        let requests: Vec<_> = chat
            .messages
            .iter()
            .flat_map(|m| &m.requests)
            .filter(|r| r.status == RequestStatus::Pending)
            .map(|r| (r.id, r.ask.clone()))
            .collect();
        for (id, ask) in requests {
            let saved = self.request_inputs.get(&id).map(|i| {
                let text = i.value(cx);
                let lines =
                    i.lines().iter().map(|l| l.read(cx).value().to_string()).collect::<Vec<_>>();
                (text, lines, i.step, i.cursor)
            });
            let picks = self.request_picks.get(&id).cloned();
            self.add_request_input(id, &ask, agent, false, window, cx);
            if let Some((text, lines, step, cursor)) = saved
                && let Some(input) = self.request_inputs.get_mut(&id)
            {
                input.step = step;
                input.cursor = cursor;
                match &input.entry {
                    Entry::Line(line) => line.update(cx, |s, cx| s.set_value(text, window, cx)),
                    Entry::Block(block) => block.update(cx, |s, cx| s.set_value(text, window, cx)),
                    Entry::Fields(fields) => {
                        for (field, value) in fields.iter().zip(lines) {
                            field.update(cx, |s, cx| s.set_value(value, window, cx));
                        }
                    }
                }
            }
            if let Some(picks) = picks {
                self.request_picks.insert(id, picks);
            }
        }
    }

    /// Fields for a new request. A CLI only honours an approval as asked and a plan as
    /// proposed, so in agent chats those are read-only; they get a note or feedback line.
    pub(crate) fn add_request_input(
        &mut self,
        id: u64,
        ask: &Ask,
        agent: bool,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            Entry::Line(cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)))
        };
        let entry = match ask {
            Ask::Approval(_) if agent => {
                line("Deny and tell the agent what to do instead…", window, cx)
            }
            Ask::Approval(Approval { input, .. }) => {
                let value = input.clone();
                Entry::Line(cx.new(|cx| {
                    let mut state = InputState::new(window, cx);
                    state.set_value(value, window, cx);
                    state
                }))
            }
            Ask::Question { choices, .. } if choices.is_empty() => {
                line("Type your answer", window, cx)
            }
            Ask::Question { .. } => line("Or type your own answer, or a choice number", window, cx),
            Ask::Questions(questions) => {
                let lines = questions.iter().map(|q| {
                    let hint =
                        if q.options.is_empty() { "Type your answer" } else { "Something else…" };
                    cx.new(|cx| InputState::new(window, cx).masked(q.secret).placeholder(hint))
                });
                Entry::Fields(lines.collect())
            }
            Ask::Form(form) => {
                let picks = form.fields.iter().map(|f| match f.kind {
                    FieldKind::Bool => f.default.iter().map(|v| usize::from(v != "true")).collect(),
                    _ => (0..f.options.len())
                        .filter(|&k| f.default.iter().any(|v| v == choice_value(&f.options[k])))
                        .collect(),
                });
                self.request_picks.insert(id, picks.collect());
                let lines = form.fields.iter().map(|f| {
                    let picked = matches!(f.kind, FieldKind::Choice | FieldKind::Multi);
                    let value = if picked { String::new() } else { f.default.join(", ") };
                    cx.new(|cx| {
                        let mut state = InputState::new(window, cx).masked(f.secret);
                        state.set_value(value, window, cx);
                        state
                    })
                });
                Entry::Fields(lines.collect())
            }
            Ask::Plan(_) if agent => line("Feedback if you send it back (optional)", window, cx),
            Ask::Plan(plan) => {
                let value = plan_text(plan);
                Entry::Block(cx.new(|cx| {
                    let mut state = TextareaState::new(window, cx).rows(6);
                    state.set_value(value, window, cx);
                    state
                }))
            }
            Ask::Unknown => return,
        };
        if let (Entry::Line(line), Some(submit)) = (&entry, enter_submits(ask, agent)) {
            let subscription =
                cx.subscribe_in(line, window, move |this, _, ev: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = ev {
                        this.submit_request(id, submit, window, cx);
                    }
                });
            self.request_subscriptions.insert(id, subscription);
        }
        let focus = cx.focus_handle().tab_stop(true);
        self.request_inputs.insert(
            id,
            RequestInput {
                window: window.window_handle().window_id(),
                entry,
                focus,
                step: 0,
                cursor: 0,
            },
        );
        let composer = self.composer.focus_handle(cx);
        let from_composer = window.focused(cx).is_none_or(|f| f == composer);
        let empty =
            composer_empty(&self.composer.read(cx).value(), &self.chats[self.active].pending);
        if from_composer && takes_focus(active, empty) {
            // after this update, once the request is in the reply and has a row
            cx.defer_in(window, |this, window, cx| this.focus_ask(window, cx));
        }
    }

    fn locate_request(&self, id: u64) -> Option<(usize, usize)> {
        let pending = |m: &aether_core::Message| {
            m.requests.iter().any(|r| r.id == id && r.status == RequestStatus::Pending)
        };
        let find = |chat: usize| {
            self.chats[chat].messages.iter().rposition(pending).map(|message| (chat, message))
        };
        find(self.active).or_else(|| (0..self.chats.len()).rev().find_map(find))
    }

    fn find_request(&self, id: u64) -> Option<(&Request, usize, bool)> {
        let (chat, message) = self.locate_request(id)?;
        let request = self.chats[chat].messages[message].requests.iter().find(|r| r.id == id)?;
        Some((request, chat, self.chats[chat].is_agent()))
    }

    /// Forgets a request's fields, picks and errors once it can't be answered here. If its
    /// card had focus, the next pending card takes it, as a new ask would, else the composer.
    pub(crate) fn drop_request(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.request_inputs.remove(&id);
        self.request_subscriptions.remove(&id);
        self.editing_plans.remove(&id);
        self.request_picks.remove(&id);
        self.request_errors.remove(&id);
        if !input.is_some_and(|r| r.focus.contains_focused(window, cx)) {
            return;
        }
        let chat = &self.chats[self.active];
        let next = chat.messages.iter().flat_map(|m| &m.requests).any(|r| {
            r.id != id
                && r.status == RequestStatus::Pending
                && self.request_inputs.contains_key(&r.id)
        });
        let empty = composer_empty(&self.composer.read(cx).value(), &chat.pending);
        if next && takes_focus(true, empty) {
            // deferred: the caller syncs rows after this
            cx.defer_in(window, |this, window, cx| this.focus_ask(window, cx));
        } else {
            self.focus_composer(window, cx);
        }
    }

    /// Records the answer, resumes the paused run, and remembers "Always allow".
    pub(crate) fn answer_request(
        &mut self,
        id: u64,
        answer: Answer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((chat, message)) = self.locate_request(id) else {
            return;
        };
        let agent = self.chats[chat].is_agent();
        let msg = &mut self.chats[chat].messages[message];
        let tool = msg.requests.iter().find(|r| r.id == id).and_then(|r| match &r.ask {
            Ask::Approval(a) => Some(a.tool.clone()),
            _ => None,
        });
        if !msg.answer(id, answer.clone()) {
            return;
        }
        // CLI tool names aren't aether's tools; the CLI keeps its own session allowance
        if let (Answer::Approve { always: true, .. }, Some(tool), false) = (&answer, tool, agent) {
            self.update_settings(cx, |s| {
                s.tool_approvals.insert(tool, ToolPolicy::Allow);
            });
        }
        self.agent.answer(id, answer);
        self.drop_request(id, window, cx);
        if chat == self.active {
            self.sync_rows(Some(message), cx);
        }
        cx.notify();
    }

    /// Scrolls to the active chat's first pending request and focuses its card.
    pub(crate) fn focus_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_ask_where(|_| true, window, cx);
    }

    /// `focus_ask` for the first pending request that `wanted`.
    pub(crate) fn focus_ask_where(
        &mut self,
        wanted: impl Fn(&Request) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let messages = &self.chats[self.active].messages;
        let pending = |r: &TranscriptRow| match r.block {
            Block::Request(i) => messages
                .get(r.message)
                .and_then(|m| m.requests.get(i))
                .filter(|q| q.status == RequestStatus::Pending && wanted(q))
                .map(|q| q.id),
            _ => None,
        };
        let Some((row, id)) =
            self.rows.iter().enumerate().find_map(|(row, r)| Some((row, pending(r)?)))
        else {
            return;
        };
        self.scroller.update(cx, |s, cx| {
            s.scroll_to_item(row, cx);
        });
        if let Some(input) = self.request_inputs.get(&id) {
            window.focus(&input.focus, cx);
        }
    }

    /// Toggles option `k` of question or field `i`, or makes it the only pick.
    fn pick_option(&mut self, id: u64, i: usize, k: usize, multi: bool, cx: &mut Context<Self>) {
        let picks = self.request_picks.entry(id).or_default();
        if picks.len() <= i {
            picks.resize(i + 1, BTreeSet::new());
        }
        let set = &mut picks[i];
        if !multi {
            *set = BTreeSet::from([k]);
        } else if !set.remove(&k) {
            set.insert(k);
        }
        self.refresh_request(id, cx);
    }

    fn refresh_request(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some((chat, message)) = self.locate_request(id)
            && chat == self.active
        {
            self.sync_rows(Some(message), cx);
        }
        cx.notify();
    }

    /// Shows question `step` of a set, or answers the set past its last question.
    fn step_to(&mut self, id: u64, step: usize, window: &mut Window, cx: &mut Context<Self>) {
        let n = match self.find_request(id) {
            Some((Request { ask: Ask::Questions(questions), .. }, ..)) => questions.len(),
            _ => return,
        };
        if step >= n {
            return self.submit_request(id, Submit::Answers, window, cx);
        }
        let Some(input) = self.request_inputs.get_mut(&id) else {
            return;
        };
        (input.step, input.cursor) = (step, 0);
        // the field it was in may be gone with the old question
        if input.focus.contains_focused(window, cx) {
            window.focus(&input.focus, cx);
        }
        self.refresh_request(id, cx);
    }

    fn activate(&mut self, id: u64, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            Target::Submit(submit) => self.submit_request(id, submit, window, cx),
            Target::Pick(option) => self.answer_request(id, Answer::Pick(option.id), window, cx),
            Target::Option { i, k, multi } => self.pick_option(id, i, k, multi, cx),
        }
    }

    /// Each field's current values: picked options, "true"/"false", or trimmed typed text.
    fn form_values(&self, id: u64, form: &Form, cx: &App) -> Vec<Vec<String>> {
        let lines = self.request_inputs.get(&id).map(RequestInput::lines).unwrap_or_default();
        let picks = self.request_picks.get(&id);
        let fields = form.fields.iter().enumerate();
        fields
            .map(|(i, f)| {
                let typed = lines
                    .get(i)
                    .map(|l| l.read(cx).value().trim().to_string())
                    .filter(|t| !t.is_empty());
                let picks = picks.and_then(|p| p.get(i));
                let other = f.other && picks.is_some_and(|p| p.contains(&f.options.len()));
                let picked = picks.into_iter().flatten();
                match f.kind {
                    FieldKind::Bool => picked.map(|&k| (k == 0).to_string()).collect(),
                    FieldKind::Choice | FieldKind::Multi => picked
                        .filter_map(|&k| f.options.get(k))
                        .map(|c| choice_value(c).to_string())
                        .chain(typed.filter(|_| other))
                        .collect(),
                    _ => typed.into_iter().collect(),
                }
            })
            .collect()
    }

    /// What the keyboard can land on: the buttons of an approval or plan, else the option
    /// rows of the question shown or of the visible form fields.
    fn targets(
        &self,
        request: &Request,
        agent: bool,
        input: &RequestInput,
        cx: &App,
    ) -> Vec<Target> {
        let option = |i, k, multi| Target::Option { i, k, multi };
        match &request.ask {
            Ask::Approval(a) => {
                let edited = input_edited(agent, &a.input, &input.value(cx));
                let targets = approval_targets(a, agent).into_iter();
                targets.filter(|t| !(edited && matches!(t, Target::Pick(_)))).collect()
            }
            Ask::Plan(plan) => plan_targets(plan),
            Ask::Questions(questions) => questions.get(input.step).map_or(Vec::new(), |q| {
                let n = q.options.len() + usize::from(q.other);
                (0..n).map(|k| option(input.step, k, q.multi)).collect()
            }),
            Ask::Form(form) => {
                let values = self.form_values(request.id, form, cx);
                let visible = form.fields.iter().enumerate();
                let visible = visible.filter(|(_, f)| field_visible(f, &form.fields, &values));
                visible
                    .flat_map(|(i, f)| {
                        let (n, multi) = match f.kind {
                            FieldKind::Bool => (2, false),
                            FieldKind::Choice => (f.options.len() + usize::from(f.other), false),
                            FieldKind::Multi => (f.options.len() + usize::from(f.other), true),
                            _ => (0, false),
                        };
                        (0..n).map(move |k| option(i, k, multi))
                    })
                    .collect()
            }
            Ask::Question { .. } | Ask::Unknown => Vec::new(),
        }
    }

    /// Card keys act only while the card itself has focus, so a key meant for one of its
    /// buttons or fields reaches it. Enter in a question or form field still moves on.
    fn ask_key(&mut self, id: u64, key: AskKey, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(input), Some((request, _, agent))) =
            (self.request_inputs.get(&id), self.find_request(id))
        else {
            return cx.propagate();
        };
        let card = input.focus.clone();
        let on_card = card.is_focused(window);
        let in_line = input.lines().iter().any(|l| l.focus_handle(cx).is_focused(window));
        let targets = self.targets(request, agent, input, cx);
        let current = targets.get(input.cursor).cloned();
        let (step, last) = (input.step, targets.len().saturating_sub(1));
        let approval = matches!(request.ask, Ask::Approval(_));
        let primary = match &request.ask {
            Ask::Approval(_) | Ask::Plan(_) => None,
            Ask::Questions(_) => Some(Submit::Answers),
            Ask::Form(_) => Some(Submit::Form),
            Ask::Question { .. } => Some(Submit::Reply),
            Ask::Unknown => return cx.propagate(),
        };
        match key {
            AskKey::Dismiss if !on_card => window.focus(&card, cx),
            AskKey::Dismiss if approval => self.submit_request(id, Submit::Deny, window, cx),
            AskKey::Dismiss => self.focus_composer(window, cx),
            AskKey::Primary if on_card || in_line => match (primary, current) {
                (Some(Submit::Answers), _) => self.step_to(id, step + 1, window, cx),
                (Some(submit), _) => self.submit_request(id, submit, window, cx),
                (None, Some(target)) => self.activate(id, target, window, cx),
                (None, None) => {}
            },
            AskKey::Toggle if on_card => match current {
                Some(target) => self.activate(id, target, window, cx),
                // nothing lit: a space is typing, like any other key
                None => {
                    self.focus_composer(window, cx);
                    self.composer.update(cx, |composer, cx| composer.insert(" ", window, cx));
                }
            },
            AskKey::Prev | AskKey::Next if on_card => {
                if let Some(input) = self.request_inputs.get_mut(&id) {
                    input.cursor = match key {
                        AskKey::Prev => input.cursor.saturating_sub(1),
                        _ => (input.cursor + 1).min(last),
                    };
                }
                cx.notify();
            }
            _ => cx.propagate(),
        }
    }

    /// Typing, pasting or composing on a focused card goes to the composer: an ask that took
    /// focus just as the user started a message must not leave their next Enter to approve it.
    fn ask_typed(
        &mut self,
        id: u64,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_card = self.request_inputs.get(&id).is_some_and(|r| r.focus.is_focused(window));
        let Some(to) = redirect(&ev.keystroke).filter(|_| on_card) else {
            return;
        };
        cx.stop_propagation();
        self.focus_composer(window, cx);
        match to {
            Redirect::Type(text) => {
                self.composer.update(cx, |composer, cx| composer.insert(text, window, cx))
            }
            // deferred: the composer's paste hook updates ChatApp, which is leased here
            Redirect::Paste => cx.defer_in(window, |this, window, cx| {
                this.composer.focus_handle(cx).dispatch_action(&Paste, window, cx)
            }),
            Redirect::Compose => {}
        }
    }

    /// Answers from the request's field, or from the request itself when it has none.
    fn submit_request(
        &mut self,
        id: u64,
        submit: Submit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((request, _, agent)) = self.find_request(id) else {
            return;
        };
        let typed = self.request_inputs.get(&id).map(|input| input.value(cx));
        let answer = match (&request.ask, submit) {
            (Ask::Approval(approval), Submit::Allow | Submit::AlwaysAllow | Submit::Deny) => {
                approval_answer(approval, typed, agent, submit)
            }
            (Ask::Question { choices, .. }, Submit::Reply) => {
                let reply = pick_choice(&typed.unwrap_or_default(), choices);
                if reply.is_empty() {
                    return;
                }
                Answer::Reply(reply)
            }
            (Ask::Questions(questions), Submit::Answers) => {
                let lines = self.request_inputs.get(&id).map(RequestInput::lines);
                let typed: Vec<String> = lines
                    .unwrap_or_default()
                    .iter()
                    .map(|l| l.read(cx).value().to_string())
                    .collect();
                let picks = self.request_picks.get(&id).map(Vec::as_slice).unwrap_or_default();
                questions_answer(questions, picks, &typed)
            }
            (Ask::Form(form), Submit::Form) => {
                match form_check(form, &self.form_values(id, form, cx)) {
                    Ok(answer) => answer,
                    Err(error) => {
                        self.request_errors.insert(id, error);
                        return self.refresh_request(id, cx);
                    }
                }
            }
            (Ask::Plan(plan), Submit::ApprovePlan) => {
                let edited = typed.filter(|_| self.editing_plans.contains(&id));
                Answer::Plan { approved: true, text: edited.unwrap_or_else(|| plan_text(plan)) }
            }
            (Ask::Plan(plan), Submit::RejectPlan) => {
                let text = typed.unwrap_or_default();
                let unchanged = text == plan_text(plan);
                Answer::Plan { approved: false, text: if unchanged { String::new() } else { text } }
            }
            _ => return,
        };
        self.answer_request(id, answer, window, cx);
    }

    pub(crate) fn render_request(
        &self,
        request: &Request,
        view: &crate::app::SessionHandle,
        _: &App,
    ) -> AnyElement {
        if request.status != RequestStatus::Pending {
            return ResolvedRequest::new(outcome(request), resolved_text(request))
                .into_any_element();
        }
        AskCard { view: view.clone(), id: request.id }.into_any_element()
    }

    fn ask_card(
        &self,
        id: u64,
        view: &crate::app::SessionHandle,
        window: &Window,
        cx: &App,
    ) -> Option<AnyElement> {
        let (request, chat, agent) = self.find_request(id)?;
        let input = self.request_inputs.get(&id)?;
        let who = if agent { self.chats[chat].provider.label() } else { "Aether" };
        let focused = input.focus.contains_focused(window, cx);
        let targets = self.targets(request, agent, input, cx);
        let current = targets.get(input.cursor).filter(|_| input.focus.is_focused(window));
        let lit = |t: &Target| current == Some(t);
        let on_click = |f: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
            let view = view.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| f(this, window, cx));
            }
        };
        let button = |key: String, label: SharedString, target: Target| {
            aether_ui::button(SharedString::from(format!("request-{id}-{key}")))
                .small()
                .min_w_0()
                .max_w_full()
                .tooltip(label.clone())
                .label(label)
                .selected(lit(&target))
                .on_click(on_click(Box::new(move |this, window, cx| {
                    this.activate(id, target.clone(), window, cx)
                })))
        };
        let answer_button = |key: &str, label: &'static str, answer: Answer| {
            aether_ui::button(SharedString::from(format!("request-{id}-{key}")))
                .small()
                .outline()
                .label(label)
                .on_click(on_click(Box::new(move |this, window, cx| {
                    this.answer_request(id, answer.clone(), window, cx)
                })))
        };
        let target_button = |(k, target): (usize, Target)| {
            let primary = matches!(target, Target::Submit(Submit::Allow | Submit::ApprovePlan));
            let (label, keys): (SharedString, _) = match &target {
                Target::Submit(Submit::Allow) => ("Allow once".into(), Some("enter")),
                Target::Submit(Submit::AlwaysAllow) => ("Always allow".into(), None),
                Target::Submit(Submit::Deny) => ("Deny".into(), Some("escape")),
                Target::Submit(Submit::ApprovePlan) => ("Approve".into(), None),
                Target::Submit(Submit::RejectPlan) => ("Send back".into(), None),
                Target::Pick(option) => (option.label.clone().into(), None),
                Target::Submit(Submit::Reply | Submit::Answers | Submit::Form)
                | Target::Option { .. } => return None,
            };
            let button = button(format!("target-{k}"), label, target);
            let button = if primary { button.primary() } else { button.outline() };
            Some(button.children(keys.and_then(kbd)))
        };
        let picks = self.request_picks.get(&id);
        let picked =
            |i: usize, k: usize| picks.and_then(|p| p.get(i)).is_some_and(|s| s.contains(&k));
        let option_row = |i: usize, k: usize, multi: bool, label: &str, description: &str| {
            let target = Target::Option { i, k, multi };
            let at = targets.iter().position(|t| *t == target).unwrap_or_default();
            RequestOption::new(
                SharedString::from(format!("request-{id}-{i}-option-{k}")),
                label.to_string(),
                multi,
            )
            .description(description)
            .selected(picked(i, k))
            .highlighted(lit(&target))
            .on_click(on_click(Box::new(move |this, window, cx| {
                if let Some(input) = this.request_inputs.get_mut(&id) {
                    input.cursor = at;
                }
                this.activate(id, target.clone(), window, cx)
            })))
        };
        let lines = input.lines();
        let line = |i: usize| lines.get(i).map(|l| field(l).small());
        let entry = match &input.entry {
            Entry::Line(line) => Some(field(line).small().into_any_element()),
            Entry::Block(block) => Some(field_area(block).into_any_element()),
            Entry::Fields(_) => None,
        };
        let inline = |entry: Option<AnyElement>| {
            entry.map(|e| div().flex_1().min_w(px(180.)).child(e).into_any_element())
        };
        let card = match &request.ask {
            Ask::Approval(a) => {
                let edited = input_edited(agent, &a.input, &input.value(cx));
                let pending =
                    self.chats[chat].messages.iter().flat_map(|m| &m.requests).filter(|r| {
                        r.status == RequestStatus::Pending && matches!(r.ask, Ask::Approval(_))
                    });
                let queued: Vec<String> = pending
                    .skip_while(|r| r.id != id)
                    .skip(1)
                    .filter_map(|r| match &r.ask {
                        Ask::Approval(q) => Some(approval_subject(&q.title, &q.input)),
                        _ => None,
                    })
                    .collect();
                let queued: Vec<&str> = queued.iter().map(String::as_str).collect();
                let buttons =
                    approval_targets(a, agent).into_iter().enumerate().filter_map(|(k, t)| {
                        let off = edited && matches!(t, Target::Pick(_));
                        target_button((k, t)).map(|b| b.disabled(off))
                    });
                let (editable, note) = if agent { (None, entry) } else { (entry, None) };
                RequestCard::new(RequestTone::Approval, Lucide::Shield, approval_heading(who, a))
                    .source(a.source.as_ref().map(|s| format!("· subagent {s}")))
                    .aside(since(request.asked_ms, now_ms()).map(|t| format!("waiting {t}")))
                    .focused(focused)
                    .when(agent && !a.input.trim().is_empty(), |card| {
                        card.child(RequestCode::new(
                            SharedString::from(format!("request-{id}-code")),
                            a.input.clone(),
                        ))
                    })
                    .children(editable)
                    .children(
                        a.diff.clone().map(|d| RequestDiff::new(format!("request-{id}-diff"), d)),
                    )
                    .fact("Directory", a.cwd.as_deref().unwrap_or_default())
                    .fact("Reason", &a.reason)
                    .map(|card| buttons.fold(card, RequestCard::action))
                    .children(inline(note))
                    .footer(queue_note(&queued))
            }
            Ask::Question { question, choices } => RequestCard::new(
                RequestTone::Question,
                Lucide::CircleQuestionMark,
                format!("{who} has a question"),
            )
            .focused(focused)
            .child(Markdown::new(format!("request-{id}-question"), question.clone().into()))
            .when(!choices.is_empty(), |card| {
                card.child(v_flex().gap_2().children(choices.iter().enumerate().map(
                    |(i, choice)| {
                        let reply = choice.clone();
                        RequestChoice::new(
                            format!("request-{id}-choice-{i}"),
                            i + 1,
                            choice.clone(),
                        )
                        .on_click(on_click(Box::new(
                            move |this, window, cx| {
                                this.answer_request(id, Answer::Reply(reply.clone()), window, cx)
                            },
                        )))
                    },
                )))
            })
            .children(entry)
            .action(
                aether_ui::button(SharedString::from(format!("request-{id}-send")))
                    .small()
                    .primary()
                    .label("Send")
                    .children(kbd("enter"))
                    .on_click(on_click(Box::new(move |this, window, cx| {
                        this.submit_request(id, Submit::Reply, window, cx)
                    }))),
            )
            .action(answer_button("skip", "Skip", Answer::Deny)),
            Ask::Questions(questions) => {
                let n = questions.len();
                let step = input.step.min(n.saturating_sub(1));
                let q = questions.get(step)?;
                let other = q.options.len();
                let aside = stepper(step, n)
                    .into_iter()
                    .chain(Some(q.header.trim().to_string()).filter(|h| !h.is_empty()));
                let rows = q
                    .options
                    .iter()
                    .enumerate()
                    .map(|(k, c)| option_row(step, k, q.multi, &c.label, &c.description));
                let rows = rows.chain(
                    q.other.then(|| option_row(step, other, q.multi, "Something else…", "")),
                );
                let free = q.options.is_empty() || (q.other && picked(step, other));
                let label = if step + 1 < n { "Next" } else { "Submit" };
                RequestCard::new(
                    RequestTone::Question,
                    Lucide::CircleQuestionMark,
                    format!("{who} has a question"),
                )
                .aside(Some(aside.collect::<Vec<_>>().join(" · ")).filter(|a| !a.is_empty()))
                .focused(focused)
                .child(div().text_size(px(14.)).child(Markdown::new(
                    format!("request-{id}-question-{step}"),
                    q.question.clone().into(),
                )))
                .children(rows)
                .when(free, |card| card.children(line(step)))
                .action(
                    aether_ui::button(SharedString::from(format!("request-{id}-next")))
                        .small()
                        .primary()
                        .label(label)
                        .children(kbd("enter"))
                        .on_click(on_click(Box::new(move |this, window, cx| {
                            this.step_to(id, step + 1, window, cx)
                        }))),
                )
                .when(step > 0, |card| {
                    card.action(
                        aether_ui::button(SharedString::from(format!("request-{id}-back")))
                            .small()
                            .outline()
                            .label("Back")
                            .on_click(on_click(Box::new(move |this, window, cx| {
                                this.step_to(id, step - 1, window, cx)
                            }))),
                    )
                })
                .action(answer_button("skip", "Let the agent decide", Answer::Deny))
                .hint((!q.options.is_empty()).then(|| pick_hint(q.multi)))
            }
            Ask::Form(form) => {
                let values = self.form_values(id, form, cx);
                let error =
                    self.request_errors.contains_key(&id).then(|| form_check(form, &values));
                let title = match form.title.trim() {
                    "" => format!("{who} needs input"),
                    title => title.to_string(),
                };
                let closes = form.timeout_ms.zip(request.asked_ms).map(|(ms, at)| {
                    let left = (at + ms as i64 - now_ms()).max(0) as u64;
                    format!("Closes in {}", clock(left))
                });
                let yes_no = [
                    Choice { label: "Yes".into(), ..Choice::default() },
                    Choice { label: "No".into(), ..Choice::default() },
                ];
                let visible = form
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| field_visible(f, &form.fields, &values));
                let muted = cx.theme().muted_foreground;
                RequestCard::new(RequestTone::Question, Lucide::CircleQuestionMark, title)
                    .aside(closes)
                    .focused(focused)
                    .when(!form.message.trim().is_empty(), |card| {
                        card.child(div().child(form.message.trim().to_string()))
                    })
                    .when_some(form.url.clone(), |card, url| {
                        let shown = url.clone();
                        card.child(
                            h_flex()
                                .items_start()
                                .gap_2()
                                .child(div().flex_1().min_w_0().child(RequestCode::new(
                                    SharedString::from(format!("request-{id}-url")),
                                    shown,
                                )))
                                .child(
                                    aether_ui::button(SharedString::from(format!(
                                        "request-{id}-open"
                                    )))
                                    .small()
                                    .outline()
                                    .icon(Lucide::ExternalLink)
                                    .label("Open link")
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                                ),
                        )
                    })
                    .children(visible.map(|(i, f)| {
                        let required = if f.required { " (required)" } else { "" };
                        let (options, multi) = match f.kind {
                            FieldKind::Bool => (&yes_no[..], false),
                            FieldKind::Choice => (&f.options[..], false),
                            FieldKind::Multi => (&f.options[..], true),
                            _ => (&[][..], false),
                        };
                        let choosing = matches!(f.kind, FieldKind::Choice | FieldKind::Multi);
                        let other = choosing && f.other;
                        let typed = match f.kind {
                            FieldKind::Bool => false,
                            FieldKind::Choice | FieldKind::Multi => {
                                other && picked(i, f.options.len())
                            }
                            _ => true,
                        };
                        let rows = options
                            .iter()
                            .enumerate()
                            .map(|(k, c)| option_row(i, k, multi, &c.label, &c.description));
                        let rows =
                            rows.chain(other.then(|| {
                                option_row(i, f.options.len(), multi, "Something else…", "")
                            }));
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_medium()
                                    .child(format!("{}{required}", field_label(f))),
                            )
                            .when(!f.description.is_empty(), |c| {
                                c.child(
                                    div().text_xs().text_color(muted).child(f.description.clone()),
                                )
                            })
                            .children(rows)
                            .when(typed, |c| c.children(line(i)))
                    }))
                    .children(error.and_then(Result::err).map(|error| {
                        div().text_xs().text_color(theme::danger_text(cx)).child(error)
                    }))
                    .action(
                        aether_ui::button(SharedString::from(format!("request-{id}-submit")))
                            .small()
                            .primary()
                            .label("Submit")
                            .children(kbd("enter"))
                            .on_click(on_click(Box::new(move |this, window, cx| {
                                this.submit_request(id, Submit::Form, window, cx)
                            }))),
                    )
                    .action(answer_button("cancel", "Cancel", Answer::Deny))
            }
            Ask::Plan(plan) => {
                let editing = self.editing_plans.contains(&id);
                let markdown = match plan.markdown.is_empty() {
                    true => {
                        plan.steps.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
                    }
                    false => plan.markdown.clone(),
                };
                let body = Markdown::new(format!("request-{id}-plan"), markdown.into());
                let buttons = plan_targets(plan).into_iter().enumerate().filter_map(target_button);
                let (editor, note) = if agent { (None, entry) } else { (entry, None) };
                RequestCard::new(RequestTone::Approval, Lucide::Map, "Proposed plan")
                    .focused(focused)
                    .map(|card| match editor.filter(|_| editing) {
                        Some(editor) => card.child(editor),
                        None => card.child(body),
                    })
                    .map(|card| buttons.fold(card, RequestCard::action))
                    .children(inline(note))
                    .when(!agent && !editing, |card| {
                        card.action(
                            aether_ui::button(SharedString::from(format!("request-{id}-edit")))
                                .small()
                                .ghost()
                                .label("Edit plan")
                                .on_click(on_click(Box::new(move |this, _, cx| {
                                    this.editing_plans.insert(id);
                                    this.refresh_request(id, cx);
                                }))),
                        )
                    })
            }
            Ask::Unknown => return None,
        };
        let act = |key: AskKey| {
            let view = view.clone();
            move |window: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| this.ask_key(id, key, window, cx))
            }
        };
        let (primary, dismiss) = (act(AskKey::Primary), act(AskKey::Dismiss));
        let (prev, next, toggle) = (act(AskKey::Prev), act(AskKey::Next), act(AskKey::Toggle));
        let typed = view.clone();
        Some(
            div()
                .id(SharedString::from(format!("request-{id}-card")))
                .track_focus(&input.focus)
                .key_context(ASK_CONTEXT)
                .on_action(move |_: &AskPrimary, window, cx| primary(window, cx))
                .on_action(move |_: &AskDismiss, window, cx| dismiss(window, cx))
                .on_action(move |_: &AskPrev, window, cx| prev(window, cx))
                .on_action(move |_: &AskNext, window, cx| next(window, cx))
                .on_action(move |_: &AskToggle, window, cx| toggle(window, cx))
                .on_key_down(move |ev, window, cx| {
                    typed.update(cx, |this, cx| this.ask_typed(id, ev, window, cx))
                })
                .child(card)
                .into_any_element(),
        )
    }
}

/// A pending card, built at layout time where the window is known, so it can show focus.
#[derive(IntoElement)]
struct AskCard {
    view: crate::app::SessionHandle,
    id: u64,
}

impl RenderOnce for AskCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.view
            .update(cx, |app, cx| {
                if app
                    .request_inputs
                    .get(&self.id)
                    .is_some_and(|input| input.window != window.window_handle().window_id())
                {
                    let chat = app.chats[app.active].id;
                    app.rebind_session_requests(chat, window, cx);
                }
                app.ask_card(self.id, &self.view, window, cx)
            })
            .unwrap_or_else(|| div().into_any_element())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Submit {
    Allow,
    AlwaysAllow,
    Deny,
    Reply,
    Answers,
    Form,
    ApprovePlan,
    RejectPlan,
}

fn outcome(request: &Request) -> Outcome {
    let RequestStatus::Answered(answer) = &request.status else {
        return Outcome::Missed;
    };
    match (&request.ask, answer) {
        (Ask::Question { .. }, Answer::Reply(_)) => Outcome::Approved,
        (Ask::Questions(_), Answer::Answers(a)) if a.iter().any(|v| !v.is_empty()) => {
            Outcome::Approved
        }
        (Ask::Question { .. } | Ask::Questions(_) | Ask::Unknown, _) => Outcome::Missed,
        (
            _,
            Answer::Approve { .. }
            | Answer::Pick(_)
            | Answer::Form(_)
            | Answer::Plan { approved: true, .. },
        ) => Outcome::Approved,
        _ => Outcome::Declined,
    }
}

fn resolved_text(request: &Request) -> String {
    let form_title = |f: &Form| match f.title.trim() {
        "" => "Form".to_string(),
        title => title.to_string(),
    };
    let first_question =
        |qs: &[Question]| qs.first().map(|q| q.question.clone()).unwrap_or_default();
    let label = |options: &[ApprovalOption], id: &String| {
        options.iter().find(|o| o.id == *id).map_or(id.clone(), |o| o.label.clone())
    };
    let what = || match &request.ask {
        Ask::Approval(a) => approval_subject(&a.title, &a.input),
        Ask::Question { question, .. } => question.clone(),
        Ask::Questions(questions) => first_question(questions),
        Ask::Form(form) => form_title(form),
        Ask::Plan(_) => "Proposed plan".to_string(),
        Ask::Unknown => "Request from a newer version".to_string(),
    };
    let answer = match &request.status {
        RequestStatus::Answered(answer) => answer,
        RequestStatus::Withdrawn => return format!("Answered elsewhere · {}", what()),
        _ => return format!("Not answered · {}", what()),
    };
    match (&request.ask, answer) {
        (Ask::Approval(a), Answer::Approve { input, always }) => {
            let allowed = if *always { "Allowed" } else { "Allowed once" };
            format!("{allowed} · {}", approval_subject(&a.title, input))
        }
        (Ask::Approval(a), Answer::Pick(id)) => format!("Allowed · {}", label(&a.options, id)),
        (Ask::Approval(a), Answer::DenyWith(note)) => {
            format!("Declined · {} · {note}", approval_subject(&a.title, &a.input))
        }
        (Ask::Approval(a), _) => format!("Declined · {}", approval_subject(&a.title, &a.input)),
        (Ask::Question { question, .. }, Answer::Reply(reply)) => format!("{question} → {reply}"),
        (Ask::Question { question, .. }, _) => format!("{question} → no answer"),
        (Ask::Questions(questions), Answer::Answers(answers))
            if answers.iter().any(|a| !a.is_empty()) =>
        {
            let answers = answers.iter().flatten().cloned().collect::<Vec<_>>().join(", ");
            format!("{} → {answers}", first_question(questions))
        }
        (Ask::Questions(questions), _) => format!("{} → no answer", first_question(questions)),
        (Ask::Form(form), Answer::Form(_)) => format!("{} → sent", form_title(form)),
        (Ask::Form(form), _) => format!("{} → cancelled", form_title(form)),
        (Ask::Plan(plan), Answer::Pick(id)) => {
            format!("Plan approved · {}", label(&plan.options, id))
        }
        (Ask::Plan(_), Answer::Plan { approved: true, .. }) => "Plan approved".to_string(),
        (Ask::Plan(_), Answer::Plan { text, .. }) if !text.trim().is_empty() => {
            format!("Plan sent back · {text}")
        }
        (Ask::Plan(_), _) => "Plan rejected".to_string(),
        (Ask::Unknown, _) => "Request from a newer version".to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/hitl.rs"]
mod tests;
