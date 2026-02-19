use super::entities::{PerfAnalysisResults, TimedOperation};
use crate::cli::PerfSortOrder;
use crate::comparator::create_styled_table;
use comfy_table::Cell;
use std::fmt::Write as _;

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
        results.operations.len()
    );
    let _ = writeln!(out, "Orphaned operations:        {}", results.orphans.len());

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

        let mut stats = results.stats.clone();
        match sort_by {
            PerfSortOrder::Duration => {
                stats.sort_by(|a, b| b.avg_duration_ms.partial_cmp(&a.avg_duration_ms).unwrap());
            }
            PerfSortOrder::Count => {
                stats.sort_by(|a, b| b.count.cmp(&a.count));
            }
            PerfSortOrder::Name => {
                stats.sort_by(|a, b| a.name.cmp(&b.name));
            }
        }

        for stat in stats.iter().take(top_n) {
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
            "║           TOP {} SLOWEST OPERATIONS                       ║",
            top_n
        );
        let _ = writeln!(
            out,
            "╚════════════════════════════════════════════════════════════╝"
        );
        let _ = writeln!(out);

        let top_ops = results.top_slowest_operations(top_n);
        for (i, op) in top_ops.iter().enumerate() {
            write_timed_operation(&mut out, i + 1, op);
        }
        let _ = writeln!(out);
    }

    // 4. Threshold violations
    let violations = results.operations_exceeding_threshold(threshold_ms);
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
            violations.len(),
            threshold_ms
        );
        let _ = writeln!(out);

        for (i, op) in violations.iter().take(20).enumerate() {
            write_timed_operation(&mut out, i + 1, op);
        }

        if violations.len() > 20 {
            let _ = writeln!(out, "... and {} more operations", violations.len() - 20);
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
            "Operations that started but never completed: {}",
            results.orphans.len()
        );
        let _ = writeln!(out);

        for (i, orphan) in results.orphans.iter().take(10).enumerate() {
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

        if results.orphans.len() > 10 {
            let _ = writeln!(
                out,
                "... and {} more orphaned operations",
                results.orphans.len() - 10
            );
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

    let _ = writeln!(out, "Total orphaned operations: {}", results.orphans.len());
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
