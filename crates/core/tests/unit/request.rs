use super::*;

#[test]
fn old_ask_json_round_trips_unchanged() {
    for old in [
        r#"{"Approval":{"tool":"Bash","title":"Run command","input":"ls\nrm -rf dist","reason":""}}"#,
        r#"{"Plan":{"steps":["a","b"]}}"#,
        r#"{"Question":{"question":"q","choices":["a"]}}"#,
    ] {
        let ask: Ask = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&ask).unwrap(), old);
    }
    let Ask::Approval(a) =
        serde_json::from_str(r#"{"Approval":{"tool":"Bash","title":"t","input":"ls"}}"#).unwrap()
    else {
        panic!()
    };
    assert_eq!((a.kind, a.options.len(), a.tool_id), (ApprovalKind::Other, 0, None));
}

#[test]
fn new_shapes_and_answers_round_trip() {
    let option = ApprovalOption { id: "rule-0".into(), label: "Always allow `pnpm add *`".into() };
    let asks = [
        Ask::Approval(Approval {
            tool: "Bash".into(),
            tool_id: Some("t1".into()),
            kind: ApprovalKind::Command,
            cwd: Some("/w".into()),
            source: Some("Explore".into()),
            options: vec![option.clone()],
            ..Approval::default()
        }),
        Ask::Questions(vec![Question {
            header: "DB".into(),
            question: "Which?".into(),
            multi: true,
            other: true,
            options: vec![Choice { label: "Postgres".into(), ..Choice::default() }],
            ..Question::default()
        }]),
        Ask::Form(Form {
            title: "Sign in".into(),
            url: Some("https://x.test".into()),
            timeout_ms: Some(30_000),
            fields: vec![Field {
                key: "k".into(),
                kind: FieldKind::Bool,
                required: true,
                when: vec![When { key: "a".into(), not: false, value: "yes".into() }],
                ..Field::default()
            }],
            ..Form::default()
        }),
        Ask::Plan(Plan {
            markdown: "# Plan\n- a".into(),
            options: vec![option],
            ..Plan::default()
        }),
    ];
    for ask in asks {
        assert_eq!(
            serde_json::from_str::<Ask>(&serde_json::to_string(&ask).unwrap()).unwrap(),
            ask
        );
    }
    for answer in [
        Answer::Pick("rule-0".into()),
        Answer::DenyWith("use yarn".into()),
        Answer::Answers(vec![vec!["Postgres".into()], vec![]]),
        Answer::Form(vec![("k".into(), vec!["true".into()])]),
    ] {
        assert_eq!(
            serde_json::from_str::<Answer>(&serde_json::to_string(&answer).unwrap()).unwrap(),
            answer
        );
    }
}

#[test]
fn unknown_kinds_load_as_unknown() {
    let a: Approval =
        serde_json::from_str(r#"{"tool":"x","title":"t","input":"i","kind":"Teleport"}"#).unwrap();
    assert_eq!(a.kind, ApprovalKind::Unknown);
    let f: Field = serde_json::from_str(r#"{"key":"k","kind":"Slider"}"#).unwrap();
    assert_eq!(f.kind, FieldKind::Unknown);
}

fn choice(label: &str, value: &str) -> Choice {
    Choice { label: label.into(), value: value.into(), ..Choice::default() }
}

#[test]
fn form_answer_requires_visible_required_fields() {
    let form = Form {
        fields: vec![
            Field {
                key: "mode".into(),
                kind: FieldKind::Choice,
                required: true,
                options: vec![choice("Fast", "fast"), choice("Safe", "safe")],
                ..Field::default()
            },
            Field {
                key: "why".into(),
                label: "Reason".into(),
                required: true,
                when: vec![When { key: "mode".into(), not: false, value: "safe".into() }],
                ..Field::default()
            },
        ],
        ..Form::default()
    };
    assert_eq!(
        form_answer(&form, &[vec!["fast".into()], vec![]]),
        Ok(Answer::Form(vec![("mode".into(), vec!["fast".into()])]))
    );
    assert_eq!(form_answer(&form, &[vec!["safe".into()], vec![]]), Err("Fill in Reason".into()));
    assert_eq!(form_answer(&form, &[vec![], vec![]]), Err("Fill in mode".into()));
    assert!(!field_visible(&form.fields[1], &form.fields, &[vec!["fast".into()], vec![]]));
}

#[test]
fn a_number_field_takes_only_numbers() {
    let field = Field {
        key: "n".into(),
        label: "Count".into(),
        kind: FieldKind::Number,
        ..Field::default()
    };
    let form = Form { fields: vec![field], ..Form::default() };
    assert_eq!(form_answer(&form, &[vec!["lots".into()]]), Err("Count must be a number".into()));
    assert_eq!(
        form_answer(&form, &[vec![" 2.5 ".into()]]),
        Ok(Answer::Form(vec![("n".into(), vec!["2.5".into()])]))
    );
}
