use super::*;

fn m(ix: usize, status: ChatStatus, activity: i64) -> CrewMember {
    CrewMember { ix, status, activity, keep: false, summary: None }
}

fn working() -> ChatStatus {
    ChatStatus::Working { since_ms: 0 }
}

#[test]
fn words_omit_zero_buckets_and_follow_tier() {
    let s = CrewSummary { needs_you: 1, failed: 1, working: 2, done: 12 };
    assert_eq!(s.words(Tier::Wide), "1 needs you, 1 failed, 2 working, 12 done");
    assert_eq!(s.words(Tier::Mid), "1 needs you, 1 failed, 2 working");
    assert_eq!(CrewSummary { done: 3, ..Default::default() }.words(Tier::Mid), "3 done");
}

#[test]
fn tiers_split_at_235_and_380() {
    assert_eq!(Tier::of(234.), Tier::Narrow);
    assert_eq!(Tier::of(255.), Tier::Mid);
    assert_eq!(Tier::of(380.), Tier::Wide);
}

#[test]
fn order_puts_needs_you_then_failed_then_working_then_done_newest_first() {
    let mut v = vec![
        m(0, ChatStatus::Done, 9),
        m(1, working(), 1),
        m(2, ChatStatus::Failed, 1),
        m(3, ChatStatus::Waiting, 1),
        m(4, working(), 5),
    ];
    order(&mut v);
    assert_eq!(v.iter().map(|x| x.ix).collect::<Vec<_>>(), [3, 2, 4, 1, 0]);
}

#[test]
fn sub_lead_sorts_by_worst_descendant() {
    let mut lead = m(0, ChatStatus::Done, 1);
    lead.summary = Some(CrewSummary { working: 1, ..Default::default() });
    let mut v = vec![m(1, ChatStatus::Done, 9), lead];
    order(&mut v);
    assert_eq!(v[0].ix, 0);
}

#[test]
fn preview_collapsed_shows_only_kept() {
    let mut v: Vec<_> = (0..15).map(|i| m(i, ChatStatus::Done, 0)).collect();
    v[3].keep = true;
    let p = preview(&v, false, false);
    assert_eq!(p.visible.iter().filter(|x| **x).count(), 1);
    assert_eq!(p.hidden.total(), 14);
}

#[test]
fn preview_collapsed_hides_busy_children_too() {
    let v: Vec<_> = (0..4).map(|i| m(i, working(), 0)).collect();
    assert_eq!(preview(&v, false, false).visible.iter().filter(|x| **x).count(), 0);
}

#[test]
fn preview_expanded_shows_attention_plus_five() {
    let mut v = vec![m(0, ChatStatus::Waiting, 0), m(1, ChatStatus::Failed, 0)];
    v.extend((2..4).map(|i| m(i, working(), 0)));
    v.extend((4..15).map(|i| m(i, ChatStatus::Done, 0)));
    let p = preview(&v, true, false);
    assert_eq!(p.visible.iter().filter(|x| **x).count(), 7);
    assert_eq!(p.hidden.done, 8);
}

#[test]
fn preview_show_all_shows_every_child() {
    let v: Vec<_> = (0..12).map(|i| m(i, ChatStatus::Done, 0)).collect();
    assert_eq!(preview(&v, true, true).hidden.total(), 0);
}

#[test]
fn crews_over_five_start_collapsed() {
    assert!(!starts_collapsed(5));
    assert!(starts_collapsed(6));
}

#[test]
fn preview_expanded_cap_picks_by_priority_then_newest() {
    let mut v = vec![m(0, ChatStatus::Waiting, 0)];
    v.extend((1..=3).map(|i| m(i, working(), i as i64)));
    v.extend((4..=10).map(|i| m(i, ChatStatus::Done, i as i64)));
    let p = preview(&v, true, false);
    let shown: Vec<_> = (0..v.len()).filter(|&i| p.visible[i]).collect();
    assert_eq!(shown, [0, 1, 2, 3, 9, 10]);
}
