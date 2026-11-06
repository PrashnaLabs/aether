use super::*;

#[test]
fn start_options_reads_provider_model_map() {
    let mut s = Settings::default();
    s.provider_models.insert("claude".into(), "sonnet".into());
    let opts = DriverStartOptions::for_provider(&s, Provider::Claude, "/tmp");
    assert_eq!(opts.model, "sonnet");
    assert_eq!(opts.binary, "");
    assert_eq!(opts.attachments, None);
}
