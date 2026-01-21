use super::entities::{PerfAnalysisResults, TimedOperation};
use crate::cli::PerfSortOrder;

/// Display performance analysis results in text format
pub fn display_perf_results(
    results: &PerfAnalysisResults,
    threshold_ms: u64,
    top_n: usize,
    orphans_only: bool,
    sort_by: PerfSortOrder,
) {
    if orphans_only {
        display_orphans_only(results);
        return;
    }

    // 1. Summary section
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║           PERFORMANCE ANALYSIS SUMMARY                    ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Total log entries analyzed: {}", results.total_entries);
    println!("Completed operations:       {}", results.operations.len());
    println!("Orphaned operations:        {}", results.orphans.len());

    if let Some((start, end)) = results.time_range {
        let duration = end.signed_duration_since(start);
        println!(
            "Time range:                 {} to {}",
            start.format("%H:%M:%S%.3f"),
            end.format("%H:%M:%S%.3f")
        );
        println!(
            "Total duration:             {:.3}s",
            duration.num_milliseconds() as f64 / 1000.0
        );
    }
    println!();

    // 2. Statistics table
    if !results.stats.is_empty() {
        println!("╔════════════════════════════════════════════════════════════╗");
        println!("║           OPERATION STATISTICS                             ║");
        println!("╚════════════════════════════════════════════════════════════╝");
        println!();
        println!(
            "{:<12} {:<30} {:>8} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "Type",
            "Operation",
            "Count",
            "Avg(ms)",
            "Min(ms)",
            "Max(ms)",
            "P50(ms)",
            "P95(ms)",
            "P99(ms)"
        );
        println!("{}", "─".repeat(138));

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
            println!(
                "{:<12} {:<30} {:>8} {:>10.2} {:>10} {:>10} {:>10} {:>10} {:>10}",
                stat.op_type,
                truncate_string(&stat.name, 30),
                stat.count,
                stat.avg_duration_ms,
                stat.min_duration_ms,
                stat.max_duration_ms,
                stat.p50_duration_ms,
                stat.p95_duration_ms,
                stat.p99_duration_ms,
            );
        }
        println!();
    }

    // 3. Top N slowest operations
    if !results.operations.is_empty() {
        println!("╔════════════════════════════════════════════════════════════╗");
        println!(
            "║           TOP {} SLOWEST OPERATIONS                       ║",
            top_n
        );
        println!("╚════════════════════════════════════════════════════════════╝");
        println!();

        let top_ops = results.top_slowest_operations(top_n);
        for (i, op) in top_ops.iter().enumerate() {
            display_timed_operation(i + 1, op);
        }
        println!();
    }

    // 4. Threshold violations
    let violations = results.operations_exceeding_threshold(threshold_ms);
    if !violations.is_empty() {
        println!("╔════════════════════════════════════════════════════════════╗");
        println!(
            "║      OPERATIONS EXCEEDING THRESHOLD ({}ms)            ║",
            threshold_ms
        );
        println!("╚════════════════════════════════════════════════════════════╝");
        println!();
        println!(
            "Found {} operation(s) exceeding {}ms threshold",
            violations.len(),
            threshold_ms
        );
        println!();

        for (i, op) in violations.iter().take(20).enumerate() {
            display_timed_operation(i + 1, op);
        }

        if violations.len() > 20 {
            println!("... and {} more operations", violations.len() - 20);
        }
        println!();
    }

    // 5. Orphaned operations
    if !results.orphans.is_empty() {
        println!("╔════════════════════════════════════════════════════════════╗");
        println!("║           ORPHANED OPERATIONS                              ║");
        println!("╚════════════════════════════════════════════════════════════╝");
        println!();
        println!(
            "Operations that started but never completed: {}",
            results.orphans.len()
        );
        println!();

        for (i, orphan) in results.orphans.iter().take(10).enumerate() {
            println!(
                "{}. [{}] {} - {}",
                i + 1,
                orphan.op_type,
                orphan.name,
                orphan.component
            );
            println!("   Started: {}", orphan.start_time.format("%H:%M:%S%.3f"));
            if let Some(ref corr_id) = orphan.correlation_id {
                println!("   Correlation ID: {}", corr_id);
            }
            println!("   Context: {}", truncate_string(&orphan.context, 80));
            println!();
        }

        if results.orphans.len() > 10 {
            println!(
                "... and {} more orphaned operations",
                results.orphans.len() - 10
            );
        }
        println!();
    }
}

/// Display only orphaned operations
fn display_orphans_only(results: &PerfAnalysisResults) {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║           ORPHANED OPERATIONS                              ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    if results.orphans.is_empty() {
        println!("No orphaned operations found!");
        return;
    }

    println!("Total orphaned operations: {}", results.orphans.len());
    println!();

    for (i, orphan) in results.orphans.iter().enumerate() {
        println!(
            "{}. [{}] {} - {}",
            i + 1,
            orphan.op_type,
            orphan.name,
            orphan.component
        );
        println!(
            "   Started: {}",
            orphan.start_time.format("%Y-%m-%d %H:%M:%S%.3f")
        );
        if let Some(ref corr_id) = orphan.correlation_id {
            println!("   Correlation ID: {}", corr_id);
        }
        println!("   Context: {}", truncate_string(&orphan.context, 100));
        println!();
    }
}

/// Display a single timed operation
fn display_timed_operation(index: usize, op: &TimedOperation) {
    println!(
        "{}. [{}] {} - {}ms",
        index, op.op_type, op.name, op.duration_ms
    );
    println!("   {} → {}", op.start_component, op.end_component);
    println!(
        "   {} → {}",
        op.start_time.format("%H:%M:%S%.3f"),
        op.end_time.format("%H:%M:%S%.3f")
    );

    if let Some(ref endpoint) = op.endpoint {
        println!("   Endpoint: {}", endpoint);
    }

    if let Some(ref status) = op.status {
        println!("   Status: {}", status);
    }

    if let Some(ref corr_id) = op.correlation_id {
        println!("   Correlation ID: {}", truncate_string(corr_id, 50));
    }

    println!();
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
