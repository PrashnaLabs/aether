use super::*;

#[test]
fn evicts_the_least_recently_used_entry_past_its_cap() {
    let mut cache = RenderCache::new(2);
    cache.insert("a".into(), 1);
    cache.insert("b".into(), 2);
    assert_eq!(cache.get(&"a".into()).copied(), Some(1));
    cache.insert("c".into(), 3);
    assert!(cache.get(&"b".into()).is_none(), "b was the least recently used");
    assert_eq!(
        (cache.get(&"a".into()).copied(), cache.get(&"c".into()).copied()),
        (Some(1), Some(3))
    );
}
