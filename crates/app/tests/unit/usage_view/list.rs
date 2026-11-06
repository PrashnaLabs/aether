use super::*;

#[test]
fn model_summary_names_the_hidden_count() {
    let models = vec![
        ("gpt-5.3-codex".to_owned(), 30.0),
        ("claude-opus-4-6".to_owned(), 20.0),
        ("gemini-2.5-pro".to_owned(), 10.0),
    ];

    assert_eq!(usage_top_models_label(&[]), None);
    assert_eq!(
        usage_top_models_label(&models[..2]).as_deref(),
        Some("gpt-5.3-codex · claude-opus-4-6")
    );
    assert_eq!(
        usage_top_models_label(&models).as_deref(),
        Some("gpt-5.3-codex · claude-opus-4-6 · +1")
    );
}
