pub mod client;
pub mod types;
pub mod view;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::Instant;
pub use types::{HnCategory, HnState, HnStory, PrototypeStateOverride, sample_stories};

#[derive(Clone)]
pub struct CachedFeed {
    pub stories: Vec<HnStory>,
    pub fetched_at: Instant,
    pub page: usize,
    pub has_more: bool,
    pub ids: Vec<u64>,
}

pub struct HackerNewsModel {
    pub stories: Vec<HnStory>,
    pub category: HnCategory,
    pub state: HnState,
    pub state_override: PrototypeStateOverride,
    pub search_query: String,
    pub last_fetch: Option<Instant>,
    pub page: usize,
    pub has_more: bool,
    pub loading_more: bool,
    pub category_cache: HashMap<HnCategory, CachedFeed>,
    pub story_ids: Vec<u64>,
    pub generation: u64,
    pub pagination_error: Option<String>,
    pub scroll: gpui_kit::ScrollHandle,
}

impl Default for HackerNewsModel {
    fn default() -> Self {
        Self {
            stories: Vec::new(),
            category: HnCategory::Top,
            state: HnState::Loading,
            state_override: PrototypeStateOverride::Auto,
            search_query: String::new(),
            last_fetch: None,
            page: 0,
            has_more: true,
            loading_more: false,
            category_cache: HashMap::new(),
            story_ids: Vec::new(),
            generation: 0,
            pagination_error: None,
            scroll: gpui_kit::ScrollHandle::new(),
        }
    }
}

impl HackerNewsModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate_requests(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.loading_more = false;
        self.pagination_error = None;
    }

    pub fn accepts_response(&self, category: HnCategory, generation: u64) -> bool {
        self.category == category && self.generation == generation
    }

    pub fn can_load_more(&self) -> bool {
        self.has_more
            && !self.loading_more
            && self.pagination_error.is_none()
            && self.search_query.trim().is_empty()
            && matches!(self.state, HnState::Loaded)
            && self.state_override == PrototypeStateOverride::Auto
    }

    pub fn cache_current(&mut self) {
        if let Some(fetched_at) = self.last_fetch {
            self.category_cache.insert(
                self.category,
                CachedFeed {
                    stories: self.stories.clone(),
                    fetched_at,
                    page: self.page,
                    has_more: self.has_more,
                    ids: self.story_ids.clone(),
                },
            );
        }
    }

    pub fn apply_page(&mut self, page: usize, result: client::StoryPage) {
        if page == 0 {
            self.stories.clear();
            self.scroll.set_offset(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)));
            self.last_fetch = Some(Instant::now());
        }
        let mut seen: std::collections::HashSet<_> = self.stories.iter().map(|s| s.id).collect();
        self.stories.extend(result.stories.into_iter().filter(|s| seen.insert(s.id)));
        self.story_ids = result.ids;
        self.page = page;
        self.has_more = result.has_more;
        self.loading_more = false;
        self.pagination_error = None;
        self.state = if self.stories.is_empty() && !self.has_more {
            HnState::Empty
        } else {
            HnState::Loaded
        };
        self.cache_current();
    }

    pub fn load_sample(&mut self) {
        self.invalidate_requests();
        self.has_more = false;
        self.story_ids.clear();
        self.stories = sample_stories();
        self.state = HnState::Loaded;
    }

    pub fn effective_state(&self) -> HnState {
        match self.state_override {
            PrototypeStateOverride::Auto => {
                if matches!(self.state, HnState::Loaded)
                    && !self.search_query.trim().is_empty()
                    && self.filtered_stories().is_empty()
                {
                    HnState::Empty
                } else {
                    self.state.clone()
                }
            }
            PrototypeStateOverride::Loading => HnState::Loading,
            PrototypeStateOverride::Fetching => HnState::Fetching,
            PrototypeStateOverride::Loaded => HnState::Loaded,
            PrototypeStateOverride::Error => HnState::Error(
                "Failed to fetch stories: Connection refused by upstream API (mock demonstration)"
                    .to_string(),
            ),
            PrototypeStateOverride::Empty => HnState::Empty,
        }
    }

    pub fn filtered_stories(&self) -> Vec<HnStory> {
        let q = self.search_query.trim().to_lowercase();
        if q.is_empty() {
            return self.stories.clone();
        }
        self.stories
            .iter()
            .filter(|s| {
                s.title.to_lowercase().contains(&q)
                    || s.by.to_lowercase().contains(&q)
                    || s.domain().to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/hackernews/model.rs"]
mod model_tests;
