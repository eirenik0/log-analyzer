use crate::cli::ErrorsSortBy;
use crate::comparator::LogFilter;
use crate::config::AnalyzerConfig;
use crate::parser::LogEntry;
use crate::perf_analyzer::{OrphanOperation, analyze_performance_with_config};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::sync::LazyLock;

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s"')]+"#).expect("valid url regex"));
static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
        .expect("valid uuid regex")
});
static ISO_TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?\b")
        .expect("valid iso timestamp regex")
});
static CLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b").expect("valid clock regex"));
static REQUEST_ID_BRACKET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[[^\]\s]*--[^\]\s]+\]").expect("valid request id regex"));
static ID_QUOTED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(id|request_id|session_id|render_id|job_id|test_id|check_id)\b\s*[:=]?\s*"[^"]+""#,
    )
    .expect("valid quoted id regex")
});
static ID_SQUOTED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(id|request_id|session_id|render_id|job_id|test_id|check_id)\b\s*[:=]?\s*'[^']+'"#,
    )
    .expect("valid single quoted id regex")
});
static ID_BARE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(id|request_id|session_id|render_id|job_id|test_id|check_id)\b\s*[:=]?\s*[A-Za-z0-9_.:-]{6,}",
    )
    .expect("valid bare id regex")
});
static LONG_HEX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[0-9a-f]{12,}\b").expect("valid long hex regex"));
static LONG_NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{6,}\b").expect("valid long number regex"));
static MULTISPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid multispace regex"));

#[derive(Debug, Clone, Copy)]
pub struct ErrorsOptions {
    pub top_n: usize,
    pub include_warn: bool,
    pub show_sessions: bool,
    pub sort_by: ErrorsSortBy,
    pub file_count: usize,
    pub limits: Option<ErrorReportLimits>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ErrorReportLimits {
    pub max_sample_chars: usize,
    pub max_stack_frames: usize,
    pub max_output_chars: usize,
}

impl Default for ErrorReportLimits {
    fn default() -> Self {
        Self {
            max_sample_chars: 600,
            max_stack_frames: 5,
            max_output_chars: 12000,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorAnalysisReport {
    pub file_count: usize,
    pub include_warn: bool,
    pub total_entries: usize,
    pub error_count: usize,
    pub warn_count: usize,
    pub unique_patterns: usize,
    pub affected_sessions_count: usize,
    pub clusters: Vec<ErrorClusterReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_blocking: Option<LongestBlockingError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorClusterReport {
    pub severity: String,
    pub pattern: String,
    pub count: usize,
    pub components: Vec<String>,
    pub first_timestamp: DateTime<Local>,
    pub last_timestamp: DateTime<Local>,
    pub sample_message: String,
    pub affected_sessions_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub affected_sessions: Vec<ClusterSessionImpact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterSessionImpact {
    pub session_path: String,
    pub error_count: usize,
    pub outcome: SessionOutcome,
    pub first_error_timestamp: DateTime<Local>,
    pub last_error_timestamp: DateTime<Local>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionOutcome {
    Completed,
    Orphaned,
}

#[derive(Debug, Clone, Serialize)]
pub struct LongestBlockingError {
    pub severity: String,
    pub pattern: String,
    pub session_path: String,
    pub duration_ms: i64,
}

#[derive(Debug)]
struct ClusterAccum {
    severity: String,
    pattern: String,
    count: usize,
    components: BTreeSet<String>,
    first_timestamp: DateTime<Local>,
    last_timestamp: DateTime<Local>,
    sample_message: String,
    session_counts: HashMap<String, usize>,
    session_first_error: HashMap<String, DateTime<Local>>,
    session_last_error: HashMap<String, DateTime<Local>>,
}

#[derive(Debug, Clone)]
struct SessionLifecycleState {
    last_seen: DateTime<Local>,
    orphaned: bool,
}

pub fn analyze_errors_with_config(
    logs: &[LogEntry],
    filter: &LogFilter,
    config: &AnalyzerConfig,
    options: &ErrorsOptions,
) -> ErrorAnalysisReport {
    let filtered_logs: Vec<&LogEntry> = logs.iter().filter(|entry| filter.matches(entry)).collect();
    let perf_results = analyze_performance_with_config(logs, filter, None, config);
    let session_states = build_session_lifecycle_states(&filtered_logs, &perf_results.orphans);
    let level_filter = build_error_level_filter(options.include_warn);

    let mut clusters: HashMap<(String, String), ClusterAccum> = HashMap::new();
    let mut error_count = 0usize;
    let mut warn_count = 0usize;
    let mut affected_sessions: HashSet<String> = HashSet::new();

    for entry in filtered_logs
        .iter()
        .copied()
        .filter(|entry| level_filter.matches(entry))
    {
        let severity = normalized_severity(&entry.level);
        match severity.as_str() {
            "ERROR" => error_count += 1,
            "WARN" => warn_count += 1,
            _ => continue,
        }

        let pattern = normalize_message_pattern(&entry.message);
        let key = (severity.clone(), pattern.clone());
        let cluster = clusters.entry(key).or_insert_with(|| ClusterAccum {
            severity,
            pattern,
            count: 0,
            components: BTreeSet::new(),
            first_timestamp: entry.timestamp,
            last_timestamp: entry.timestamp,
            sample_message: entry.message.clone(),
            session_counts: HashMap::new(),
            session_first_error: HashMap::new(),
            session_last_error: HashMap::new(),
        });

        cluster.count += 1;
        cluster.components.insert(entry.component.clone());
        if entry.timestamp < cluster.first_timestamp {
            cluster.first_timestamp = entry.timestamp;
        }
        if entry.timestamp > cluster.last_timestamp {
            cluster.last_timestamp = entry.timestamp;
        }

        if !entry.component_id.is_empty() {
            affected_sessions.insert(entry.component_id.clone());
            *cluster
                .session_counts
                .entry(entry.component_id.clone())
                .or_insert(0) += 1;
            cluster
                .session_first_error
                .entry(entry.component_id.clone())
                .and_modify(|ts| {
                    if entry.timestamp < *ts {
                        *ts = entry.timestamp;
                    }
                })
                .or_insert(entry.timestamp);
            cluster
                .session_last_error
                .entry(entry.component_id.clone())
                .and_modify(|ts| {
                    if entry.timestamp > *ts {
                        *ts = entry.timestamp;
                    }
                })
                .or_insert(entry.timestamp);
        }
    }

    let mut longest_blocking: Option<LongestBlockingError> = None;
    let mut finalized_clusters: Vec<ErrorClusterReport> = clusters
        .into_values()
        .map(|accum| finalize_cluster(accum, &session_states, &mut longest_blocking))
        .collect();

    sort_clusters(&mut finalized_clusters, options.sort_by);

    ErrorAnalysisReport {
        file_count: options.file_count,
        include_warn: options.include_warn,
        total_entries: error_count + warn_count,
        error_count,
        warn_count,
        unique_patterns: finalized_clusters.len(),
        affected_sessions_count: affected_sessions.len(),
        clusters: finalized_clusters,
        longest_blocking,
    }
}

pub fn format_errors_text(report: &ErrorAnalysisReport, options: &ErrorsOptions) -> String {
    if let Some(limits) = options.limits {
        return format_bounded_errors_text(report, options, limits, "");
    }
    let mut out = String::new();
    let header_label = if report.warn_count > 0 {
        "ERRORS/WARNS"
    } else {
        "ERRORS"
    };
    let _ = writeln!(
        out,
        "{}: {} entries ({} patterns) across {} {}",
        header_label,
        report.total_entries,
        report.unique_patterns,
        report.file_count,
        if report.file_count == 1 {
            "file"
        } else {
            "files"
        }
    );

    out.push('\n');
    let _ = writeln!(out, "Impact summary");
    let _ = writeln!(out, "  Total errors: {}", report.error_count);
    if report.include_warn {
        let _ = writeln!(out, "  Total warnings: {}", report.warn_count);
    }
    let _ = writeln!(out, "  Unique error patterns: {}", report.unique_patterns);
    let _ = writeln!(
        out,
        "  Affected sessions: {}",
        report.affected_sessions_count
    );
    if let Some(longest) = &report.longest_blocking {
        let _ = writeln!(
            out,
            "  Longest blocking error: {}  [{}] {}",
            format_duration_approx(longest.duration_ms),
            longest.severity,
            longest.pattern
        );
        let _ = writeln!(out, "  Session: {}", longest.session_path);
    } else {
        let _ = writeln!(out, "  Longest blocking error: n/a");
    }

    if report.total_entries == 0 {
        let _ = writeln!(out, "\nNo matching ERROR/WARN entries found.");
        return out;
    }

    let display_limit = displayed_cluster_count(report, options);
    let show_date = spans_multiple_dates(report);
    out.push('\n');

    for (idx, cluster) in report.clusters.iter().take(display_limit).enumerate() {
        let components = if cluster.components.is_empty() {
            "<unknown>".to_string()
        } else {
            cluster.components.join(", ")
        };

        let _ = writeln!(
            out,
            " #{:<2} [{}] ×{}  {}",
            idx + 1,
            cluster.severity,
            cluster.count,
            components
        );
        let _ = writeln!(out, "     {}", cluster.pattern);
        let _ = writeln!(
            out,
            "     First: {}  Last: {}",
            format_timestamp(cluster.first_timestamp, show_date),
            format_timestamp(cluster.last_timestamp, show_date)
        );
        let _ = writeln!(out, "     Sample: {}", cluster.sample_message);
        if let Some(blocking_ms) = cluster.blocking_ms
            && blocking_ms > 0
        {
            let _ = writeln!(
                out,
                "     Blocking: {} (max error-to-session-end span)",
                format_duration_approx(blocking_ms)
            );
        }

        if options.show_sessions {
            if cluster.affected_sessions.is_empty() {
                let _ = writeln!(
                    out,
                    "     Affected sessions: none (no component_id on entries)"
                );
            } else {
                let _ = writeln!(
                    out,
                    "     Affected sessions ({}):",
                    cluster.affected_sessions_count
                );
                for session in &cluster.affected_sessions {
                    let _ = writeln!(
                        out,
                        "       - {}  ×{}  {}{}",
                        session.session_path,
                        session.error_count,
                        session.outcome.as_label(),
                        session
                            .blocking_ms
                            .filter(|ms| *ms > 0)
                            .map(|ms| format!("  ({})", format_duration_approx(ms)))
                            .unwrap_or_default()
                    );
                }
            }
        }

        if idx + 1 < display_limit {
            out.push('\n');
        }
    }

    if display_limit < report.clusters.len() {
        let _ = writeln!(
            out,
            "\n... {} more cluster{} hidden by --top-n {}",
            report.clusters.len() - display_limit,
            if report.clusters.len() - display_limit == 1 {
                ""
            } else {
                "s"
            },
            options.top_n
        );
    }

    out
}

#[derive(Debug, Default, Clone, Serialize)]
struct DetailOmissions {
    clusters: usize,
    sample_chars: usize,
    stack_frames: usize,
    pattern_chars: usize,
    session_details: usize,
}

impl DetailOmissions {
    fn text(&self) -> String {
        format!(
            "\nOmitted: {} clusters; {} sample characters; {} stack-frame lines; {} pattern characters; {} session details.\n",
            self.clusters,
            self.sample_chars,
            self.stack_frames,
            self.pattern_chars,
            self.session_details
        )
    }
}

fn char_prefix(text: &str, limit: usize) -> &str {
    match text.char_indices().nth(limit) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

fn is_stack_frame(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("at ")
        || line.starts_with("at\t")
        || line.starts_with("File \"")
        || line.split_once(':').is_some_and(|(index, rest)| {
            !index.is_empty()
                && index.chars().all(|ch| ch.is_ascii_digit())
                && rest.starts_with(' ')
        })
}

fn frame_count(message: &str) -> usize {
    message.lines().filter(|line| is_stack_frame(line)).count()
}

fn bounded_sample(message: &str, limits: ErrorReportLimits) -> (String, usize, usize) {
    let mut frames = 0;
    let lines: Vec<_> = message
        .lines()
        .filter(|line| {
            if is_stack_frame(line) {
                frames += 1;
                frames <= limits.max_stack_frames
            } else {
                true
            }
        })
        .collect();
    let filtered = lines.join("\n");
    let sample = char_prefix(&filtered, limits.max_sample_chars).to_string();
    let retained_frames = complete_frame_count(&sample, &filtered);
    let omitted_chars = message
        .chars()
        .count()
        .saturating_sub(sample.chars().count());
    (
        sample,
        omitted_chars,
        frame_count(message).saturating_sub(retained_frames),
    )
}

fn complete_frame_count(sample: &str, original: &str) -> usize {
    let mut count = frame_count(sample);
    if sample.len() < original.len()
        && !sample.ends_with('\n')
        && !original[sample.len()..].starts_with('\n')
        && sample.lines().last().is_some_and(is_stack_frame)
    {
        count = count.saturating_sub(1);
    }
    count
}

/// Reserve scope, totals, impact and omission counts, then share detail space across clusters.
/// Mandatory metadata is retained even when it exceeds the requested character budget.
pub fn format_bounded_errors_text(
    report: &ErrorAnalysisReport,
    options: &ErrorsOptions,
    limits: ErrorReportLimits,
    scope: &str,
) -> String {
    let mut out = format!(
        "{}: {} entries ({} patterns) across {} {}\n{}\nImpact summary\n  Total errors: {}\n  Total warnings: {}\n  Unique error patterns: {}\n  Affected sessions: {}\n",
        if report.warn_count > 0 {
            "ERRORS/WARNS"
        } else {
            "ERRORS"
        },
        report.total_entries,
        report.unique_patterns,
        report.file_count,
        if report.file_count == 1 {
            "file"
        } else {
            "files"
        },
        scope,
        report.error_count,
        report.warn_count,
        report.unique_patterns,
        report.affected_sessions_count,
    );
    if let Some(longest) = &report.longest_blocking {
        let pattern = char_prefix(&longest.pattern, limits.max_sample_chars.min(120));
        let _ = writeln!(
            out,
            "  Longest blocking error: {} [{}] {}{}\n  Session: {}",
            format_duration_approx(longest.duration_ms),
            longest.severity,
            pattern,
            if pattern.len() < longest.pattern.len() {
                " [pattern truncated]"
            } else {
                ""
            },
            longest.session_path
        );
    } else {
        out.push_str("  Longest blocking error: n/a\n");
    }
    let _ = writeln!(
        out,
        "Budget: {} Unicode characters including newlines; mandatory metadata may exceed budget.",
        limits.max_output_chars
    );
    if report.total_entries == 0 {
        out.push_str("No matching ERROR/WARN entries found.\n");
    }

    let mut omitted = DetailOmissions {
        clusters: report.clusters.len(),
        sample_chars: report
            .clusters
            .iter()
            .map(|cluster| cluster.sample_message.chars().count())
            .sum(),
        stack_frames: report
            .clusters
            .iter()
            .map(|cluster| frame_count(&cluster.sample_message))
            .sum(),
        pattern_chars: report
            .clusters
            .iter()
            .map(|cluster| cluster.pattern.chars().count())
            .sum(),
        session_details: if options.show_sessions {
            report
                .clusters
                .iter()
                .map(|cluster| cluster.affected_sessions.len())
                .sum()
        } else {
            0
        },
    };
    let footer_reserve = omitted.text().chars().count();
    let mandatory_chars = out.chars().count() + footer_reserve;
    if mandatory_chars > limits.max_output_chars {
        out.push_str("Mandatory metadata exceeds budget; no cluster details shown.\n");
    }
    let mut remaining = limits.max_output_chars.saturating_sub(mandatory_chars);
    let mut displayed = Vec::new();
    let show_date = spans_multiple_dates(report);

    // Put every overview that fits before any samples or session detail rows.
    for (index, cluster) in report
        .clusters
        .iter()
        .take(displayed_cluster_count(report, options))
        .enumerate()
    {
        let pattern = char_prefix(&cluster.pattern, limits.max_sample_chars.min(120));
        let components = cluster.components.join(", ");
        let heading = format!(
            "\n #{:<2} [{}] ×{} {}{}\n     {}{}\n     First: {}  Last: {}; affected sessions: {}\n",
            index + 1,
            cluster.severity,
            cluster.count,
            char_prefix(&components, 80),
            if components.chars().count() > 80 {
                "..."
            } else {
                ""
            },
            pattern,
            if pattern.len() < cluster.pattern.len() {
                " [pattern truncated]"
            } else {
                ""
            },
            format_timestamp(cluster.first_timestamp, show_date),
            format_timestamp(cluster.last_timestamp, show_date),
            cluster.affected_sessions_count,
        );
        let cost = heading.chars().count();
        if cost > remaining {
            break;
        }
        out.push_str(&heading);
        remaining -= cost;
        omitted.clusters -= 1;
        omitted.pattern_chars -= pattern.chars().count();
        displayed.push((index, cluster));
    }
    let count = displayed.len();
    for (position, (index, cluster)) in displayed.into_iter().enumerate() {
        let mut share = remaining / (count - position);
        let sample_label = format!("\n Sample #{}: ", index + 1);
        let overhead = sample_label.chars().count() + 1;
        if share > overhead {
            let (sample, _, _) = bounded_sample(
                &cluster.sample_message,
                ErrorReportLimits {
                    max_sample_chars: limits.max_sample_chars.min(share - overhead),
                    ..limits
                },
            );
            if !sample.is_empty() {
                let detail = format!("{sample_label}{sample}\n");
                let cost = detail.chars().count();
                out.push_str(&detail);
                share -= cost;
                remaining -= cost;
                omitted.sample_chars -= sample.chars().count();
                let (filtered, _, _) = bounded_sample(
                    &cluster.sample_message,
                    ErrorReportLimits {
                        max_sample_chars: usize::MAX,
                        ..limits
                    },
                );
                omitted.stack_frames -= complete_frame_count(&sample, &filtered);
            }
        }
        if options.show_sessions {
            for session in &cluster.affected_sessions {
                let row = format!(
                    "     Session #{}: {} ×{} {}{}\n",
                    index + 1,
                    session.session_path,
                    session.error_count,
                    session.outcome.as_label(),
                    session
                        .blocking_ms
                        .filter(|ms| *ms > 0)
                        .map(|ms| format!(" ({})", format_duration_approx(ms)))
                        .unwrap_or_default()
                );
                let cost = row.chars().count();
                if cost > share {
                    break;
                }
                out.push_str(&row);
                share -= cost;
                remaining -= cost;
                omitted.session_details -= 1;
            }
        }
    }
    out.push_str(&omitted.text());
    out
}

pub fn format_errors_json(report: &ErrorAnalysisReport, options: &ErrorsOptions) -> String {
    let display_limit = displayed_cluster_count(report, options);
    let mut omitted = DetailOmissions::default();
    let clusters: Vec<_> = report
        .clusters
        .iter()
        .take(display_limit)
        .map(|cluster| {
            let mut displayed = cluster.clone();
            if let Some(limits) = options.limits {
                let (sample, sample_chars, frames) =
                    bounded_sample(&cluster.sample_message, limits);
                displayed.sample_message = sample;
                let pattern = char_prefix(&cluster.pattern, limits.max_sample_chars);
                displayed.pattern = pattern.to_string();
                omitted.sample_chars += sample_chars;
                omitted.stack_frames += frames;
                omitted.pattern_chars += cluster.pattern.chars().count() - pattern.chars().count();
            }
            displayed
        })
        .collect();
    omitted.clusters = report.clusters.len() - display_limit;
    for cluster in report.clusters.iter().skip(display_limit) {
        omitted.sample_chars += cluster.sample_message.chars().count();
        omitted.stack_frames += frame_count(&cluster.sample_message);
        omitted.pattern_chars += cluster.pattern.chars().count();
    }
    let mut longest_blocking = report.longest_blocking.clone();
    let mut longest_pattern_chars_omitted = 0;
    if let (Some(longest), Some(limits)) = (&mut longest_blocking, options.limits) {
        let shortened = char_prefix(&longest.pattern, limits.max_sample_chars).to_string();
        longest_pattern_chars_omitted = longest.pattern.chars().count() - shortened.chars().count();
        longest.pattern = shortened;
    }
    serde_json::to_string_pretty(&json!({
        "errors": {
            "summary": {
                "file_count": report.file_count,
                "include_warn": report.include_warn,
                "total_entries": report.total_entries,
                "error_count": report.error_count,
                "warn_count": report.warn_count,
                "unique_patterns": report.unique_patterns,
                "affected_sessions_count": report.affected_sessions_count,
                "longest_blocking": longest_blocking,
                "longest_blocking_pattern_chars_omitted": longest_pattern_chars_omitted,
            },
            "options": {
                "top_n": options.top_n,
                "show_sessions": options.show_sessions,
                "limits": options.limits,
                "output_budget_applies_to": "text",
                "sort_by": format!("{:?}", options.sort_by).to_ascii_lowercase(),
            },
            "clusters_total": report.clusters.len(),
            "clusters_displayed": display_limit,
            "clusters": clusters,
            "omitted": omitted,
        }
    }))
    .unwrap_or_else(|_| "{\"errors\":{\"error\":\"failed to serialize errors output\"}}".into())
}

fn finalize_cluster(
    accum: ClusterAccum,
    session_states: &HashMap<String, SessionLifecycleState>,
    longest_blocking: &mut Option<LongestBlockingError>,
) -> ErrorClusterReport {
    let mut affected_sessions = Vec::with_capacity(accum.session_counts.len());
    let mut blocking_ms = None;

    for (session_path, error_count) in accum.session_counts {
        let first_error_timestamp = accum
            .session_first_error
            .get(&session_path)
            .cloned()
            .unwrap_or(accum.first_timestamp);
        let last_error_timestamp = accum
            .session_last_error
            .get(&session_path)
            .cloned()
            .unwrap_or(accum.last_timestamp);

        let (outcome, terminal_time) = if let Some(state) = session_states.get(&session_path) {
            (
                if state.orphaned {
                    SessionOutcome::Orphaned
                } else {
                    SessionOutcome::Completed
                },
                Some(state.last_seen),
            )
        } else {
            (SessionOutcome::Completed, None)
        };

        let session_blocking_ms = terminal_time.map(|terminal| {
            terminal
                .signed_duration_since(first_error_timestamp)
                .num_milliseconds()
                .max(0)
        });

        if let Some(ms) = session_blocking_ms {
            blocking_ms = Some(blocking_ms.map_or(ms, |current: i64| current.max(ms)));
            if longest_blocking
                .as_ref()
                .is_none_or(|current| ms > current.duration_ms)
            {
                *longest_blocking = Some(LongestBlockingError {
                    severity: accum.severity.clone(),
                    pattern: accum.pattern.clone(),
                    session_path: session_path.clone(),
                    duration_ms: ms,
                });
            }
        }

        affected_sessions.push(ClusterSessionImpact {
            session_path,
            error_count,
            outcome,
            first_error_timestamp,
            last_error_timestamp,
            blocking_ms: session_blocking_ms,
        });
    }

    affected_sessions.sort_by(|a, b| {
        b.error_count
            .cmp(&a.error_count)
            .then_with(|| outcome_sort_rank(a.outcome).cmp(&outcome_sort_rank(b.outcome)))
            .then_with(|| a.session_path.cmp(&b.session_path))
    });

    ErrorClusterReport {
        severity: accum.severity,
        pattern: accum.pattern,
        count: accum.count,
        components: accum.components.into_iter().collect(),
        first_timestamp: accum.first_timestamp,
        last_timestamp: accum.last_timestamp,
        sample_message: accum.sample_message,
        affected_sessions_count: affected_sessions.len(),
        affected_sessions,
        blocking_ms,
    }
}

fn build_session_lifecycle_states(
    logs: &[&LogEntry],
    orphans: &[OrphanOperation],
) -> HashMap<String, SessionLifecycleState> {
    let mut states: HashMap<String, SessionLifecycleState> = HashMap::new();

    for entry in logs.iter().copied() {
        if !entry.component_id.is_empty() {
            states
                .entry(entry.component_id.clone())
                .and_modify(|state| {
                    if entry.timestamp > state.last_seen {
                        state.last_seen = entry.timestamp;
                    }
                })
                .or_insert(SessionLifecycleState {
                    last_seen: entry.timestamp,
                    orphaned: false,
                });
        }
    }

    for orphan in orphans {
        let Some(component_id) = orphan.component_id.as_ref() else {
            continue;
        };
        states
            .entry(component_id.clone())
            .and_modify(|state| state.orphaned = true);
    }

    states
}

fn build_error_level_filter(include_warn: bool) -> LogFilter {
    let filter = LogFilter::new().with_level(Some("ERROR"));
    if include_warn {
        filter.with_level(Some("WARN"))
    } else {
        filter
    }
}

fn normalized_severity(level: &str) -> String {
    let upper = level.trim().to_ascii_uppercase();
    if upper.starts_with("WARN") {
        "WARN".to_string()
    } else {
        "ERROR".to_string()
    }
}

fn normalize_message_pattern(message: &str) -> String {
    let mut normalized = message.replace('\n', " ");
    normalized = URL_RE.replace_all(&normalized, "...").into_owned();
    normalized = UUID_RE.replace_all(&normalized, "...").into_owned();
    normalized = ISO_TIMESTAMP_RE
        .replace_all(&normalized, "...")
        .into_owned();
    normalized = CLOCK_RE.replace_all(&normalized, "...").into_owned();
    normalized = REQUEST_ID_BRACKET_RE
        .replace_all(&normalized, "[...]")
        .into_owned();
    normalized = ID_QUOTED_RE
        .replace_all(&normalized, "$1 \"...\"")
        .into_owned();
    normalized = ID_SQUOTED_RE
        .replace_all(&normalized, "$1 '...'")
        .into_owned();
    normalized = ID_BARE_RE.replace_all(&normalized, "$1 ...").into_owned();
    normalized = LONG_HEX_RE.replace_all(&normalized, "...").into_owned();
    normalized = LONG_NUMBER_RE.replace_all(&normalized, "...").into_owned();
    normalized = MULTISPACE_RE.replace_all(&normalized, " ").into_owned();
    normalized.trim().to_string()
}

fn sort_clusters(clusters: &mut [ErrorClusterReport], sort_by: ErrorsSortBy) {
    clusters.sort_by(|a, b| match sort_by {
        ErrorsSortBy::Count => b
            .count
            .cmp(&a.count)
            .then_with(|| b.affected_sessions_count.cmp(&a.affected_sessions_count))
            .then_with(|| b.last_timestamp.cmp(&a.last_timestamp))
            .then_with(|| a.pattern.cmp(&b.pattern)),
        ErrorsSortBy::Time => b
            .last_timestamp
            .cmp(&a.last_timestamp)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.pattern.cmp(&b.pattern)),
        ErrorsSortBy::Impact => b
            .affected_sessions_count
            .cmp(&a.affected_sessions_count)
            .then_with(|| {
                b.blocking_ms
                    .unwrap_or_default()
                    .cmp(&a.blocking_ms.unwrap_or_default())
            })
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| b.last_timestamp.cmp(&a.last_timestamp))
            .then_with(|| a.pattern.cmp(&b.pattern)),
    });
}

fn displayed_cluster_count(report: &ErrorAnalysisReport, options: &ErrorsOptions) -> usize {
    if options.top_n == 0 {
        report.clusters.len()
    } else {
        options.top_n.min(report.clusters.len())
    }
}

fn outcome_sort_rank(outcome: SessionOutcome) -> u8 {
    match outcome {
        SessionOutcome::Orphaned => 0,
        SessionOutcome::Completed => 1,
    }
}

fn spans_multiple_dates(report: &ErrorAnalysisReport) -> bool {
    let Some(first) = report.clusters.first() else {
        return false;
    };
    let min_date = first.first_timestamp.date_naive();
    let max_date = report
        .clusters
        .iter()
        .map(|cluster| cluster.last_timestamp.date_naive())
        .max()
        .unwrap_or(min_date);
    min_date != max_date
}

fn format_timestamp(ts: DateTime<Local>, show_date: bool) -> String {
    if show_date {
        ts.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    } else {
        ts.format("%H:%M:%S%.3f").to_string()
    }
}

fn format_duration_approx(duration_ms: i64) -> String {
    if duration_ms <= 0 {
        return "~0s".to_string();
    }

    let total_seconds = ((duration_ms + 500) / 1000).max(1);
    if total_seconds < 60 {
        return format!("~{}s", total_seconds);
    }

    let total_minutes = (total_seconds + 30) / 60;
    if total_minutes < 120 {
        return format!("~{}min", total_minutes);
    }

    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if minutes == 0 {
        format!("~{}h", hours)
    } else {
        format!("~{}h {}m", hours, minutes)
    }
}

impl SessionOutcome {
    fn as_label(self) -> &'static str {
        match self {
            SessionOutcome::Completed => "completed",
            SessionOutcome::Orphaned => "orphaned",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_dynamic_tokens() {
        let input = "Render with id \"5bfcc412-1fd6-4f8d-a6d5-246f90f3e7ab\" failed at 2026-02-25T18:34:01.220Z (retry 1708888888) https://example.test/x?id=42";
        let normalized = normalize_message_pattern(input);
        assert_eq!(
            normalized,
            "Render with id \"...\" failed at ... (retry ...) ..."
        );
    }

    #[test]
    fn normalizes_request_ids_in_brackets() {
        let input = "Request \"check\" [0--f227f11e-aaaa-bbbb-cccc-1234567890ab] failed";
        let normalized = normalize_message_pattern(input);
        assert_eq!(normalized, "Request \"check\" [...] failed");
    }
}
