use super::*;

#[test]
fn quote_leads_the_typed_text_as_a_blockquote() {
    assert_eq!(quoted(Some("first\nsecond"), "why?"), "> first\n> second\n\nwhy?");
}

#[test]
fn quote_alone_sends_just_the_blockquote() {
    assert_eq!(quoted(Some("a\n\nb"), ""), "> a\n> \n> b");
}

#[test]
fn no_quote_sends_the_typed_text() {
    assert_eq!(quoted(None, "hi"), "hi");
}

#[test]
fn explain_asks_about_the_quoted_selection() {
    assert_eq!(explain("x\ny"), "Explain this:\n\n> x\n> y");
}
