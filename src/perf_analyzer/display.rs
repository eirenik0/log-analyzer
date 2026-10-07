use super::entities::{PerfAnalysisResults, TimedOperation};
use crate::cli::PerfSortOrder;
use crate::comparator::create_styled_table;
use comfy_table::Cell;
use std::fmt::Write as _;

#[derive(serde::Serialize)]
pub struct PerfCounts {
    pub operations: usize,
    pub stats: usize,
    pub orphans: usize,
    pub threshold_violations: usize,
    pub unmatched_events: usize,
    pub ambiguous_groups: usize,
}

#[derive(serde::Serialize)]
pub struct PerfReport {
    #[serde(flatten)]
    pub results: PerfAnalysisResults,
    pub threshold_ms: u64,
    pub threshold_violations: Vec<TimedOperation>,
    pub totals: PerfCounts,
    pub omitted: PerfCounts,
}

fn select_results(
    results: &PerfAnalysisResults,
    threshold_ms: u64,
    top_n: usize,
    orphans_only: bool,
    sort_by: PerfSortOrder,
) -> PerfReport {
    let mut selected = results.clone();
    selected.stats.sort_by(|a, b| {
        let primary = match sort_by {
            PerfSortOrder::Duration => b.avg_duration_ms.total_cmp(&a.avg_duration_ms),
            PerfSortOrder::Count => b.count.cmp(&a.count),
            PerfSortOrder::Name => a.name.cmp(&b.name),
        };
        primary
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.op_type.cmp(&b.op_type))
    });
    let counts: std::collections::HashMap<_, _> = results
        .stats
        .iter()
        .map(|stat| ((stat.op_type.as_str(), stat.name.as_str()), stat.count))
        .collect();
    selected.operations.sort_by(|a, b| {
        let primary = match sort_by {
            PerfSortOrder::Duration => b.duration_ms.cmp(&a.duration_ms),
            PerfSortOrder::Count => counts
                .get(&(b.op_type.as_str(), b.name.as_str()))
                .cmp(&counts.get(&(a.op_type.as_str(), a.name.as_str()))),
            PerfSortOrder::Name => a.name.cmp(&b.name),
        };
        primary
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.op_type.cmp(&b.op_type))
            .then_with(|| a.start_time.cmp(&b.start_time))
            .then_with(|| a.correlation_id.cmp(&b.correlation_id))
    });
    selected.orphans.sort_by(|a, b| {
        a.start_time
            .cmp(&b.start_time)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.correlation_id.cmp(&b.correlation_id))
    });
    let mut violations: Vec<_> = selected
        .operations
        .iter()
        .filter(|op| u64::try_from(op.duration_ms).is_ok_and(|duration| duration >= threshold_ms))
        .cloned()
        .collect();
    selected
        .unmatched_events
        .sort_by_key(|event| event.timestamp);
    let totals = PerfCounts {
        operations: results.operations.len(),
        stats: results.stats.len(),
        orphans: results.orphans.len(),
        threshold_violations: violations.len(),
        unmatched_events: results.unmatched_events.len(),
        ambiguous_groups: results.ambiguous_groups.len(),
    };
    if orphans_only {
        selected.operations.clear();
        selected.stats.clear();
        violations.clear();
    }
    if top_n > 0 {
        selected.operations.truncate(top_n);
        selected.stats.truncate(top_n);
        selected.orphans.truncate(top_n);
        violations.truncate(top_n);
        selected.unmatched_events.truncate(top_n);
        selected.ambiguous_groups.truncate(top_n);
    }
    let omitted = PerfCounts {
        operations: totals.operations - selected.operations.len(),
        stats: totals.stats - selected.stats.len(),
        orphans: totals.orphans - selected.orphans.len(),
        threshold_violations: totals.threshold_violations - violations.len(),
        unmatched_events: totals.unmatched_events - selected.unmatched_events.len(),
        ambiguous_groups: totals.ambiguous_groups - selected.ambiguous_groups.len(),
    };
    PerfReport {
        results: selected,
        threshold_ms,
        threshold_violations: violations,
        totals,
        omitted,
    }
}

fn write_selection_summary(out: &mut String, report: &PerfReport) {
    let _ = writeln!(
        out,
        "Full totals: {} completed operations, {} statistics groups, {} orphans, {} threshold violations",
        report.totals.operations,
        report.totals.stats,
        report.totals.orphans,
        report.totals.threshold_violations
    );
    let _ = writeln!(
        out,
        "Omitted: {} completed operations, {} statistics groups, {} orphans, {} threshold violations",
        report.omitted.operations,
        report.omitted.stats,
        report.omitted.orphans,
        report.omitted.threshold_violations
    );
}

/// Display performance analysis results in text format
pub fn display_perf_results(
    results: &PerfAnalysisResults,
    threshold_ms: u64,
    top_n: usize,
    orphans_only: bool,
    sort_by: PerfSortOrder,
) {
    let output = format_perf_results_text(results, threshold_ms, top_n, orphans_only, sort_by);
    print!("{output}");
}

/// Format performance analysis results as text.
pub fn format_perf_results_text(
    results: &PerfAnalysisResults,
    threshold_ms: u64,
    top_n: usize,
    orphans_only: bool,
    sort_by: PerfSortOrder,
) -> String {
    let mut out = String::new();
    let report = select_results(results, threshold_ms, top_n, orphans_only, sort_by);
    let results = &report.results;
    write_selection_summary(&mut out, &report);
    let _ = writeln!(
        out,
        "Correlation diagnostics: {} ambiguous groups, {} unmatched events",
        report.totals.ambiguous_groups, report.totals.unmatched_events
    );
    let _ = writeln!(
        out,
        "Omitted correlation diagnostics: {} ambiguous groups, {} unmatched events",
        report.omitted.ambiguous_groups, report.omitted.unmatched_events
    );
    for event in &results.unmatched_events {
        let _ = writeln!(
            out,
            "  [{}] {} {}: {} at {}:{} ({})",
            event.op_type,
            event.name,
            event.boundary,
            event.reason,
            event.source.file.as_deref().unwrap_or("<unknown>"),
            event.source.line,
            event.timestamp.to_rfc3339()
        );
    }

    if orphans_only {
        write_orphans_only(&mut out, results);
        return out;
    }

    // 1. Summary section
    let _ = writeln!(
        out,
        "╔════════════════════════════════════════════════════════════╗"
    );
    let _ = writeln!(
        out,
        "║           PERFORMANCE ANALYSIS SUMMARY                    ║"
    );
    let _ = writeln!(
        out,
        "╚════════════════════════════════════════════════════════════╝"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Total log entries analyzed: {}", results.total_entries);
    let _ = writeln!(
        out,
        "Completed operations:       {}",
        report.totals.operations
    );
    let _ = writeln!(out, "Orphaned operations:        {}", report.totals.orphans);

    if let Some((start, end)) = results.time_range {
        let duration = end.signed_duration_since(start);
        let _ = writeln!(
            out,
            "Time range:                 {} to {}",
            start.format("%H:%M:%S%.3f"),
            end.format("%H:%M:%S%.3f")
        );
        let _ = writeln!(
            out,
            "Total duration:             {:.3}s",
            duration.num_milliseconds() as f64 / 1000.0
        );
    }
    let _ = writeln!(out);

    // 2. Statistics table
    if !results.stats.is_empty() {
        let _ = writeln!(
            out,
            "╔════════════════════════════════════════════════════════════╗"
        );
        let _ = writeln!(
            out,
            "║           OPERATION STATISTICS                             ║"
        );
        let _ = writeln!(
            out,
            "╚════════════════════════════════════════════════════════════╝"
        );
        let _ = writeln!(out);

        let mut table = create_styled_table(&[
            "Type",
            "Operation",
            "Count",
            "Avg(ms)",
            "Min(ms)",
            "Max(ms)",
            "P50(ms)",
            "P95(ms)",
            "P99(ms)",
        ]);

        for stat in &results.stats {
            table.add_row(vec![
                Cell::new(&stat.op_type),
                Cell::new(truncate_string(&stat.name, 30)),
                Cell::new(stat.count),
                Cell::new(format!("{:.2}", stat.avg_duration_ms)),
                Cell::new(stat.min_duration_ms),
                Cell::new(stat.max_duration_ms),
                Cell::new(stat.p50_duration_ms),
                Cell::new(stat.p95_duration_ms),
                Cell::new(stat.p99_duration_ms),
            ]);
        }

        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    // 3. Top N slowest operations
    if !results.operations.is_empty() {
        let _ = writeln!(
            out,
            "╔════════════════════════════════════════════════════════════╗"
        );
        let _ = writeln!(
            out,
            "║           SELECTED OPERATIONS ({})                       ║",
            results.operations.len()
        );
        let _ = writeln!(
            out,
            "╚════════════════════════════════════════════════════════════╝"
        );
        let _ = writeln!(out);

        for (i, op) in results.operations.iter().enumerate() {
            write_timed_operation(&mut out, i + 1, op);
        }
        let _ = writeln!(out);
    }

    // 4. Threshold violations
    let violations = &report.threshold_violations;
    if !violations.is_empty() {
        let _ = writeln!(
            out,
            "╔════════════════════════════════════════════════════════════╗"
        );
        let _ = writeln!(
            out,
            "║      OPERATIONS EXCEEDING THRESHOLD ({}ms)            ║",
            threshold_ms
        );
        let _ = writeln!(
            out,
            "╚════════════════════════════════════════════════════════════╝"
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Found {} operation(s) exceeding {}ms threshold",
            report.totals.threshold_violations, threshold_ms
        );
        let _ = writeln!(out);

        for (i, op) in violations.iter().enumerate() {
            write_timed_operation(&mut out, i + 1, op);
        }

        let _ = writeln!(out);
    }

    // 5. Orphaned operations
    if !results.orphans.is_empty() {
        let _ = writeln!(
            out,
            "╔════════════════════════════════════════════════════════════╗"
        );
        let _ = writeln!(
            out,
            "║           ORPHANED OPERATIONS                              ║"
        );
        let _ = writeln!(
            out,
            "╚════════════════════════════════════════════════════════════╝"
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "Displayed operations that started but never completed: {}",
            results.orphans.len()
        );
        let _ = writeln!(out);

        for (i, orphan) in results.orphans.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}. [{}] {} - {}",
                i + 1,
                orphan.op_type,
                orphan.name,
                orphan.component
            );
            let _ = writeln!(
                out,
                "   Started: {}",
                orphan.start_time.format("%H:%M:%S%.3f")
            );
            if let Some(ref corr_id) = orphan.correlation_id {
                let _ = writeln!(out, "   Correlation ID: {}", corr_id);
            }
            let _ = writeln!(out, "   Context: {}", truncate_string(&orphan.context, 80));
            let _ = writeln!(out);
        }

        let _ = writeln!(out);
    }

    out
}

/// Display only orphaned operations
fn write_orphans_only(out: &mut String, results: &PerfAnalysisResults) {
    let _ = writeln!(
        out,
        "╔════════════════════════════════════════════════════════════╗"
    );
    let _ = writeln!(
        out,
        "║           ORPHANED OPERATIONS                              ║"
    );
    let _ = writeln!(
        out,
        "╚════════════════════════════════════════════════════════════╝"
    );
    let _ = writeln!(out);

    if results.orphans.is_empty() {
        let _ = writeln!(out, "No orphaned operations found!");
        return;
    }

    let _ = writeln!(
        out,
        "Displayed orphaned operations: {}",
        results.orphans.len()
    );
    let _ = writeln!(out);

    for (i, orphan) in results.orphans.iter().enumerate() {
        let _ = writeln!(
            out,
            "{}. [{}] {} - {}",
            i + 1,
            orphan.op_type,
            orphan.name,
            orphan.component
        );
        let _ = writeln!(
            out,
            "   Started: {}",
            orphan.start_time.format("%Y-%m-%d %H:%M:%S%.3f")
        );
        if let Some(ref corr_id) = orphan.correlation_id {
            let _ = writeln!(out, "   Correlation ID: {}", corr_id);
        }
        let _ = writeln!(out, "   Context: {}", truncate_string(&orphan.context, 100));
        let _ = writeln!(out);
    }
}

/// Display a single timed operation
fn write_timed_operation(out: &mut String, index: usize, op: &TimedOperation) {
    let _ = writeln!(
        out,
        "{}. [{}] {} - {}ms",
        index, op.op_type, op.name, op.duration_ms
    );
    let _ = writeln!(out, "   {} → {}", op.start_component, op.end_component);
    let _ = writeln!(
        out,
        "   {} → {}",
        op.start_time.format("%H:%M:%S%.3f"),
        op.end_time.format("%H:%M:%S%.3f")
    );

    let _ = writeln!(
        out,
        "   Source: {}:{} → {}:{}",
        op.start_source.file.as_deref().unwrap_or("<unknown>"),
        op.start_source.line,
        op.end_source.file.as_deref().unwrap_or("<unknown>"),
        op.end_source.line
    );
    if !op.scope.is_empty() {
        let _ = writeln!(out, "   Scope: {:?}", op.scope);
    }

    if let Some(ref endpoint) = op.endpoint {
        let _ = writeln!(out, "   Endpoint: {}", endpoint);
    }

    if let Some(ref status) = op.status {
        let _ = writeln!(out, "   Status: {}", status);
    }

    if let Some(ref corr_id) = op.correlation_id {
        let _ = writeln!(out, "   Correlation ID: {}", truncate_string(corr_id, 50));
    }

    let _ = writeln!(out);
}

/// Truncate a string to a maximum length with ellipsis
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Format performance analysis results as JSON
pub fn format_perf_results_json(results: &PerfAnalysisResults) -> String {
    serde_json::to_string_pretty(results).unwrap_or_else(|_| "{}".to_string())
}

/// Format the same selected rows, full totals, and omissions used in text output.
pub fn format_perf_results_json_with_options(
    results: &PerfAnalysisResults,
    threshold_ms: u64,
    top_n: usize,
    orphans_only: bool,
    sort_by: PerfSortOrder,
) -> String {
    serde_json::to_string_pretty(&select_results(
        results,
        threshold_ms,
        top_n,
        orphans_only,
        sort_by,
    ))
    .expect("performance report is serializable")
}
