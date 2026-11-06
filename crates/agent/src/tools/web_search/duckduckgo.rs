use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, ORIGIN, REFERER, USER_AGENT};
use scraper::{Html, Selector};
use serde::Serialize;

const MAX_RESULTS: usize = 8;
const ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
    (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub(crate) async fn search(
    client: &reqwest::Client,
    query: &str,
) -> Result<Vec<SearchResult>, String> {
    // DDG serves a 202 bot challenge unless the request looks like a browser form post.
    let response = client
        .post(ENDPOINT)
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, "text/html,application/xhtml+xml")
        .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(ORIGIN, "https://html.duckduckgo.com")
        .header(REFERER, "https://html.duckduckgo.com/")
        .form(&[("q", query)])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("DuckDuckGo returned {}, likely rate limited", response.status()));
    }
    let html = response.text().await.map_err(|e| e.to_string())?;
    Ok(parse(&html))
}

fn parse(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let result = Selector::parse(".web-result:not(.result--ad)").unwrap();
    let link = Selector::parse(".result__a").unwrap();
    let snippet = Selector::parse(".result__snippet").unwrap();
    let text = |el: scraper::ElementRef| el.text().collect::<String>().trim().to_string();
    doc.select(&result)
        .filter_map(|r| {
            let a = r.select(&link).next()?;
            Some(SearchResult {
                title: text(a),
                url: a.value().attr("href")?.to_string(),
                snippet: r.select(&snippet).next().map(text).unwrap_or_default(),
            })
        })
        .take(MAX_RESULTS)
        .collect()
}

#[cfg(test)]
#[path = "../../../tests/unit/tools/web_search/duckduckgo.rs"]
mod tests;
