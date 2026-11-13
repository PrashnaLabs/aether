use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aether_core::home_dir;
use chrono::{Local, NaiveDate, TimeZone as _, Utc};

use super::discovery::{
    collect_opencode_service_records, list_antigravity_transcripts, list_files_with_extension,
    load_opencode_session_dirs, provider_roots, read_transcript_records,
};
use super::parsers::collect_saved_chat_records;
use super::pricing::{load_rate_table, lookup_rate, unix_time_ms};
use super::{
    CostQuality, DaySlice, FileCacheEntry, ModelSlice, MonthSlice, PricingStatus, ProjectSlice,
    ProviderDay, ProviderSlice, RateTable, ScanCache, TokenTotals, UsageHistory, UsageProvider,
    UsageRecord, UsageWindow, first_of_month,
};
use crate::store::SavedChat;

/// Slack before the window's start day when prefiltering transcript files by
/// mtime.
const MTIME_SLACK: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Clone, Copy, PartialEq)]
enum CostSource {
    Reported,
    Priced,
    Unpriced,
}

#[derive(Default)]
struct Bucket {
    totals: TokenTotals,
    cost_usd: f64,
    cache_savings_usd: f64,
    records: u64,
    unpriced_records: u64,
    reported_records: u64,
}

#[derive(Default)]
struct ProjectAccumulator {
    cost_usd: f64,
    total_tokens: u64,
    by_provider: [ProviderDay; UsageProvider::COUNT],
    sessions: HashSet<(UsageProvider, String)>,
    models: HashMap<String, f64>,
    last_day: Option<NaiveDate>,
}

struct Aggregator {
    since_day: NaiveDate,
    until_day: NaiveDate,
    project_roots: Vec<PathBuf>,
    buckets: HashMap<(NaiveDate, UsageProvider, String), Bucket>,
    seen: HashSet<String>,
    sessions: HashSet<(UsageProvider, String)>,
    month_sessions: HashSet<(NaiveDate, UsageProvider, String)>,
    projects: HashMap<String, ProjectAccumulator>,
}

impl Aggregator {
    fn new(since_day: NaiveDate, until_day: NaiveDate, project_roots: &[PathBuf]) -> Self {
        let mut project_roots = project_roots.to_vec();
        project_roots.sort_by_key(|root| std::cmp::Reverse(root.as_os_str().len()));
        Self {
            since_day,
            until_day,
            project_roots,
            buckets: HashMap::new(),
            seen: HashSet::new(),
            sessions: HashSet::new(),
            month_sessions: HashSet::new(),
            projects: HashMap::new(),
        }
    }

    fn resolve_project(&self, cwd: &str) -> String {
        if cwd.is_empty() {
            return String::new();
        }
        let path = Path::new(cwd);
        self.project_roots
            .iter()
            .find(|root| path.starts_with(root))
            .map(|root| root.display().to_string())
            .unwrap_or_else(|| cwd.to_owned())
    }

    fn add(&mut self, record: &UsageRecord, rates: &RateTable) {
        if let Some(key) = &record.dedupe_key
            && !self.seen.insert(key.clone())
        {
            return;
        }
        let Some(timestamp) = Utc.timestamp_millis_opt(record.timestamp_ms).single() else {
            return;
        };
        let day = timestamp.with_timezone(&Local).date_naive();
        if day < self.since_day || day > self.until_day {
            return;
        }

        let (cost_usd, source) = match record.reported_cost_usd {
            Some(cost) => (cost, CostSource::Reported),
            None => match lookup_rate(rates, &record.model) {
                Some(rate) => (
                    record.totals.uncached_input as f64 * rate.input
                        + record.totals.cached_input as f64 * rate.cache_read
                        + record.totals.cache_creation as f64 * rate.cache_creation
                        + record.totals.output as f64 * rate.output,
                    CostSource::Priced,
                ),
                None => (0.0, CostSource::Unpriced),
            },
        };
        let cache_savings_usd = lookup_rate(rates, &record.model)
            .map(|rate| {
                (record.totals.cached_input as f64 * (rate.input - rate.cache_read)).max(0.0)
            })
            .unwrap_or(0.0);

        let bucket = self.buckets.entry((day, record.provider, record.model.clone())).or_default();
        bucket.totals.add(&record.totals);
        bucket.cost_usd += cost_usd;
        bucket.cache_savings_usd += cache_savings_usd;
        bucket.records += 1;
        match source {
            CostSource::Unpriced => bucket.unpriced_records += 1,
            CostSource::Reported => bucket.reported_records += 1,
            CostSource::Priced => {}
        }
        if !record.session_id.is_empty() {
            self.sessions.insert((record.provider, record.session_id.clone()));
            self.month_sessions.insert((
                first_of_month(day),
                record.provider,
                record.session_id.clone(),
            ));
        }

        let tokens = record.totals.total();
        let project_key = self.resolve_project(&record.project);
        let project = self.projects.entry(project_key).or_default();
        project.cost_usd += cost_usd;
        project.total_tokens += tokens;
        project.by_provider[record.provider.index()].cost_usd += cost_usd;
        project.by_provider[record.provider.index()].total_tokens += tokens;
        if !record.session_id.is_empty() {
            project.sessions.insert((record.provider, record.session_id.clone()));
        }
        *project.models.entry(record.model.clone()).or_default() += cost_usd;
        project.last_day = Some(project.last_day.map_or(day, |last| last.max(day)));
    }
}

pub fn scan_cached(
    window: UsageWindow,
    project_roots: &[PathBuf],
    saved_chats: &[SavedChat],
    fallback_prices: &HashMap<String, (f64, f64)>,
) -> UsageHistory {
    static CACHE: std::sync::LazyLock<std::sync::Mutex<ScanCache>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
    let rates = load_rate_table(&home_dir().join(".aether"), fallback_prices);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    scan(&mut cache, &rates, window, project_roots, saved_chats)
}

pub fn scan(
    cache: &mut ScanCache,
    rates: &RateTable,
    window: UsageWindow,
    project_roots: &[PathBuf],
    saved_chats: &[SavedChat],
) -> UsageHistory {
    let started = Instant::now();
    let (since_day, until_day) = window.bounds(Local::now().date_naive());
    let window_start_ms = since_day
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map(|midnight| midnight.timestamp_millis())
        .unwrap_or_else(|| unix_time_ms() - (until_day - since_day).num_days().max(1) * 86_400_000);
    let mtime_cutoff_ms = window_start_ms - MTIME_SLACK.as_millis() as i64;

    let mut aggregator = Aggregator::new(since_day, until_day, project_roots);
    let mut scanned_files = 0;
    let mut skipped_files = 0;
    let errors = Vec::new();
    let (opencode_dirs, no_dirs) = (std::cell::OnceCell::new(), HashMap::new());
    let mut native_sessions: HashSet<String> = HashSet::new();
    let mut listed: HashSet<PathBuf> = HashSet::new();

    for provider in UsageProvider::ALL {
        let roots = provider_roots(provider);
        for root in roots {
            if !root.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            skipped_files += match provider {
                UsageProvider::Antigravity => {
                    list_antigravity_transcripts(&root, mtime_cutoff_ms, &mut files)
                }
                UsageProvider::OpenCode => {
                    list_files_with_extension(&root, "json", mtime_cutoff_ms, &mut files)
                }
                _ => list_files_with_extension(&root, "jsonl", mtime_cutoff_ms, &mut files),
            };
            for (path, size, mtime_ms) in files {
                scanned_files += 1;
                listed.insert(path.clone());
                let records = match cache.get(&path) {
                    Some(entry)
                        if entry.size == size
                            && entry.mtime_ms == mtime_ms
                            && entry.provider == provider =>
                    {
                        &entry.records
                    }
                    _ => match read_transcript_records(
                        &path,
                        provider,
                        if provider == UsageProvider::OpenCode {
                            opencode_dirs.get_or_init(load_opencode_session_dirs)
                        } else {
                            &no_dirs
                        },
                    ) {
                        Some(records) => {
                            &cache
                                .entry(path)
                                .insert_entry(FileCacheEntry { size, mtime_ms, provider, records })
                                .into_mut()
                                .records
                        }
                        None => continue,
                    },
                };
                for record in records {
                    if !record.session_id.is_empty() {
                        native_sessions.insert(record.session_id.clone());
                    }
                    aggregator.add(record, rates);
                }
            }
        }
    }

    cache.retain(|path, _| listed.contains(path) || path.exists());

    // OpenCode live daemon sessions not already covered by file storage.
    for record in collect_opencode_service_records(&native_sessions) {
        if !record.session_id.is_empty() {
            native_sessions.insert(record.session_id.clone());
        }
        aggregator.add(&record, rates);
    }

    // OpenRouter and Aether-stored chats (skipping CLI sessions already counted from disk).
    for record in collect_saved_chat_records(saved_chats) {
        if record.provider != UsageProvider::OpenRouter
            && !record.session_id.is_empty()
            && native_sessions.contains(&record.session_id)
        {
            continue;
        }
        aggregator.add(&record, rates);
    }

    derive_history(
        aggregator,
        window,
        since_day,
        until_day,
        rates.status,
        scanned_files,
        skipped_files,
        errors,
        started.elapsed(),
    )
}

#[allow(clippy::too_many_arguments)]
fn derive_history(
    aggregator: Aggregator,
    window: UsageWindow,
    since_day: NaiveDate,
    until_day: NaiveDate,
    pricing: PricingStatus,
    scanned_files: usize,
    skipped_files: usize,
    errors: Vec<String>,
    scan_duration: Duration,
) -> UsageHistory {
    let mut totals = TokenTotals::default();
    let mut cost_usd = 0.0;
    let mut cache_savings_usd = 0.0;
    let mut records = 0;
    let mut unpriced_records = 0;
    let mut reported_records = 0;
    let mut providers: HashMap<UsageProvider, (f64, u64)> = HashMap::new();
    let mut models: HashMap<(UsageProvider, String), (f64, u64)> = HashMap::new();
    let mut daily: HashMap<NaiveDate, DaySlice> = HashMap::new();
    let mut month_models: HashMap<(NaiveDate, String), f64> = HashMap::new();

    for ((day, provider, model), bucket) in &aggregator.buckets {
        let tokens = bucket.totals.total();
        totals.add(&bucket.totals);
        cost_usd += bucket.cost_usd;
        cache_savings_usd += bucket.cache_savings_usd;
        records += bucket.records;
        unpriced_records += bucket.unpriced_records;
        reported_records += bucket.reported_records;

        let provider_entry = providers.entry(*provider).or_default();
        provider_entry.0 += bucket.cost_usd;
        provider_entry.1 += tokens;

        let model_entry = models.entry((*provider, model.clone())).or_default();
        model_entry.0 += bucket.cost_usd;
        model_entry.1 += tokens;

        let day_entry = daily.entry(*day).or_insert_with(|| DaySlice {
            day: *day,
            cost_usd: 0.0,
            total_tokens: 0,
            by_provider: [ProviderDay::default(); UsageProvider::COUNT],
        });
        day_entry.cost_usd += bucket.cost_usd;
        day_entry.total_tokens += tokens;
        day_entry.by_provider[provider.index()].cost_usd += bucket.cost_usd;
        day_entry.by_provider[provider.index()].total_tokens += tokens;

        *month_models.entry((first_of_month(*day), model.clone())).or_default() += bucket.cost_usd;
    }

    let total_tokens = totals.total();
    let share = |part: f64, whole: f64| if whole == 0.0 { 0.0 } else { part / whole };

    let mut provider_slices: Vec<ProviderSlice> = providers
        .into_iter()
        .map(|(provider, (provider_cost, provider_tokens))| ProviderSlice {
            provider,
            cost_usd: provider_cost,
            total_tokens: provider_tokens,
            cost_share: share(provider_cost, cost_usd),
            token_share: share(provider_tokens as f64, total_tokens as f64),
        })
        .collect();
    provider_slices.sort_by(|a, b| {
        b.cost_usd.total_cmp(&a.cost_usd).then(b.total_tokens.cmp(&a.total_tokens))
    });

    let mut model_slices: Vec<ModelSlice> = models
        .into_iter()
        .map(|((provider, model), (model_cost, model_tokens))| ModelSlice {
            provider,
            model,
            cost_usd: model_cost,
            total_tokens: model_tokens,
            cost_share: share(model_cost, cost_usd),
        })
        .collect();
    model_slices.sort_by(|a, b| {
        b.cost_usd.total_cmp(&a.cost_usd).then(b.total_tokens.cmp(&a.total_tokens))
    });

    let mut day_slices: Vec<DaySlice> = daily.into_values().collect();
    day_slices.sort_by_key(|slice| slice.day);

    let mut months: HashMap<NaiveDate, MonthSlice> = HashMap::new();
    for day in &day_slices {
        let month = months.entry(first_of_month(day.day)).or_insert_with(|| MonthSlice {
            first_day: first_of_month(day.day),
            cost_usd: 0.0,
            total_tokens: 0,
            by_provider: [ProviderDay::default(); UsageProvider::COUNT],
            sessions: 0,
            active_days: 0,
            top_models: Vec::new(),
        });
        month.cost_usd += day.cost_usd;
        month.total_tokens += day.total_tokens;
        for index in 0..month.by_provider.len() {
            month.by_provider[index].cost_usd += day.by_provider[index].cost_usd;
            month.by_provider[index].total_tokens += day.by_provider[index].total_tokens;
        }
        if day.total_tokens > 0 {
            month.active_days += 1;
        }
    }
    for (first_day, _, _) in &aggregator.month_sessions {
        if let Some(month) = months.get_mut(first_day) {
            month.sessions += 1;
        }
    }
    for ((first_day, model), model_cost) in month_models {
        if let Some(month) = months.get_mut(&first_day) {
            month.top_models.push((model, model_cost));
        }
    }
    let mut month_slices: Vec<MonthSlice> = months.into_values().collect();
    month_slices.sort_by_key(|slice| slice.first_day);
    for month in &mut month_slices {
        month.top_models.sort_by(|a, b| b.1.total_cmp(&a.1));
    }

    let mut project_slices: Vec<ProjectSlice> = aggregator
        .projects
        .into_iter()
        .map(|(path, project)| {
            let mut top_models: Vec<(String, f64)> = project.models.into_iter().collect();
            top_models.sort_by(|a, b| b.1.total_cmp(&a.1));
            ProjectSlice {
                path,
                cost_usd: project.cost_usd,
                total_tokens: project.total_tokens,
                by_provider: project.by_provider,
                sessions: project.sessions.len() as u64,
                cost_share: share(project.cost_usd, cost_usd),
                last_day: project.last_day,
                top_models,
            }
        })
        .collect();
    project_slices.sort_by(|a, b| {
        b.cost_usd.total_cmp(&a.cost_usd).then(b.total_tokens.cmp(&a.total_tokens))
    });

    let record_share = |part: u64| share(part as f64, records as f64);
    UsageHistory {
        window,
        since_day,
        until_day,
        totals,
        total_tokens,
        cost_usd,
        records,
        sessions: aggregator.sessions.len() as u64,
        providers: provider_slices,
        models: model_slices,
        daily: day_slices,
        months: month_slices,
        projects: project_slices,
        quality: CostQuality {
            provider_reported_share: record_share(reported_records),
            model_priced_share: record_share(records - reported_records - unpriced_records),
            unpriced_share: record_share(unpriced_records),
            cache_savings_usd,
        },
        pricing,
        scanned_files,
        skipped_files,
        errors,
        scan_duration,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/usage_history/aggregate.rs"]
mod tests;
