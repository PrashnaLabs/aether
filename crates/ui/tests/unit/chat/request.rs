use super::*;

#[test]
fn plain_text_drops_markdown_syntax() {
    assert_eq!(
        plain_text("**1. Algorithms** — what's a `HashMap`?\n\n- [docs](https://x.y)\n- *two*"),
        "1. Algorithms — what's a HashMap? docs two"
    );
}

#[test]
fn a_choice_splits_off_its_description() {
    let choice = RequestChoice::new("c", 1, "Red — the warm one");
    assert_eq!(
        (choice.label.as_ref(), choice.description.as_deref()),
        ("Red", Some("the warm one"))
    );
    let plain = RequestChoice::new("c", 2, "Blue");
    assert_eq!((plain.label.as_ref(), plain.description), ("Blue", None));
}
