use futures::{StreamExt, TryStreamExt, stream};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;

use super::types::{HnCategory, HnStory};

const API: &str = "https://hacker-news.firebaseio.com/v0";
pub const PAGE_SIZE: usize = 30;

pub struct StoryPage {
    pub stories: Vec<HnStory>,
    pub ids: Vec<u64>,
    pub has_more: bool,
}

#[derive(Deserialize)]
struct Item {
    id: u64,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    dead: bool,
    #[serde(rename = "type")]
    kind: Option<String>,
    title: Option<String>,
    by: Option<String>,
    score: Option<u32>,
    time: Option<i64>,
    url: Option<String>,
    descendants: Option<u32>,
    text: Option<String>,
}

impl Item {
    fn into_story(self) -> Option<HnStory> {
        if self.deleted
            || self.dead
            || !matches!(self.kind.as_deref(), Some("story" | "job" | "poll"))
        {
            return None;
        }
        let title = self.title.filter(|title| !title.trim().is_empty())?;
        Some(HnStory {
            id: self.id,
            title,
            by: self.by.unwrap_or_else(|| "anonymous".into()),
            score: self.score.unwrap_or(0),
            time: self.time.unwrap_or(0),
            url: self.url,
            comments_count: self.descendants.unwrap_or(0),
            text: self.text,
        })
    }
}

/// Later pages reuse the first response's ordered ID snapshot.
pub async fn fetch_stories(
    category: HnCategory,
    page: usize,
    ids: Vec<u64>,
) -> Result<StoryPage, String> {
    fetch_page(API.into(), category, page, ids).await
}

async fn fetch_page(
    base: String,
    category: HnCategory,
    page: usize,
    ids: Vec<u64>,
) -> Result<StoryPage, String> {
    // GPUI has no Tokio reactor, including for the timeout created by send().
    static RUNTIME: OnceLock<Result<Runtime, std::io::Error>> = OnceLock::new();
    let runtime = RUNTIME
        .get_or_init(Runtime::new)
        .as_ref()
        .map_err(|e| format!("Failed to start HTTP runtime: {e}"))?;
    runtime
        .spawn(fetch_response(base, category, page, ids))
        .await
        .map_err(|e| format!("HTTP request task failed: {e}"))?
}

async fn get<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: String,
) -> Result<T, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("HN API request failed: {e}"))?;
    let body = response.text().await.map_err(|e| format!("Failed to read HN response: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("Failed to parse HN response: {e}"))
}

async fn fetch_response(
    base: String,
    category: HnCategory,
    page: usize,
    mut ids: Vec<u64>,
) -> Result<StoryPage, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, reqwest::Error>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .user_agent("Aether-HackerNews/1.0")
                .build()
        })
        .as_ref()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
    if page == 0 {
        ids = get(client, format!("{base}/{}.json", category.feed_endpoint())).await?;
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(*id));
    }
    let start = page.saturating_mul(PAGE_SIZE).min(ids.len());
    let end = start.saturating_add(PAGE_SIZE).min(ids.len());
    // Bounded parallel requests retain the API's ranking order.
    let stories =
        stream::iter(ids[start..end].iter().copied().map(|id| {
            let url = format!("{base}/item/{id}.json");
            async move {
                get::<Option<Item>>(client, url).await.map(|item| item.and_then(Item::into_story))
            }
        }))
        .buffered(8)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect();
    Ok(StoryPage { stories, has_more: end < ids.len(), ids })
}

#[cfg(test)]
#[path = "../../tests/unit/hackernews/client.rs"]
mod tests;
