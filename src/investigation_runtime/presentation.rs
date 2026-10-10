use serde_json::Value;
use std::fmt::Write;

/// Bind occurrences only after all independent input identities are available.
pub(super) fn bind_snapshot(value: &mut Value, snapshot: &Value) {
    match value {
        Value::Object(object) => {
            if object
                .get("snapshot_id")
                .is_some_and(|id| id == "pending-snapshot")
                && object.contains_key("evidence_ref")
            {
                object.insert("snapshot_id".into(), snapshot.clone());
            }
            for (key, child) in object.iter_mut() {
                // Original payload and classification trees are untrusted source data,
                // even when they imitate an occurrence address.
                if key != "fields" {
                    bind_snapshot(child, snapshot);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                bind_snapshot(child, snapshot);
            }
        }
        _ => (),
    }
}
fn label(value: &Value) -> &str {
    value.as_str().unwrap_or("unknown")
}
pub(super) fn text(report: &Value) -> String {
    let mut output = String::new();
    let incomplete = report["scopes"].as_array().is_some_and(|scopes| {
        scopes
            .iter()
            .any(|scope| scope["completeness"] != "complete")
    });
    let _ = writeln!(
        output,
        "Investigation: {} (processing {})",
        if incomplete {
            "partial"
        } else {
            label(&report["processing"]["status"])
        },
        label(&report["processing"]["status"])
    );
    if !report["processing"]["stop"].is_null() {
        let stop = &report["processing"]["stop"];
        let _ = writeln!(
            output,
            "Stopped during {}: {} ({})",
            label(&stop["stage"]),
            label(&stop["reason"]),
            label(&stop["limit_name"])
        );
    }
    let _ = writeln!(
        output,
        "Profile: {}",
        label(&report["report_metadata"]["active_profile"])
    );
    if let Some(selection) =
        report.pointer("/report_metadata/evidence/query/execution/profile_selection")
    {
        let _ = writeln!(output, "Profile selection: {}", label(&selection["status"]));
    }
    if let Some(inputs) = report["processing"]["inputs"].as_array() {
        for input in inputs {
            let ordinal = input["input_ordinal"].as_u64().unwrap_or(0) as usize;
            let coverage = &report["report_metadata"]["evidence"]["inputs"][ordinal];
            let _ = writeln!(
                output,
                "Input {}: {} — {} bytes, {} parsed records ({})",
                ordinal + 1,
                coverage["file"]
                    .as_str()
                    .or_else(|| report
                        .pointer("/report_metadata/evidence/query/execution/source_locations")
                        .and_then(Value::as_array)
                        .and_then(|paths| paths.get(ordinal))
                        .and_then(Value::as_str))
                    .unwrap_or("unavailable source path"),
                input["consumed_bytes"],
                coverage["coverage"]["parsed_entries"],
                label(&input["capture"])
            );
            if let Some(rejected) = coverage
                .pointer("/coverage/rejected_candidates")
                .and_then(Value::as_u64)
                .filter(|count| *count > 0)
            {
                let _ = writeln!(
                    output,
                    "  Rejected candidates: {rejected}; analysis covers parsed records only."
                );
            }
            if coverage
                .pointer("/coverage/nonempty_lines")
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0)
                && coverage.pointer("/coverage/parsed_entries") == Some(&Value::from(0))
            {
                output.push_str(
                    "  Nonempty input was not parsed; issue assessment is unavailable.\n",
                );
            }
        }
    }
    output.push_str("\nObserved findings (selected processed records):\n");
    if let Some(findings) = report["findings"].as_array() {
        for finding in findings {
            let _ = writeln!(
                output,
                "- [{}] {}: {}",
                label(&finding["scope_id"]),
                label(&finding["kind"]),
                label(&finding["claim"])
            );
            if !finding["details"]["value"].is_null() {
                let _ = writeln!(
                    output,
                    "  Value: {} {}",
                    finding["details"]["value"],
                    label(&finding["details"]["unit"])
                );
            }
            if let Some(excerpts) = finding["evidence"].as_array() {
                for excerpt in excerpts.iter().take(2) {
                    let reference = &excerpt["occurrence"]["evidence_ref"];
                    let _ = writeln!(
                        output,
                        "  Input {}, line {} [{}]: {}",
                        excerpt["occurrence"]["input_ordinal"]
                            .as_u64()
                            .map_or_else(|| "unknown".into(), |ordinal| (ordinal + 1).to_string()),
                        reference["line"],
                        reference["reference_id"].as_str().unwrap_or("source"),
                        label(&excerpt["text"]).replace(['\n', '\r'], " ")
                    );
                }
            }
        }
        if findings.is_empty() {
            output.push_str(
                "No findings displayed; check processing coverage and presentation limits.\n",
            );
        }
    }
    output.push_str("\nAssessment limits:\n");
    if let Some(assessments) = report["assessments"].as_array() {
        for assessment in assessments
            .iter()
            .filter(|assessment| assessment["status"] != "supported")
        {
            let _ = writeln!(
                output,
                "- {} / {}: {}. {}",
                label(&assessment["scope_id"]),
                label(&assessment["goal"]),
                label(&assessment["status"]),
                label(&assessment["reason"])
            );
        }
    }
    output.push_str("No causal root cause is inferred from counts or resource fingerprints.\n");
    if report["presentation"]["omitted_findings"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        let _ = writeln!(
            output,
            "{} findings omitted from this page; retained evidence is available separately.",
            report["presentation"]["omitted_findings"]
        );
    }
    if report["artifact"]["status"] != "unavailable" {
        let _ = writeln!(
            output,
            "\nEvidence: {}\nSHA-256: {}",
            label(&report["artifact"]["location"]),
            label(&report["artifact"]["stored_sha256"])
        );
        let _ = writeln!(
            output,
            "Next: log-analyzer evidence '{}' --expected-sha256 {} --collection /findings{}",
            label(&report["artifact"]["location"]),
            label(&report["artifact"]["stored_sha256"]),
            report["retrieval"]["next_cursor"]
                .as_str()
                .map_or_else(String::new, |cursor| format!(" --report-cursor {cursor}"))
        );
    } else {
        output.push_str(
            "Evidence artifact unavailable; retrieval cannot recover omitted findings.\n",
        );
    }
    output
}
