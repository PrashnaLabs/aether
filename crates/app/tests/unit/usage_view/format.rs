use super::*;

#[test]
fn token_counts_compact_to_three_significant_figures() {
    assert_eq!(format_tokens_compact(804_000.0), "804K");
    assert_eq!(format_tokens_compact(76_700_000.0), "76.7M");
    assert_eq!(format_tokens_compact(19_900_000_000.0), "19.9B");
    assert_eq!(format_tokens_compact(950.0), "950");
    assert_eq!(format_tokens_compact(1_000.0), "1K");
    assert_eq!(format_tokens_compact(1_500.0), "1.50K");
}

#[test]
fn currency_groups_thousands() {
    assert_eq!(format_usd(0.0), "$0.00");
    assert_eq!(format_usd(1_234.5), "$1,234.50");
    assert_eq!(format_usd(1_234_567.891), "$1,234,567.89");
}

#[test]
fn project_paths_only_shorten_the_home_prefix() {
    let home = Path::new("/Users/developer");

    assert_eq!(
        usage_project_path(Path::new("/Users/developer/dev/aether"), Some(home)),
        "~/dev/aether"
    );
    assert_eq!(usage_project_path(home, Some(home)), "~");
    assert_eq!(
        usage_project_path(Path::new("/Users/developer-2/aether"), Some(home)),
        "/Users/developer-2/aether"
    );
    assert_eq!(
        usage_project_path(Path::new("/Volumes/work/aether"), Some(home)),
        "/Volumes/work/aether"
    );
}
