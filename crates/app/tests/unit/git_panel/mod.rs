use super::*;

#[test]
fn sidebar_cache_retries_transient_errors_but_keeps_not_repo() {
    use StatusError::*;
    let folder = PathBuf::from("/repo");
    let folders = [folder.clone()];
    let mut statuses: HashMap<PathBuf, Loaded> = HashMap::new();
    let idle: HashSet<PathBuf> = HashSet::new();
    let cached = |status| Loaded { status, at: 0 };

    assert_eq!(sidebar_pending(&statuses, &idle, &folders), vec![folder.clone()]);
    assert_eq!(sidebar_cached_summary(&statuses, &folder), None);

    for transient in [Locked, Failed("boom".into())] {
        statuses.insert(folder.clone(), cached(Err(transient)));
        assert_eq!(sidebar_pending(&statuses, &idle, &folders), vec![folder.clone()]);
        assert_eq!(sidebar_cached_summary(&statuses, &folder), None);
    }

    statuses.insert(folder.clone(), cached(Err(NotRepo)));
    assert!(sidebar_pending(&statuses, &idle, &folders).is_empty());
    assert_eq!(sidebar_cached_summary(&statuses, &folder), None);

    statuses.insert(
        folder.clone(),
        cached(Ok(Status {
            changes: vec![change("a.rs", ChangeKind::Modified, false)],
            branch: Some("aether/fix".into()),
            commit: Some("abc1234".into()),
            upstream: Some("origin/aether/fix".into()),
            ahead_behind: Some((1, 0)),
            ..Default::default()
        })),
    );
    assert!(sidebar_pending(&statuses, &idle, &folders).is_empty());
    assert_eq!(sidebar_cached_summary(&statuses, &folder), Some((1, 0, 0, Some((1, 0)))));

    let mut inflight: HashSet<PathBuf> = HashSet::new();
    inflight.insert(folder.clone());
    statuses.clear();
    assert!(sidebar_pending(&statuses, &inflight, &folders).is_empty());
}
