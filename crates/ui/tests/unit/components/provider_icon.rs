use super::*;

#[test]
fn paths_are_distinct() {
    let mut paths: Vec<_> = Provider::ALL.map(provider_icon_path).to_vec();
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), Provider::ALL.len());
}
