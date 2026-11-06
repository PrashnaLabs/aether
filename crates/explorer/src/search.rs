use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

/// An item shown in the Quick Open (Cmd+P) file finder list.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickOpenItem {
    pub path: PathBuf,
    pub rel_path: String,
    pub score: i64,
}

/// Fuzzy matches a pre-lowercased query against a pre-lowercased path.
/// `candidate_len` is the original path's byte length, for the distance penalty.
/// Returns `Some(score)` if all query chars match in order.
pub(crate) fn fuzzy_match_path(
    query_lower: &str,
    candidate_lower: &str,
    candidate_len: usize,
) -> Option<i64> {
    if query_lower.is_empty() {
        return Some(0);
    }

    let q_chars: Vec<char> = query_lower.chars().collect();
    let c_chars: Vec<char> = candidate_lower.chars().collect();

    let mut q_idx = 0;
    let mut score: i64 = 0;
    let mut consecutive = 0;
    let mut first_match_idx = None;

    // Check if filename matches directly
    let file_name =
        Path::new(candidate_lower).file_name().and_then(|f| f.to_str()).unwrap_or(candidate_lower);

    if file_name.starts_with(query_lower) {
        score += 50;
    } else if file_name.contains(query_lower) {
        score += 30;
    }

    for (c_idx, &ch) in c_chars.iter().enumerate() {
        if q_idx < q_chars.len() && ch == q_chars[q_idx] {
            if first_match_idx.is_none() {
                first_match_idx = Some(c_idx);
            }
            score += 10;
            if consecutive > 0 {
                score += (consecutive as i64) * 5;
            }
            consecutive += 1;

            // Word or segment boundary bonus
            if c_idx == 0
                || c_chars[c_idx - 1] == '/'
                || c_chars[c_idx - 1] == '_'
                || c_chars[c_idx - 1] == '.'
                || c_chars[c_idx - 1] == '-'
            {
                score += 15;
            }

            q_idx += 1;
        } else {
            consecutive = 0;
        }
    }

    if q_idx == q_chars.len() {
        // Distance penalty
        if let Some(first) = first_match_idx {
            score -= (candidate_len - first) as i64;
        }
        Some(score)
    } else {
        None
    }
}

/// The 30 best Quick Open matches of `query` among `files` (relative to `root`).
/// `lower_files` is `files` lowercased, parallel to `files`.
pub(crate) fn quick_open_matches(
    root: &Path,
    files: &[String],
    lower_files: &[String],
    query: &str,
) -> Vec<QuickOpenItem> {
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(i64, &String)> = files
        .iter()
        .zip(lower_files)
        .filter_map(|(rel, rel_lower)| {
            Some((fuzzy_match_path(&query_lower, rel_lower, rel.len())?, rel))
        })
        .collect();
    scored.sort_by_key(|(score, _)| Reverse(*score));
    scored
        .into_iter()
        .take(30)
        .map(|(score, rel)| QuickOpenItem { path: root.join(rel), rel_path: rel.clone(), score })
        .collect()
}

/// Parameters for global workspace grep search.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrepOptions {
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub is_regex: bool,
}

/// A line match from workspace grep.
#[derive(Clone, Debug, PartialEq)]
pub struct GrepMatch {
    pub path: PathBuf,
    pub line_number: usize,
    pub line_text: String,
    pub match_indices: (usize, usize),
}

/// The regex `options` search with, compiled once per run; a query that isn't a
/// valid regex is searched as plain text.
pub(crate) fn grep_regex(options: &GrepOptions) -> Option<Regex> {
    let build = |pattern: &str| {
        let pattern =
            if options.whole_word { format!(r"\b(?:{pattern})\b") } else { pattern.to_string() };
        RegexBuilder::new(&pattern).case_insensitive(!options.case_sensitive).build().ok()
    };
    options
        .is_regex
        .then(|| build(&options.query))
        .flatten()
        .or_else(|| build(&regex::escape(&options.query)))
}

/// Every match of `re` in `content`, with 1-indexed line numbers.
pub(crate) fn search_file_content(path: &Path, content: &str, re: &Regex) -> Vec<GrepMatch> {
    let mut results = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        for mat in re.find_iter(line) {
            results.push(GrepMatch {
                path: path.to_path_buf(),
                line_number: line_idx + 1,
                line_text: line.to_string(),
                match_indices: (mat.start(), mat.end()),
            });
        }
    }
    results
}

/// Greps `files` (relative to `root`) for `options`, stopping at `limit` matches.
/// Blocking: run it off the UI thread.
pub(crate) fn grep_files(
    root: &Path,
    files: &[String],
    options: &GrepOptions,
    limit: usize,
) -> Vec<GrepMatch> {
    let Some(re) = grep_regex(options).filter(|_| !options.query.trim().is_empty()) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for rel in files {
        let path = root.join(rel);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if re.is_match(&content) {
            results.extend(search_file_content(&path, &content, &re));
            if results.len() >= limit {
                results.truncate(limit);
                break;
            }
        }
    }
    results
}

#[cfg(test)]
#[path = "../tests/unit/search.rs"]
mod tests;
