use super::*;
use aether_core::{Attachment, Field};

#[test]
fn a_number_picks_its_choice_and_anything_else_is_the_answer() {
    let choices = ["Red — warm".to_string(), "Blue".to_string()];
    assert_eq!(pick_choice(" 2 ", &choices), "Blue");
    assert_eq!(pick_choice("1", &choices), "Red — warm");
    assert_eq!(pick_choice("3", &choices), "3");
    assert_eq!(pick_choice("0", &choices), "0");
    assert_eq!(pick_choice("green", &choices), "green");
    assert_eq!(pick_choice("2", &[]), "2");
}

#[test]
fn an_approval_says_its_subject_once() {
    let path = "src/routes/index.tsx";
    assert_eq!(approval_subject(path, path), path);
    assert_eq!(
        approval_subject("pnpm build", "pnpm build 2>&1 | tail -25"),
        "pnpm build 2>&1 | tail -25"
    );
    assert_eq!(approval_subject("Run command", "ls -la"), "Run command “ls -la”");
    assert_eq!(approval_subject("Write probe.txt", ""), "Write probe.txt");
    let ask = |title: &str, input: &str, kind| Approval {
        title: title.into(),
        input: input.into(),
        kind,
        ..Approval::default()
    };
    let heading = |a: Approval| approval_heading("OpenCode", &a);
    assert_eq!(
        heading(ask("pnpm build", "pnpm build 2>&1", ApprovalKind::Command)),
        "Allow OpenCode to run a command?"
    );
    assert_eq!(heading(ask(path, path, ApprovalKind::Edit)), "Allow OpenCode to edit a file?");
    assert_eq!(
        heading(ask("", "https://x.y", ApprovalKind::Fetch)),
        "Allow OpenCode to fetch a URL?"
    );
    assert_eq!(heading(ask("", "{}", ApprovalKind::Mcp)), "Allow OpenCode to use a tool?");
    assert_eq!(heading(ask("", "", ApprovalKind::Other)), "Allow OpenCode to continue?");
    assert_eq!(heading(ask("Run command", "ls", ApprovalKind::Command)), "Run command");
}

fn choice(label: &str, value: &str) -> Choice {
    Choice { label: label.into(), value: value.into(), ..Choice::default() }
}

#[test]
fn questions_answer_keeps_picks_and_other_text_per_question() {
    let q = |multi, other| Question {
        question: "q".into(),
        multi,
        other,
        options: vec![choice("A", ""), choice("B", ""), choice("C", "")],
        ..Question::default()
    };
    // index 3, one past the options, is "Something else…"
    let picks = [BTreeSet::from([2, 0]), BTreeSet::from([3]), BTreeSet::from([1])];
    let typed = [String::new(), " mine ".into(), "ignored".into()];
    assert_eq!(
        questions_answer(&[q(true, false), q(false, true), q(false, false)], &picks, &typed),
        Answer::Answers(vec![vec!["A".into(), "C".into()], vec!["mine".into()], vec!["B".into()]])
    );
}

#[test]
fn a_question_without_options_takes_typed_text() {
    let open = Question { question: "Name?".into(), ..Question::default() };
    assert_eq!(
        questions_answer(&[open], &[], &[" Ada ".into()]),
        Answer::Answers(vec![vec!["Ada".into()]])
    );
}

#[test]
fn an_agent_approval_note_only_goes_out_with_a_deny() {
    let bash = Approval { tool: "Bash".into(), input: "rm -rf dist".into(), ..Approval::default() };
    let note = || Some(" use trash ".to_string());
    assert_eq!(
        approval_answer(&bash, note(), true, Submit::Allow),
        Answer::Approve { input: "rm -rf dist".into(), always: false }
    );
    assert_eq!(
        approval_answer(&bash, note(), true, Submit::AlwaysAllow),
        Answer::Approve { input: "rm -rf dist".into(), always: true }
    );
    assert_eq!(
        approval_answer(&bash, note(), true, Submit::Deny),
        Answer::DenyWith("use trash".into())
    );
    assert_eq!(approval_answer(&bash, Some("  ".into()), true, Submit::Deny), Answer::Deny);
    let edited = Some("rm -rf build".to_string());
    assert_eq!(
        approval_answer(&bash, edited.clone(), false, Submit::Allow),
        Answer::Approve { input: "rm -rf build".into(), always: false }
    );
    assert_eq!(approval_answer(&bash, edited, false, Submit::Deny), Answer::Deny);
}

#[test]
fn typed_text_only_counts_while_something_else_is_picked() {
    let q = Question {
        question: "q".into(),
        other: true,
        options: vec![choice("A", "")],
        ..Question::default()
    };
    assert_eq!(
        questions_answer(&[q], &[BTreeSet::from([0])], &["left over".into()]),
        Answer::Answers(vec![vec!["A".into()]])
    );
}

#[test]
fn ask_takes_focus_only_from_an_empty_composer() {
    assert!(takes_focus(true, true));
    assert!(!takes_focus(true, false), "the user is typing");
    assert!(!takes_focus(false, true), "another chat");
    let image = Attachment {
        name: "shot.png".into(),
        path: "/tmp/shot.png".into(),
        mime: "image/png".into(),
        size: 1,
    };
    assert!(takes_focus(true, composer_empty(" \n", &[])), "whitespace is empty");
    assert!(!takes_focus(true, composer_empty("", &[image])), "an attachment is a draft");
}

#[test]
fn enter_in_a_card_field_answers_what_the_field_is_for() {
    let approval = Ask::Approval(Approval::default());
    let plan = Ask::Plan(Plan::default());
    let question = Ask::Question { question: "q".into(), choices: vec![] };
    assert_eq!(enter_submits(&approval, true), Some(Submit::Deny), "the deny note");
    assert_eq!(enter_submits(&approval, false), Some(Submit::Allow), "the edited command");
    assert_eq!(enter_submits(&plan, true), Some(Submit::RejectPlan), "the plan feedback");
    assert_eq!(enter_submits(&plan, false), None, "the plan editor takes newlines");
    assert_eq!(enter_submits(&question, false), Some(Submit::Reply));
    assert_eq!(enter_submits(&Ask::Questions(vec![]), true), None, "the card moves on");
}

#[test]
fn queue_notes_and_steppers_read_like_the_prototype() {
    assert_eq!(queue_note(&[]), None);
    assert_eq!(
        queue_note(&["Run `pnpm build`"]).as_deref(),
        Some("1 more request queued: Run `pnpm build`")
    );
    assert_eq!(queue_note(&["a", "b"]).as_deref(), Some("2 more requests queued: a"));
    assert_eq!(stepper(0, 1), None);
    assert_eq!(stepper(0, 2).as_deref(), Some("Question 1 of 2"));
    assert_eq!((pick_hint(true), pick_hint(false)), ("Pick any", "Pick one"));
}

#[test]
fn withdrawn_and_once_resolutions_say_what_happened() {
    let approval = Ask::Approval(Approval {
        title: "Run command".into(),
        input: "pnpm build".into(),
        ..Approval::default()
    });
    let req = |status| Request { id: 1, at: 0, asked_ms: None, ask: approval.clone(), status };
    assert_eq!(
        resolved_text(&req(RequestStatus::Withdrawn)),
        "Answered elsewhere · Run command “pnpm build”"
    );
    assert_eq!(
        resolved_text(&req(RequestStatus::Answered(Answer::Approve {
            input: "pnpm build".into(),
            always: false
        }))),
        "Allowed once · Run command “pnpm build”"
    );
    assert_eq!(outcome(&req(RequestStatus::Withdrawn)), Outcome::Missed);
}

#[test]
fn an_empty_questions_answer_reads_as_skipped() {
    let ask = Ask::Questions(vec![Question { question: "Name?".into(), ..Question::default() }]);
    let req = |answers| Request {
        id: 1,
        at: 0,
        asked_ms: None,
        ask: ask.clone(),
        status: RequestStatus::Answered(Answer::Answers(answers)),
    };
    let skipped = req(vec![vec![]]);
    assert_eq!(
        (outcome(&skipped), resolved_text(&skipped).as_str()),
        (Outcome::Missed, "Name? → no answer")
    );
    let answered = req(vec![vec!["Ada".into()]]);
    assert_eq!(
        (outcome(&answered), resolved_text(&answered).as_str()),
        (Outcome::Approved, "Name? → Ada")
    );
}

#[test]
fn number_fields_must_hold_numbers() {
    let field = |key: &str, kind| Field { key: key.into(), kind, ..Field::default() };
    let form = Form {
        fields: vec![field("port", FieldKind::Number), field("name", FieldKind::Text)],
        ..Form::default()
    };
    assert_eq!(
        form_check(&form, &[vec!["80a".into()], vec!["x".into()]]),
        Err("port must be a number".into())
    );
    for odd in ["NaN", "inf", "-infinity"] {
        assert_eq!(
            form_check(&form, &[vec![odd.into()], vec![]]),
            Err("port must be a number".into()),
            "{odd}"
        );
    }
    assert_eq!(
        form_check(&form, &[vec!["8080".into()], vec![]]),
        Ok(Answer::Form(vec![("port".into(), vec!["8080".into()])]))
    );
}

#[test]
fn an_edited_command_is_only_an_edit_in_aethers_own_chats() {
    assert!(input_edited(false, "rm -rf dist", " rm -rf build "));
    assert!(!input_edited(false, "rm -rf dist", "rm -rf dist "));
    assert!(!input_edited(true, "rm -rf dist", "use trash instead"), "a note, not an edit");
}

#[test]
fn keys_on_a_focused_card_that_belong_in_the_composer() {
    let key = |k: &str| redirect(&Keystroke::parse(k).unwrap());
    assert_eq!(key("a->a"), Some(Redirect::Type("a".into())));
    assert_eq!(key("secondary-v"), Some(Redirect::Paste), "paste then Enter must not approve");
    assert_eq!(key("a"), Some(Redirect::Compose), "an IME composition starts");
    assert_eq!(key("secondary-c"), None);
    assert_eq!(key("secondary-shift-v"), None);
    assert_eq!(key("escape"), None);
    assert_eq!(key("shift-tab"), None, "moves focus back");
}

#[test]
fn a_subagent_card_shows_its_own_ask() {
    let ask = |tool_id: Option<&str>, source: Option<&str>| {
        Ask::Approval(Approval {
            tool_id: tool_id.map(Into::into),
            source: source.map(Into::into),
            ..Approval::default()
        })
    };
    let rows = ["c1".to_string(), "c2".to_string()];
    assert!(asks_for(&ask(Some("c2"), Some("explore")), &rows, "designer"));
    assert!(!asks_for(&ask(Some("x9"), Some("designer")), &rows, "designer"));
    assert!(asks_for(&ask(None, Some("designer")), &rows, "designer"));
    assert!(!asks_for(&ask(None, None), &rows, "designer"));
    assert!(!asks_for(&Ask::Unknown, &rows, "designer"));
}

#[gpui_kit::test]
fn session_transfer_keeps_partially_typed_request(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    window
        .update(cx, |app, window, cx| {
            let chat = app.chats[app.active].id;
            let ask = Ask::Question { question: "Which branch?".into(), choices: vec![] };
            let mut message = aether_core::Message::assistant();
            message.apply(aether_core::Update::Ask { id: 700, ask: ask.clone() });
            app.chats[app.active].messages.push(message);
            app.add_request_input(700, &ask, false, false, window, cx);
            if let Entry::Line(input) = &app.request_inputs[&700].entry {
                input.update(cx, |state, cx| state.set_value("feature/partial", window, cx));
            } else {
                panic!("question should accept a typed reply");
            }
            app.rebind_session_ui(chat, window, cx);
            assert_eq!(app.request_inputs[&700].value(cx), "feature/partial");
            assert_eq!(
                app.chats[app.active].messages[0].requests[0].status,
                RequestStatus::Pending
            );
        })
        .unwrap();
}

#[test]
fn secret_answers_are_masked_before_they_are_kept() {
    let question = |secret| Question { question: "q".into(), secret, ..Question::default() };
    let ask = Ask::Questions(vec![question(false), question(true)]);
    let answer = Answer::Answers(vec![vec!["plain".into()], vec!["hunter2".into()]]);
    let Answer::Answers(kept) = redact_secrets(&ask, &answer) else { panic!("kept") };
    assert_eq!(kept[0], ["plain"]);
    assert_eq!(kept[1], [MASK]);

    let field = |key: &str, secret| Field { key: key.into(), secret, ..Field::default() };
    let form = Ask::Form(Form {
        fields: vec![field("user", false), field("token", true)],
        ..Form::default()
    });
    let answer = Answer::Form(vec![
        ("user".into(), vec!["me".into()]),
        ("token".into(), vec!["sk-live".into()]),
    ]);
    let Answer::Form(kept) = redact_secrets(&form, &answer) else { panic!("kept") };
    assert_eq!(kept[0].1, ["me"]);
    assert_eq!(kept[1].1, [MASK]);
}
