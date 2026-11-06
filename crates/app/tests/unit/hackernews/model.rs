use super::*;

#[test]
fn stale_requests_are_rejected_after_refresh_and_category_change() {
    let mut model = HackerNewsModel::new();
    let generation = model.generation;
    model.loading_more = true;
    model.invalidate_requests();
    assert!(!model.loading_more);
    assert!(!model.accepts_response(HnCategory::Top, generation));
    assert!(model.accepts_response(HnCategory::Top, model.generation));
    model.category = HnCategory::New;
    assert!(!model.accepts_response(HnCategory::Top, model.generation));
}

#[test]
fn pagination_pauses_for_loading_errors_search_and_exhaustion() {
    let mut model = HackerNewsModel::new();
    assert!(!model.can_load_more());
    model.state = HnState::Loaded;
    assert!(model.can_load_more());
    model.loading_more = true;
    assert!(!model.can_load_more());
    model.loading_more = false;
    model.pagination_error = Some("network unavailable".into());
    assert!(!model.can_load_more());
    model.pagination_error = None;
    assert!(model.can_load_more());
    model.search_query = "rust".into();
    assert!(!model.can_load_more());
    model.search_query.clear();
    model.has_more = false;
    assert!(!model.can_load_more());
}

#[test]
fn pagination_deduplicates_and_keeps_cursor_with_empty_pages() {
    let mut model = HackerNewsModel::new();
    let stories = sample_stories();
    let ids = vec![stories[0].id, stories[1].id];
    model.apply_page(
        0,
        client::StoryPage { stories: vec![stories[0].clone()], ids: ids.clone(), has_more: true },
    );
    model.apply_page(
        1,
        client::StoryPage {
            stories: vec![stories[0].clone(), stories[1].clone()],
            ids: ids.clone(),
            has_more: true,
        },
    );
    assert_eq!(model.stories.len(), 2);
    // A page of deleted items must advance the cursor, not end the feed.
    model.apply_page(2, client::StoryPage { stories: vec![], ids: ids.clone(), has_more: true });
    assert_eq!(model.page, 2);
    assert!(model.can_load_more());
    let cached = &model.category_cache[&HnCategory::Top];
    assert_eq!(cached.ids, ids);
    assert_eq!(cached.page, 2);
    model.apply_page(3, client::StoryPage { stories: vec![], ids, has_more: false });
    assert!(!model.can_load_more());
    assert_eq!(model.stories.len(), 2);
}
