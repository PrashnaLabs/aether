use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{ModelRate, PricingStatus, RateTable};

const LITELLM_RATES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const RATES_CACHE_FILE: &str = "usage-model-rates.json";
const RATES_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Models that appear in transcripts without a real price.
const UNPRICEABLE_MODELS: [&str; 6] =
    ["<synthetic>", "synthetic", "unknown", "default", "fable", "lark"];

fn normalize_model_name(model: &str) -> String {
    let trimmed = model.trim().to_ascii_lowercase();
    match trimmed.rfind('/') {
        Some(slash) => trimmed[slash + 1..].to_owned(),
        None => trimmed,
    }
}

pub(super) fn lookup_rate<'a>(table: &'a RateTable, model: &str) -> Option<&'a ModelRate> {
    let normalized = normalize_model_name(model);
    if normalized.is_empty() || UNPRICEABLE_MODELS.contains(&normalized.as_str()) {
        return None;
    }
    if let Some(rate) = table.rates.get(&normalized) {
        return Some(rate);
    }
    // Strip trailing date stamp (e.g. `-20250929`) or `-latest` if present.
    if let Some((stem, suffix)) = normalized.rsplit_once('-')
        && (suffix == "latest" || (suffix.len() == 8 && suffix.bytes().all(|b| b.is_ascii_digit())))
        && let Some(rate) = table.rates.get(stem)
    {
        return Some(rate);
    }
    None
}

fn parse_rate_table(document: &Value) -> HashMap<String, ModelRate> {
    let mut table = HashMap::new();
    let Some(entries) = document.as_object() else {
        return table;
    };
    let finite = |value: Option<&Value>| value.and_then(Value::as_f64).filter(|v| v.is_finite());
    for (name, entry) in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let Some(input) = finite(entry.get("input_cost_per_token")) else {
            continue;
        };
        let Some(output) = finite(entry.get("output_cost_per_token")) else {
            continue;
        };
        table.insert(
            normalize_model_name(name),
            ModelRate {
                input,
                output,
                cache_read: finite(entry.get("cache_read_input_token_cost")).unwrap_or(input),
                cache_creation: finite(entry.get("cache_creation_input_token_cost"))
                    .unwrap_or(input),
            },
        );
    }
    table
}

pub(super) fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn read_rates_cache(path: &Path) -> Option<(i64, HashMap<String, ModelRate>)> {
    let document: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let fetched_at_ms = document.get("fetched_at_ms")?.as_i64()?;
    let mut rates = HashMap::new();
    for (name, entry) in document.get("rates")?.as_object()? {
        let values = entry.as_array()?;
        let field = |index: usize| values.get(index).and_then(Value::as_f64);
        rates.insert(
            name.clone(),
            ModelRate {
                input: field(0)?,
                output: field(1)?,
                cache_read: field(2)?,
                cache_creation: field(3)?,
            },
        );
    }
    Some((fetched_at_ms, rates))
}

fn write_rates_cache(path: &Path, fetched_at_ms: i64, rates: &HashMap<String, ModelRate>) {
    let entries: serde_json::Map<String, Value> = rates
        .iter()
        .map(|(name, rate)| {
            (
                name.clone(),
                serde_json::json!([rate.input, rate.output, rate.cache_read, rate.cache_creation]),
            )
        })
        .collect();
    let document = serde_json::json!({ "fetched_at_ms": fetched_at_ms, "rates": entries });
    let _ = std::fs::write(path, document.to_string());
}

fn fetch_litellm_rates() -> Option<HashMap<String, ModelRate>> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().ok()?;
    let body = rt
        .block_on(async {
            let res =
                client.get(LITELLM_RATES_URL).header("Accept", "application/json").send().await?;
            if res.status() != 200 {
                return Err(res.error_for_status().unwrap_err());
            }
            res.text().await
        })
        .ok()?;
    let doc: Value = serde_json::from_str(&body).ok()?;
    let rates = parse_rate_table(&doc);
    (!rates.is_empty()).then_some(rates)
}

fn merge_fallback_prices(
    rates: &mut HashMap<String, ModelRate>,
    fallback_prices: &HashMap<String, (f64, f64)>,
) {
    for (model, (input, output)) in fallback_prices {
        if !input.is_finite() || !output.is_finite() {
            continue;
        }
        let key = normalize_model_name(model);
        rates.entry(key).or_insert(ModelRate {
            input: *input,
            output: *output,
            cache_read: *input,
            cache_creation: *input,
        });
    }
}

pub fn load_rate_table(
    cache_dir: &Path,
    fallback_prices: &HashMap<String, (f64, f64)>,
) -> RateTable {
    let cache_path = cache_dir.join(RATES_CACHE_FILE);
    let disk = read_rates_cache(&cache_path).filter(|(_, rates)| !rates.is_empty());
    let now_ms = unix_time_ms();
    if let Some((fetched_at_ms, mut rates)) = disk.clone()
        && now_ms.saturating_sub(fetched_at_ms) < RATES_TTL.as_millis() as i64
    {
        merge_fallback_prices(&mut rates, fallback_prices);
        return RateTable { rates, status: PricingStatus::Cached };
    }

    let fetched = fetch_litellm_rates();
    match (fetched, disk) {
        (Some(mut rates), _) => {
            write_rates_cache(&cache_path, now_ms, &rates);
            merge_fallback_prices(&mut rates, fallback_prices);
            RateTable { rates, status: PricingStatus::Fresh }
        }
        (None, Some((_, mut rates))) => {
            merge_fallback_prices(&mut rates, fallback_prices);
            RateTable { rates, status: PricingStatus::Cached }
        }
        (None, None) => {
            let mut rates = HashMap::new();
            merge_fallback_prices(&mut rates, fallback_prices);
            let status =
                if rates.is_empty() { PricingStatus::Unavailable } else { PricingStatus::Cached };
            RateTable { rates, status }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/usage_history/pricing.rs"]
mod tests;
