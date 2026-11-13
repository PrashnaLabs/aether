use super::*;

#[test]
fn activity_line_is_the_first_non_empty_line() {
    assert_eq!(activity_line("\n  \n  Reviewing #5  \nmore"), "Reviewing #5");
    assert_eq!(activity_line(""), "");
    assert_eq!(money(4.1), "$4.10");
}

fn contrast(a: u32, b: u32) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn actual_role_palette_has_wcag_aa_avatar_text() {
    for role in crate::org::model::ROLES.iter().map(|role| role.0).chain(["CUSTOM"]) {
        let background = role_color(role);
        let foreground = avatar_foreground(background);
        let ratio = contrast(background, foreground);
        assert!(ratio >= 4.5, "{role}: {ratio:.3}:1");
    }
}

#[test]
fn avatar_foreground_adapts_to_dark_and_light_colors() {
    assert_eq!(avatar_foreground(0x000000), 0xffffff);
    assert_eq!(avatar_foreground(0xffffff), 0x000000);
    // Include every grayscale value, especially near the black/white crossover.
    for channel in 0..=255 {
        let background = channel * 0x010101;
        assert!(contrast(background, avatar_foreground(background)) >= 4.5);
    }
}

#[test]
fn every_status_has_a_distinct_icon_and_explicit_label() {
    let cases = [
        (ChatStatus::Idle, "Idle"),
        (ChatStatus::Starting, "Starting"),
        (ChatStatus::Working { since_ms: 0 }, "Working"),
        (ChatStatus::Waiting, "Needs you"),
        (ChatStatus::Retrying, "Retrying"),
        (ChatStatus::Quiet { since_ms: 0, turn_ms: 0 }, "No output"),
        (ChatStatus::Background(3), "3 tasks in background"),
        (ChatStatus::Done, "New reply"),
        (ChatStatus::Failed, "Failed"),
    ];
    let mut icons = std::collections::HashSet::new();
    for (status, expected) in cases {
        let (icon, label, _) = status_visual(status);
        assert_eq!(label, expected);
        assert!(icons.insert(icon.path()), "duplicate icon for {label}");
    }
}

fn header_chat(id: i64, parent: Option<i64>, busy: bool) -> Chat {
    let mut chat = crate::app::chat_with("header test", &[]);
    chat.id = id;
    chat.parent = parent;
    chat.cancel = busy.then(aether_agent::Cancel::inert);
    chat
}

#[test]
fn header_busy_count_is_scoped_to_live_heads_and_workers_of_this_org() {
    let org =
        Org { heads: vec![Head { chat: Some(1), ..Default::default() }], ..Default::default() };
    let mut chats = vec![
        header_chat(1, None, true),
        header_chat(2, Some(1), true),
        header_chat(3, None, true),
        header_chat(4, Some(3), true),
        header_chat(5, Some(1), false),
        header_chat(6, Some(2), true),
        header_chat(7, Some(1), true),
    ];
    chats[6].archived = true;
    assert_eq!(org_busy_counts(&org, &chats), (1, 1));
    // Another org's head and worker must not make this org look busy.
    let other =
        Org { heads: vec![Head { chat: Some(3), ..Default::default() }], ..Default::default() };
    assert_eq!(org_busy_counts(&other, &chats), (1, 1));
    chats[0].cancel = None;
    chats[1].cancel = None;
    assert_eq!(org_busy_counts(&org, &chats), (0, 0));
    assert_eq!(org_busy_counts(&Org::default(), &chats), (0, 0));
    chats[4].queue.push(crate::app::Queued {
        text: "queued steer".into(),
        attachments: vec![],
        delivering: true,
        from_children: false,
        deliveries: vec![],
    });
    assert_eq!(org_busy_counts(&org, &chats), (0, 1));
    chats[4].queue[0].delivering = false;
    assert_eq!(org_busy_counts(&org, &chats), (0, 0));
}

#[gpui_kit::test]
fn header_exports_busy_count_or_idle_without_inventing_paused_state(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::Element;
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        let org =
            Org { heads: vec![Head { chat: Some(1), ..Default::default() }], ..Default::default() };
        let mut chats = vec![
            header_chat(1, None, true),
            header_chat(2, Some(1), true),
            header_chat(3, None, true),
        ];
        for (busy, label, description) in [
            (true, "2 busy", "1 worker busy · 1 head busy"),
            (false, "idle", "0 workers busy · 0 heads busy"),
        ] {
            if !busy {
                chats[0].cancel = None;
                chats[1].cancel = None;
            }
            let element = org_header_status(&org, &chats, cx).into_element();
            assert_eq!(Element::id(&element), Some("org-header-status".into()));
            assert_eq!(element.a11y_role(), Some(Role::Status));
            let mut node = gpui_kit::accesskit::Node::new(Role::Status);
            element.write_a11y_info(&mut node);
            assert_eq!(node.value(), Some(label));
            assert_eq!(node.label(), Some(format!("Organization {label}").as_str()));
            assert_eq!(node.description(), Some(description));
            assert_eq!(node.live(), None);
        }
    });
}
