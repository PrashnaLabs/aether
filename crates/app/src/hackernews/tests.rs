#[cfg(test)]
mod tests {
    use crate::hackernews::HackerNewsModel;
    use crate::hackernews::types::{
        HnCategory, HnState, HnStory, PrototypeStateOverride, sample_stories,
    };

    #[test]
    fn test_hn_story_domain_extraction() {
        let story_with_url = HnStory {
            id: 1,
            title: "Test".to_string(),
            by: "author".to_string(),
            score: 10,
            time: 1000,
            url: Some("https://github.com/aether/aether-workspace".to_string()),
            comments_count: 5,
            text: None,
        };
        assert_eq!(story_with_url.domain(), "github.com");

        let story_with_www = HnStory {
            id: 2,
            title: "Test 2".to_string(),
            by: "author".to_string(),
            score: 10,
            time: 1000,
            url: Some("https://www.nytimes.com/tech".to_string()),
            comments_count: 5,
            text: None,
        };
        assert_eq!(story_with_www.domain(), "nytimes.com");

        let ask_hn = HnStory {
            id: 3,
            title: "Ask HN".to_string(),
            by: "author".to_string(),
            score: 10,
            time: 1000,
            url: None,
            comments_count: 5,
            text: Some("Question text".to_string()),
        };
        assert_eq!(ask_hn.domain(), "news.ycombinator.com");
    }

    #[test]
    fn test_hn_story_time_ago() {
        let now = 10000;
        let s1 = HnStory {
            id: 1,
            title: "T".into(),
            by: "a".into(),
            score: 1,
            time: now - 30,
            url: None,
            comments_count: 0,
            text: None,
        };
        assert_eq!(s1.time_ago(now), "just now");

        let s2 = HnStory {
            id: 2,
            title: "T".into(),
            by: "a".into(),
            score: 1,
            time: now - 300,
            url: None,
            comments_count: 0,
            text: None,
        };
        assert_eq!(s2.time_ago(now), "5m ago");

        let s3 = HnStory {
            id: 3,
            title: "T".into(),
            by: "a".into(),
            score: 1,
            time: now - 7200,
            url: None,
            comments_count: 0,
            text: None,
        };
        assert_eq!(s3.time_ago(now), "2h ago");

        let s4 = HnStory {
            id: 4,
            title: "T".into(),
            by: "a".into(),
            score: 1,
            time: now - 172800,
            url: None,
            comments_count: 0,
            text: None,
        };
        assert_eq!(s4.time_ago(now), "2d ago");
    }

    #[test]
    fn test_hn_category_endpoints() {
        for cat in HnCategory::ALL {
            assert!(!cat.feed_endpoint().is_empty());
            assert!(!cat.label().is_empty());
        }
    }

    #[test]
    fn test_hackernews_model_filtering() {
        let mut model = HackerNewsModel::new();
        model.stories = sample_stories();
        assert!(!model.stories.is_empty());

        model.search_query = "Rust".to_string();
        let filtered = model.filtered_stories();
        assert!(!filtered.is_empty());
        for story in filtered {
            let matches = story.title.to_lowercase().contains("rust")
                || story.by.to_lowercase().contains("rust")
                || story.domain().to_lowercase().contains("rust");
            assert!(matches);
        }

        model.search_query = "xyznonexistentquery999".to_string();
        assert!(model.filtered_stories().is_empty());

        model.search_query.clear();
        assert_eq!(model.filtered_stories().len(), model.stories.len());
    }

    #[test]
    fn test_prototype_state_overrides() {
        let mut model = HackerNewsModel::new();
        model.state = HnState::Loaded;
        assert_eq!(model.effective_state(), HnState::Loaded);

        model.state_override = PrototypeStateOverride::Loading;
        assert_eq!(model.effective_state(), HnState::Loading);

        model.state_override = PrototypeStateOverride::Fetching;
        assert_eq!(model.effective_state(), HnState::Fetching);

        model.state_override = PrototypeStateOverride::Error;
        assert!(matches!(model.effective_state(), HnState::Error(_)));

        model.state_override = PrototypeStateOverride::Empty;
        assert_eq!(model.effective_state(), HnState::Empty);

        model.state_override = PrototypeStateOverride::Auto;
        assert_eq!(model.effective_state(), HnState::Loaded);
    }

    #[test]
    fn test_empty_state_on_unmatched_search() {
        let mut model = HackerNewsModel::new();
        model.stories = sample_stories();
        model.state = HnState::Loaded;
        assert_eq!(model.effective_state(), HnState::Loaded);

        model.search_query = "completelyunmatchedquerystring123".to_string();
        assert_eq!(model.effective_state(), HnState::Empty);

        model.search_query.clear();
        assert_eq!(model.effective_state(), HnState::Loaded);
    }

    #[test]
    fn test_category_cache_storage() {
        let mut model = HackerNewsModel::new();
        let stories = sample_stories();
        let now = std::time::Instant::now();

        model.stories = stories.clone();
        model.last_fetch = Some(now);
        model.story_ids = vec![42, 43];
        model.cache_current();
        let cached = &model.category_cache[&HnCategory::Top];
        assert_eq!(cached.stories.len(), stories.len());
        assert_eq!(cached.page, 0);
        assert!(cached.has_more);
        assert_eq!(cached.ids, vec![42, 43]);
    }

    #[test]
    fn test_story_urls_and_discussions() {
        let story_with_url = HnStory {
            id: 42,
            title: "Title".into(),
            by: "dev".into(),
            score: 100,
            time: 12345,
            url: Some("https://example.com/blog".into()),
            comments_count: 10,
            text: None,
        };
        assert_eq!(story_with_url.link_url(), "https://example.com/blog");
        assert_eq!(story_with_url.discussion_url(), "https://news.ycombinator.com/item?id=42");

        let ask_hn = HnStory {
            id: 99,
            title: "Ask HN: Ideas?".into(),
            by: "asker".into(),
            score: 25,
            time: 12345,
            url: None,
            comments_count: 15,
            text: Some("Text".into()),
        };
        assert_eq!(ask_hn.link_url(), "https://news.ycombinator.com/item?id=99");
        assert_eq!(ask_hn.discussion_url(), "https://news.ycombinator.com/item?id=99");
    }
}
