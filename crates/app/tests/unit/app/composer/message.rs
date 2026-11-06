use super::*;

fn thinking(ms: Option<u64>) -> Step {
    Step::Thinking { text: String::new(), ms }
}

#[test]
fn only_the_last_open_thought_is_live() {
    let steps = [thinking(None), thinking(Some(5)), thinking(None)];
    assert!(!is_live_thought(&steps, 0));
    assert!(is_live_thought(&steps, 2));
}
