use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HnStory {
    pub id: u64,
    pub title: String,
    pub by: String,
    pub score: u32,
    pub time: i64,
    pub url: Option<String>,
    pub comments_count: u32,
    pub text: Option<String>,
}

impl HnStory {
    pub fn domain(&self) -> String {
        if let Some(ref url) = self.url
            && let Some(pos) = url.find("://")
        {
            let rest = &url[pos + 3..];
            let host = rest.split('/').next().unwrap_or(rest);
            let host = host.split(':').next().unwrap_or(host);
            return host.strip_prefix("www.").unwrap_or(host).to_string();
        }
        "news.ycombinator.com".to_string()
    }

    pub fn discussion_url(&self) -> String {
        format!("https://news.ycombinator.com/item?id={}", self.id)
    }

    pub fn link_url(&self) -> String {
        self.url.clone().unwrap_or_else(|| self.discussion_url())
    }

    pub fn time_ago(&self, now_ts: i64) -> String {
        let diff = (now_ts - self.time).max(0);
        if diff < 60 {
            "just now".to_string()
        } else if diff < 3600 {
            let m = diff / 60;
            format!("{m}m ago")
        } else if diff < 86400 {
            let h = diff / 3600;
            format!("{h}h ago")
        } else {
            let d = diff / 86400;
            format!("{d}d ago")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HnCategory {
    #[default]
    Top,
    Best,
    New,
    ShowHN,
    AskHN,
}

impl HnCategory {
    pub const ALL: [HnCategory; 5] =
        [HnCategory::Top, HnCategory::Best, HnCategory::New, HnCategory::ShowHN, HnCategory::AskHN];

    pub fn label(&self) -> &'static str {
        match self {
            HnCategory::Top => "Top Stories",
            HnCategory::Best => "Best",
            HnCategory::New => "Newest",
            HnCategory::ShowHN => "Show HN",
            HnCategory::AskHN => "Ask HN",
        }
    }

    pub fn feed_endpoint(&self) -> &'static str {
        match self {
            HnCategory::Top => "topstories",
            HnCategory::Best => "beststories",
            HnCategory::New => "newstories",
            HnCategory::ShowHN => "showstories",
            HnCategory::AskHN => "askstories",
        }
    }
}

/// The runtime / visual state of the Hacker News module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HnState {
    /// Initial load: shows skeleton placeholder cards
    Loading,
    /// Background refreshing: keeps existing stories while showing a progress banner & spinner
    Fetching,
    /// Stories loaded successfully
    Loaded,
    /// Network or API failure: shows error message with retry and offline fallback buttons
    Error(String),
    /// No stories matching current search filter or empty feed
    Empty,
}

/// Prototype state override allows pair programmers & reviewers to click and view every state immediately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum PrototypeStateOverride {
    #[default]
    Auto,
    Loading,
    Fetching,
    Loaded,
    Error,
    Empty,
}

pub fn sample_stories() -> Vec<HnStory> {
    let now = Utc::now().timestamp();
    vec![
        HnStory {
            id: 4192001,
            title: "Show HN: Aether – Native Rust desktop workspace for coding agents".to_string(),
            by: "alex_dev".to_string(),
            score: 742,
            time: now - 3600 * 2,
            url: Some("https://github.com/aether/aether-workspace".to_string()),
            comments_count: 218,
            text: None,
        },
        HnStory {
            id: 4192002,
            title: "Anthropic announces Claude 3.7 Sonnet with hybrid reasoning architecture".to_string(),
            by: "tptacek".to_string(),
            score: 1105,
            time: now - 3600 * 4,
            url: Some("https://anthropic.com/news/claude-3-7-sonnet".to_string()),
            comments_count: 532,
            text: None,
        },
        HnStory {
            id: 4192003,
            title: "SQLite is 35% faster than the filesystem for blob storage".to_string(),
            by: "drh".to_string(),
            score: 689,
            time: now - 3600 * 6,
            url: Some("https://sqlite.org/fasterthanfs.html".to_string()),
            comments_count: 310,
            text: None,
        },
        HnStory {
            id: 4192004,
            title: "Show HN: GPUI 2.0 – High-performance GPU-rendered desktop applications in Rust".to_string(),
            by: "nathansobo".to_string(),
            score: 894,
            time: now - 3600 * 7,
            url: Some("https://zed.dev/blog/gpui-technology".to_string()),
            comments_count: 341,
            text: None,
        },
        HnStory {
            id: 4192005,
            title: "Ask HN: What are your go-to tools for local LLM inference in 2026?".to_string(),
            by: "matrix_runner".to_string(),
            score: 345,
            time: now - 3600 * 9,
            url: None,
            comments_count: 247,
            text: Some("Looking to run 70B models locally with sub-second time-to-first-token. What hardware and runtime stack is currently performing best for pair-programming?".to_string()),
        },
        HnStory {
            id: 4192006,
            title: "Linux 6.18 released with real-time PREEMPT_RT enabled by default".to_string(),
            by: "torvalds_fan".to_string(),
            score: 512,
            time: now - 3600 * 12,
            url: Some("https://kernelnewbies.org/Linux_6_18".to_string()),
            comments_count: 156,
            text: None,
        },
        HnStory {
            id: 4192007,
            title: "Why we rewritten our distributed consensus engine from Go to Rust".to_string(),
            by: "infra_lead".to_string(),
            score: 620,
            time: now - 3600 * 15,
            url: Some("https://blog.distributed-systems.io/rewriting-in-rust".to_string()),
            comments_count: 403,
            text: None,
        },
        HnStory {
            id: 4192008,
            title: "Google DeepMind open sources new agent evaluation sandbox".to_string(),
            by: "demis_h".to_string(),
            score: 418,
            time: now - 3600 * 18,
            url: Some("https://deepmind.google/research/agent-eval-sandbox".to_string()),
            comments_count: 129,
            text: None,
        },
    ]
}
