use super::*;

#[test]
fn templates_and_roles_match_the_prototype() {
    assert_eq!(TEMPLATES.map(|t| t.id), ["solo", "startup", "company", "custom"]);
    assert_eq!(template("startup").unwrap().roles, ["CEO", "ENG", "PRODUCT", "SEC"]);
    assert_eq!(template("company").unwrap().roles.len(), 8);
    for role in template("company").unwrap().roles {
        assert!(ROLES.iter().any(|r| r.0 == *role), "{role}");
        assert_ne!(role_color(role), 0x888888);
    }
    let eng = Head::new("ENG", 2);
    assert_eq!(
        (eng.title.as_str(), eng.provider, eng.model.as_str()),
        ("Engineering", Provider::Codex, "gpt-6.1-sol")
    );
    assert_eq!((eng.reviewer.as_deref(), eng.max_workers), (Some("SEC"), 2));
    assert_eq!(Head::new("CEO", 2).reviewer, None);
    assert_eq!(eng.chat_title(), "[ENG] Engineering");
    let custom = Head::new("LEGAL", 1);
    assert_eq!((custom.title.as_str(), custom.provider), ("LEGAL", Provider::Claude));
    assert!(models(Provider::Codex).contains(&"gpt-6-luna"));
    assert_eq!(role_badge("PRODUCT"), "PM");
    assert_eq!(role_badge("DESIGN"), "DES");
}

#[test]
fn old_org_files_with_a_budget_still_load() {
    let json = r#"{"project":"/p","heads":[{"role":"ENG","budget_usd":10.0,"effort":"high"}]}"#;
    let org: Org = serde_json::from_str(json).unwrap();
    assert_eq!((org.heads[0].role.as_str(), org.heads[0].effort.as_str()), ("ENG", "high"));
}

#[test]
fn spec_from_template_falls_back_to_a_ceo() {
    let spec = OrgSpec::from_template("solo", 3);
    assert_eq!(spec.heads.iter().map(|h| h.role.as_str()).collect::<Vec<_>>(), ["CEO", "ENG"]);
    assert!(spec.heads.iter().all(|h| h.max_workers == 3));
    assert_eq!(OrgSpec::from_template("nope", 2).heads.len(), 1);
}

#[test]
fn org_round_trips_through_json_and_tolerates_missing_fields() {
    let mut org = Org { project: "/p".into(), goal: "ship".into(), ..Org::default() };
    org.heads.push(Head { chat: Some(7), ..Head::new("CEO", 2) });
    org.requests.push(Request {
        id: 3,
        head: "CEO".into(),
        checks: vec![("x".into(), true)],
        ..Request::default()
    });
    let back: Org = serde_json::from_str(&serde_json::to_string(&org).unwrap()).unwrap();
    assert_eq!(back, org);
    let old: Org = serde_json::from_str(r#"{"project":"/p","heads":[{"role":"ENG"}]}"#).unwrap();
    assert_eq!(old.heads[0].role, "ENG");
    assert!(old.requests.is_empty());
    assert_eq!(org.next_request_id(), 4);
    assert_eq!(Org::default().next_request_id(), 1);
    assert_eq!(org.ceo_chat(), Some(7));
    assert_eq!(org.head_of_chat(7).unwrap().role, "CEO");
}

#[test]
fn slug_is_stable_and_path_safe() {
    assert_eq!(
        slug("/Users/iris/Downloads/xzx/PERSONAL/aether"),
        "users-iris-downloads-xzx-personal-aether"
    );
    assert_eq!(slug("C:\\work\\my app\\"), "c-work-my-app");
    assert_eq!(slug("/"), "project");
}

#[test]
fn role_file_and_onboarding_name_the_files_and_ceo() {
    let dir = Path::new("/cfg/org/p");
    let mut org = Org::default();
    org.heads.push(Head { chat: Some(11), ..Head::new("CEO", 2) });
    org.heads.push(Head { chat: Some(12), ..Head::new("ENG", 2) });
    let eng = org.heads[1].clone();
    let role = role_file_text(&eng);
    assert!(role.starts_with("# Role: Engineering"));
    assert!(role.contains("memory/eng.md"));
    let prompt = onboarding_prompt(&org, &eng, &role, dir);
    assert!(prompt.contains("CEO, chat 11"));
    assert!(prompt.contains(&role_path(dir, "ENG").display().to_string()));
    assert!(prompt.contains(&memory_path(dir, "ENG").display().to_string()));
    assert!(prompt.contains("reviewed by SEC"));
    assert!(prompt.contains("# Role: Engineering"));
    let ceo = onboarding_prompt(&org, &org.heads[0], "", dir);
    assert!(ceo.contains("You are the CEO"));
    assert!(ceo.contains("Engineering (ENG): chat 12"));
}

#[test]
fn hire_checks_apply_the_three_rules() {
    let eng = Head::new("ENG", 2);
    let ok = hire_checks(&eng, Some(6), 1, 1, 3);
    assert!(ok.iter().all(|c| c.1));
    assert_eq!(ok[0].0, "Has card #6");
    let bad = hire_checks(&eng, None, 2, 3, 3);
    assert_eq!(bad.iter().map(|c| c.1).collect::<Vec<_>>(), [false, false, false]);
    assert_eq!(bad[1].0, "ENG at 2 of 2 workers");
    assert_eq!(bad[2].0, "Org pool 3/3 busy");
}

#[test]
fn pool_cap_auto_sizes_from_the_machine() {
    let mut s = Settings::default();
    assert_eq!(pool_cap(&s, 8, 16), 3);
    assert_eq!(pool_cap(&s, 16, 64), 8);
    assert_eq!(pool_cap(&s, 1, 2), 1);
    s.org_pool = Some(5);
    assert_eq!(pool_cap(&s, 8, 16), 5);
    s.org_pool = Some(0);
    assert_eq!(pool_cap(&s, 8, 16), 1);
}

#[test]
fn parse_from_reads_sender_through_bracketed_titles() {
    assert_eq!(
        parse_from("[from [ENG] Engineering #42] ready for review"),
        Some((42, "ready for review"))
    );
    assert_eq!(parse_from("[from Lead #7]hi"), Some((7, "hi")));
    assert_eq!(parse_from("plain text"), None);
    assert_eq!(parse_from("[from nobody] x"), None);
}

#[test]
fn hire_policy_decides_from_the_checks() {
    let pass = [("a".to_string(), true), ("b".to_string(), true)];
    let fail = [("a".to_string(), true), ("b".to_string(), false)];
    let cases = [
        (OrgHire::Rules, &pass, HireOutcome::Hire),
        (OrgHire::Rules, &fail, HireOutcome::Deny),
        (OrgHire::Auto, &pass, HireOutcome::Hire),
        (OrgHire::Auto, &fail, HireOutcome::Pending),
        (OrgHire::Ask, &pass, HireOutcome::Pending),
        (OrgHire::Ask, &fail, HireOutcome::Pending),
    ];
    for (policy, checks, want) in cases {
        assert_eq!(hire_outcome(policy, checks), want, "{policy:?}");
    }
}

#[test]
fn card_labels_pick_who_to_wake() {
    let head = |role: &str, chat: i64| Head { chat: Some(chat), ..Head::new(role, 2) };
    let org = Org { heads: vec![head("CEO", 1), head("ENG", 2), head("SEC", 3)], ..Org::default() };
    let labels = ["dept:eng".to_string(), "review:sec".to_string()];
    assert_eq!(org.labelled(&labels, "review"), Some(3));
    assert_eq!(org.labelled(&labels, "dept"), Some(2));
    assert_eq!(org.labelled(&["review:qa".to_string()], "review"), None);
    assert_eq!(org.note_wake(&labels, 3), [2, 1]);
    assert_eq!(org.note_wake(&labels, 2), [1]);
    assert_eq!(org.note_wake(&[], 1), Vec::<i64>::new());
}
