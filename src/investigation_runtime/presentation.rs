use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

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
fn escape_controls(value: &str) -> String {
    value.chars().fold(String::new(), |mut output, character| {
        if character.is_control() {
            output.extend(character.escape_default());
        } else {
            output.push(character);
        }
        output
    })
}
const MAX_OVERVIEW_GROUPS: usize = 20;
use crate::investigation_view::{Aggregate, FindingGroup, InvestigationView, Measurement};

pub(super) struct FindingOverview {
    source_errors: String,
    unknowns: BTreeSet<(String, String)>,
    highlighted: BTreeSet<String>,
    rendered: String,
    groups: Vec<(FindingGroup, BTreeSet<String>)>,
}
/// Native source errors must remain visible when other findings fill the page.
fn source_errors(report: &Value, detailed: bool) -> (String, BTreeSet<String>) {
    let mut output = String::new();
    let mut highlighted = BTreeSet::new();
    let findings = report["findings"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    #[derive(Default)]
    struct Errors<'a> {
        counts: [Option<&'a Value>; 2],
        distinct: usize,
        samples: Vec<&'a Value>,
    }
    let mut inputs: BTreeMap<&str, Errors<'_>> = BTreeMap::new();
    for finding in findings {
        let Some(scope_id) = finding["scope_id"].as_str() else {
            continue;
        };
        let Some(suffix) = finding["id"]
            .as_str()
            .and_then(|id| id.strip_prefix(scope_id))
            .and_then(|id| id.strip_prefix("-observed-errors-"))
        else {
            continue;
        };
        let errors = inputs.entry(scope_id).or_default();
        if finding["kind"] == "observation" {
            errors.distinct += 1;
            if errors.samples.len() < 3 {
                errors.samples.push(finding);
            }
        } else if suffix == "count" {
            errors.counts[0] = Some(finding);
        } else if suffix == "normalized-count" {
            errors.counts[1] = Some(finding);
        }
    }
    for scope in report["scopes"].as_array().into_iter().flatten() {
        let scope_id = label(&scope["id"]);
        let Some(errors) = inputs.get(scope_id).filter(|errors| errors.distinct > 0) else {
            continue;
        };
        let count = errors.distinct;
        if output.is_empty() {
            output.push_str("\nObserved ERROR/FATAL records (selected parsed population):\n");
        }
        let _ = write!(
            output,
            "- [{}]",
            if detailed {
                scope_id.to_owned()
            } else {
                input_label(report, &scope["id"])
            }
        );
        for (finding, unit) in errors
            .counts
            .iter()
            .zip(["physical records", "normalized records"])
        {
            if let Some(finding) = finding {
                let _ = write!(output, " {} {unit};", finding["details"]["value"]);
                if let Some(id) = finding["id"].as_str() {
                    highlighted.insert(id.into());
                }
            }
        }
        let _ = writeln!(output, " {count} distinct source messages");
        for finding in &errors.samples {
            write_evidence(&mut output, finding, 1, detailed);
            if let Some(id) = finding["id"].as_str() {
                highlighted.insert(id.into());
            }
        }
        if count > 3 {
            let _ = writeln!(
                output,
                "  {} additional distinct source messages not displayed here.",
                count - 3
            );
        }
    }
    if !detailed {
        for finding in findings.iter().filter(|f| {
            f["id"]
                .as_str()
                .is_some_and(|id| id.ends_with("-observed-warnings-count"))
        }) {
            if finding["details"]["value"]
                .as_u64()
                .is_some_and(|count| count > 0)
            {
                let _ = writeln!(
                    output,
                    "{}: {} observed WARN records.",
                    input_label(report, &finding["scope_id"]),
                    finding["details"]["value"]
                );
            }
        }
    }
    if detailed && !output.is_empty() {
        output.push_str("At most 3 distinct message samples per input are shown, independently of the findings page. Source severity does not establish operation failure or cause.\n");
    }
    (output, highlighted)
}
fn group_key(finding: &Value, definition: &FindingGroup) -> Option<(String, Vec<Value>)> {
    if !definition.select.iter().all(|condition| {
        finding
            .pointer(&condition.path)
            .is_some_and(|value| value == &condition.equals)
    }) {
        return None;
    }
    let dimensions: Vec<_> = definition
        .by
        .iter()
        .map(|dimension| {
            finding
                .pointer(&dimension.path)
                .filter(|value| value.to_string().len() <= 4096)
                .cloned()
                .unwrap_or(Value::Null)
        })
        .collect();
    // Independent captures cannot be pooled, even when they share component names.
    Some((
        serde_json::json!([finding["scope_id"], dimensions]).to_string(),
        dimensions,
    ))
}
#[derive(Default)]
struct Measured {
    count: usize,
    missing: usize,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
}
impl Measured {
    fn add(&mut self, finding: &Value, metric: &Measurement) {
        let value = finding
            .pointer(&metric.path)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite());
        if finding.pointer(&metric.unit_path).and_then(Value::as_str) != Some(metric.unit.as_str())
            || value.is_none()
        {
            self.missing += 1;
            return;
        }
        let value = value.unwrap();
        self.count += 1;
        self.sum += value;
        self.min = Some(self.min.map_or(value, |min| min.min(value)));
        self.max = Some(self.max.map_or(value, |max| max.max(value)));
    }
    fn result(&self, aggregate: Aggregate) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        match aggregate {
            Aggregate::Min => self.min,
            Aggregate::Max => self.max,
            Aggregate::Sum => Some(self.sum).filter(|value| value.is_finite()),
            Aggregate::Mean => Some(self.sum / self.count as f64).filter(|value| value.is_finite()),
        }
    }
}
struct Group<'a> {
    key: String,
    representative: &'a Value,
    dimensions: Vec<Value>,
    count: usize,
    measurements: Vec<Measured>,
}
fn dimension_text(value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::String(value) => escape_controls(value),
        value => escape_controls(&value.to_string()),
    }
}
fn formatted_dimension(value: &Value, template: Option<&str>) -> String {
    let Some(template) = template.filter(|_| !value.is_null()) else {
        return dimension_text(value);
    };
    let mut rendered = String::new();
    let mut rest = template;
    while let Some((prefix, token)) = rest.split_once('{') {
        rendered.push_str(prefix);
        let Some((path, suffix)) = token.split_once('}') else {
            rendered.push_str(token);
            return escape_controls(&rendered);
        };
        rendered.push_str(&dimension_text(value.pointer(path).unwrap_or(&Value::Null)));
        rest = suffix;
    }
    rendered.push_str(rest);
    escape_controls(&rendered)
}
/// Profile-defined views of all processed findings; individual pages stay unchanged.
pub(super) fn finding_overview(
    report: &Value,
    view: Option<&InvestigationView>,
    redacted: bool,
    detailed: bool,
) -> FindingOverview {
    let (source_errors, highlighted) = source_errors(report, detailed);
    let mut overview = FindingOverview {
        source_errors,
        unknowns: report["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|finding| finding["kind"] == "unknown")
            .filter(|finding| {
                let scope = &finding["scope_id"];
                let suffix = label(&finding["id"])
                    .strip_prefix(label(scope))
                    .unwrap_or("");
                let goal = match suffix {
                    "-lifecycle-unavailable" | "-starts-unavailable" | "-ends-unavailable" => {
                        "incomplete_lifecycles"
                    }
                    "-failures-unavailable" | "-successes-unavailable" => "failures",
                    _ => return true,
                };
                !report["assessments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|assessment| {
                        assessment["scope_id"] == *scope
                            && assessment["goal"] == goal
                            && assessment["status"] != "supported"
                    })
            })
            .map(|finding| {
                (
                    input_label(report, &finding["scope_id"]),
                    escape_controls(label(&finding["claim"])),
                )
            })
            .collect(),
        highlighted,
        rendered: String::new(),
        groups: Vec::new(),
    };
    let mut displayed = 0;
    for definition in view.into_iter().flat_map(|view| &view.groups) {
        let mut groups: Vec<Group<'_>> = Vec::new();
        let mut omitted = 0;
        for finding in report["findings"].as_array().into_iter().flatten() {
            let Some((key, dimensions)) = group_key(finding, definition) else {
                continue;
            };
            let group = if let Some(index) = groups.iter().position(|group| group.key == key) {
                &mut groups[index]
            } else if groups.len() < definition.max_groups
                && displayed + groups.len() < MAX_OVERVIEW_GROUPS
            {
                groups.push(Group {
                    key,
                    representative: finding,
                    dimensions,
                    count: 0,
                    measurements: definition
                        .measurements
                        .iter()
                        .map(|_| Measured::default())
                        .collect(),
                });
                groups.last_mut().unwrap()
            } else {
                omitted += 1;
                continue;
            };
            group.count += 1;
            for (metric, values) in definition.measurements.iter().zip(&mut group.measurements) {
                values.add(finding, metric);
            }
        }
        if groups.is_empty() && omitted == 0 {
            continue;
        }
        let title = if redacted {
            "Configured findings"
        } else {
            &definition.title
        };
        let _ = writeln!(
            overview.rendered,
            "\n{} (processed findings):",
            escape_controls(title)
        );
        for group in &groups {
            let _ = write!(
                overview.rendered,
                "- [{}] ",
                if detailed {
                    label(&group.representative["scope_id"]).to_owned()
                } else {
                    input_label(report, &group.representative["scope_id"])
                }
            );
            for (index, (dimension, value)) in
                definition.by.iter().zip(&group.dimensions).enumerate()
            {
                if index > 0 {
                    overview.rendered.push_str(", ");
                }
                let name = if redacted {
                    format!("Field {}", index + 1)
                } else {
                    escape_controls(&dimension.label)
                };
                let _ = write!(
                    overview.rendered,
                    "{name}: {}",
                    formatted_dimension(value, dimension.template.as_deref().filter(|_| !redacted))
                );
            }
            let _ = writeln!(overview.rendered, " — {} findings", group.count);
            for (index, (metric, measured)) in definition
                .measurements
                .iter()
                .zip(&group.measurements)
                .enumerate()
            {
                let name = if redacted {
                    format!("Measurement {}", index + 1)
                } else {
                    escape_controls(&metric.label)
                };
                if let Some(value) = measured.result(metric.aggregate) {
                    let _ = writeln!(
                        overview.rendered,
                        "  {name}: {value:.2} {} ({} measured, {} unavailable)",
                        if redacted {
                            "configured unit".into()
                        } else {
                            escape_controls(&metric.unit)
                        },
                        measured.count,
                        measured.missing
                    );
                } else {
                    let _ = writeln!(
                        overview.rendered,
                        "  {name}: unknown ({} measured, {} unavailable)",
                        measured.count, measured.missing
                    );
                }
            }
            for excerpt in group.representative["evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .take(if detailed { 3 } else { 0 })
            {
                let reference = &excerpt["occurrence"]["evidence_ref"];
                let _ = writeln!(
                    overview.rendered,
                    "  Representative: Input {}, line {} [{}]",
                    excerpt["occurrence"]["input_ordinal"]
                        .as_u64()
                        .map_or_else(|| "unknown".into(), |ordinal| (ordinal + 1).to_string()),
                    reference["line"],
                    reference["reference_id"].as_str().unwrap_or("source")
                );
            }
        }
        if omitted > 0 {
            let _ = writeln!(
                overview.rendered,
                "{omitted} additional matching findings are outside the displayed groups (limit {}, {} groups overall).",
                definition.max_groups, MAX_OVERVIEW_GROUPS
            );
        }
        displayed += groups.len();
        overview.groups.push((
            definition.clone(),
            groups.into_iter().map(|group| group.key).collect(),
        ));
    }
    if detailed && !overview.rendered.is_empty() {
        overview.rendered.push_str("Groups may overlap. Counts are findings, not unique resources, records or failures. Measured summaries use only matching reported values; missing data stays unknown. Individual findings and citations remain in retained evidence.\n");
    }
    overview
}
fn write_evidence(output: &mut String, finding: &Value, max: usize, detailed: bool) {
    if let Some(excerpts) = finding["evidence"].as_array() {
        for excerpt in excerpts.iter().take(max) {
            let reference = &excerpt["occurrence"]["evidence_ref"];
            let _ = writeln!(
                output,
                "  Input {}, line {}{}{}: {}",
                excerpt["occurrence"]["input_ordinal"]
                    .as_u64()
                    .map_or_else(|| "unknown".into(), |ordinal| (ordinal + 1).to_string()),
                reference["line"],
                reference["row_path"]
                    .as_str()
                    .map_or_else(String::new, |path| format!(
                        ", row {}",
                        escape_controls(path)
                    )),
                if detailed {
                    format!(
                        " [{}]",
                        reference["reference_id"].as_str().unwrap_or("source")
                    )
                } else {
                    String::new()
                },
                escape_controls(label(&excerpt["text"]))
            );
        }
    }
}
fn source_location(occurrence: &Value) -> String {
    let reference = &occurrence["evidence_ref"];
    format!(
        "input {}, line {}{}",
        occurrence["input_ordinal"]
            .as_u64()
            .map_or_else(|| "unknown".into(), |n| (n + 1).to_string()),
        reference["line"],
        reference["row_path"]
            .as_str()
            .map_or_else(String::new, |p| format!(", row {}", escape_controls(p)))
    )
}

fn input_label(report: &Value, scope_id: &Value) -> String {
    let ordinals = report["scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|scope| scope["id"] == *scope_id)
        .and_then(|scope| scope["input_ordinals"].as_array());
    let inputs = ordinals
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|ordinal| (ordinal + 1).to_string())
        .collect::<Vec<_>>();
    format!(
        "Input {}",
        if inputs.is_empty() {
            "unknown".into()
        } else {
            inputs.join(", ")
        }
    )
}

fn readable_chain(output: &mut String, chain: &Value) {
    let _ = writeln!(
        output,
        "- {} ({}):",
        escape_controls(label(&chain["root_event_name"])),
        source_location(&chain["root"])
    );
    if chain["ambiguity"] != "none" {
        output.push_str("  Unknown: event identity is ambiguous; no reliable timing.\n");
    }
    if chain["completeness"] != "complete" {
        output.push_str("  Coverage is partial; additional events may be missing.\n");
    }
    let mut missing = Vec::new();
    for step in chain["steps"].as_array().into_iter().flatten() {
        let name = escape_controls(label(&step["label"])).replace('_', " ");
        if step["status"] == "not_observed" && step["count_completeness"] == "complete" {
            missing.push(name);
            continue;
        }
        if step["linked_count"] != 1 || step["count_completeness"] != "complete" {
            let count = step["observed_count"]
                .as_u64()
                .map_or_else(|| "unknown".into(), |n| n.to_string());
            let _ = writeln!(
                output,
                "  {name}: {count} observed; {}{}",
                label(&step["status"]).replace('_', " "),
                if step["count_completeness"] != "complete" {
                    "; count incomplete"
                } else {
                    ""
                }
            );
        }
        for sample in step["join_samples"].as_array().into_iter().flatten() {
            let interval = &sample["endpoint_interval"];
            let elapsed = if interval["status"] == "measured" {
                format!("{} {}", interval["value"], label(&interval["unit"]))
            } else {
                format!(
                    "unknown ({})",
                    escape_controls(label(&interval["reason"])).replace('_', " ")
                )
            };
            if sample["source"] == chain["root"] && interval["status"] == "measured" {
                let _ = writeln!(
                    output,
                    "  {}: +{elapsed} ({})",
                    escape_controls(label(&interval["end_operation"])),
                    source_location(&sample["target"])
                );
                continue;
            }
            let _ = writeln!(
                output,
                "  {} -> {}: {elapsed}; {} -> {}",
                escape_controls(label(&interval["start_operation"])),
                escape_controls(label(&interval["end_operation"])),
                source_location(&sample["source"]),
                source_location(&sample["target"])
            );
        }
        for outcome in step["outcomes"].as_array().into_iter().flatten() {
            let _ = writeln!(
                output,
                "  {} outcome: {} = {}",
                escape_controls(label(&outcome["level"])),
                escape_controls(label(&outcome["field"])),
                escape_controls(&outcome["value"].to_string())
            );
        }
        if step["omitted_occurrences"].as_u64().unwrap_or(0) > 0 {
            let _ = writeln!(
                output,
                "  {} {name} occurrences not shown.",
                step["omitted_occurrences"]
            );
        }
    }
    if !missing.is_empty() {
        let _ = writeln!(output, "  Unknown / not observed: {}.", missing.join(", "));
    }
}

pub(super) fn text(report: &Value, overview: &FindingOverview, detailed: bool) -> String {
    let mut output = String::new();
    if detailed {
        let _ = writeln!(output, "Next action: {}", report["action_summary"]);
    }
    let incomplete = report["scopes"].as_array().is_some_and(|scopes| {
        scopes
            .iter()
            .any(|scope| scope["completeness"] != "complete")
    });
    let _ = writeln!(
        output,
        "Investigation: {}{}",
        if incomplete {
            "partial"
        } else {
            label(&report["processing"]["status"])
        },
        if detailed || incomplete {
            format!(" (processing {})", label(&report["processing"]["status"]))
        } else {
            String::new()
        }
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
    if detailed {
        let _ = writeln!(
            output,
            "Profile: {}",
            escape_controls(label(&report["report_metadata"]["active_profile"]))
        );
        if let Some(selection) =
            report.pointer("/report_metadata/evidence/query/execution/profile_selection")
        {
            let _ = writeln!(output, "Profile selection: {}", label(&selection["status"]));
        }
    }
    if let Some(inputs) = report["processing"]["inputs"].as_array() {
        for input in inputs {
            let ordinal = input["input_ordinal"].as_u64().unwrap_or(0) as usize;
            let coverage = &report["report_metadata"]["evidence"]["inputs"][ordinal];
            let _ = writeln!(
                output,
                "Input {}: {} — {}{} parsed records ({})",
                ordinal + 1,
                escape_controls(
                    coverage["file"]
                        .as_str()
                        .or_else(|| report
                            .pointer("/report_metadata/evidence/query/execution/source_locations")
                            .and_then(Value::as_array)
                            .and_then(|paths| paths.get(ordinal))
                            .and_then(Value::as_str))
                        .unwrap_or("unavailable source path")
                ),
                if detailed {
                    format!("{} bytes, ", input["consumed_bytes"])
                } else {
                    String::new()
                },
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
    output.push_str(&overview.source_errors);
    output.push_str(&overview.rendered);
    output.push_str(if detailed {
        "\nObserved findings (individual findings page):\n"
    } else {
        "\nWhat happened (times from each starting event):\n"
    });
    let summary_start = output.len();
    if let Some(findings) = report["findings"].as_array() {
        let mut grouped = 0usize;
        let mut highlighted = 0usize;
        for finding in findings {
            if finding["id"]
                .as_str()
                .is_some_and(|id| overview.highlighted.contains(id))
            {
                highlighted += 1;
                continue;
            }
            if overview.groups.iter().any(|(definition, keys)| {
                group_key(finding, definition).is_some_and(|(key, _)| keys.contains(&key))
            }) {
                grouped += 1;
                continue;
            }
            if !detailed {
                if let Some(chain) = finding["details"].get("event_chain") {
                    readable_chain(&mut output, chain);
                    continue;
                }
                if finding["kind"] == "unknown"
                    || finding["id"].as_str().is_some_and(|id| {
                        id.starts_with(&format!("{}-unmatched-", label(&finding["scope_id"])))
                    })
                    || (finding["kind"] == "calculated_fact"
                        && finding["details"]["unit"] == "records")
                {
                    continue;
                }
                let _ = writeln!(
                    output,
                    "- {}: {}",
                    input_label(report, &finding["scope_id"]),
                    escape_controls(label(&finding["claim"]))
                );
            } else {
                let _ = writeln!(
                    output,
                    "- [{}] {}: {}",
                    label(&finding["scope_id"]),
                    label(&finding["kind"]),
                    escape_controls(label(&finding["claim"]))
                );
            }
            if !finding["details"]["value"].is_null() {
                let _ = writeln!(
                    output,
                    "  Value: {} {}",
                    finding["details"]["value"],
                    escape_controls(label(&finding["details"]["unit"]))
                );
            }
            if let Some(chain) = finding["details"].get("event_chain") {
                let _ = writeln!(
                    output,
                    "  {} [{}]: root occurrences {}; ambiguity {}",
                    escape_controls(label(&chain["title"])),
                    finding["id"],
                    chain["root_occurrences"],
                    label(&chain["ambiguity"])
                );
                let _ = writeln!(
                    output,
                    "  Identity / scope: {}",
                    escape_controls(&chain["identity"].to_string())
                );
                for step in chain["steps"].as_array().into_iter().flatten() {
                    let _ = writeln!(
                        output,
                        "    -> {}: {} observed, {} linked; {} (count {}); from {}",
                        escape_controls(label(&step["label"])),
                        step["observed_count"],
                        step["linked_count"],
                        label(&step["status"]).replace('_', " "),
                        label(&step["count_completeness"]),
                        escape_controls(label(&step["from"]))
                    );
                    if let Some(recognition) = step.get("recognition") {
                        let _ = writeln!(
                            output,
                            "       Recognition: {} role matches in this selected input; {} rule matches; failed selectors: {}",
                            recognition["role_matched_records"],
                            recognition["rule_matched_records"],
                            escape_controls(&recognition["failed_selectors"].to_string())
                        );
                    }
                    let _ = writeln!(
                        output,
                        "       Join: {}; ambiguity: {}",
                        escape_controls(&step["join_samples"].to_string()),
                        step["ambiguity"]
                    );
                    for outcome in step["outcomes"].as_array().into_iter().flatten() {
                        let _ = writeln!(
                            output,
                            "       Outcome [{}]: {} = {}",
                            escape_controls(label(&outcome["level"])),
                            escape_controls(label(&outcome["field"])),
                            escape_controls(&outcome["value"].to_string())
                        );
                    }
                    write_evidence(&mut output, step, 4, detailed);
                    if step["omitted_occurrences"].as_u64().unwrap_or(0) > 0 {
                        let _ = writeln!(
                            output,
                            "       {} occurrences omitted from the bundle; retained records are available.",
                            step["omitted_occurrences"]
                        );
                    }
                }
                let _ = writeln!(
                    output,
                    "  Evidence bundle: {} excerpts; {} later same-context samples ({} observed, {} omitted). {}",
                    finding["evidence"].as_array().map_or(0, Vec::len),
                    chain["later_activity"].as_array().map_or(0, Vec::len),
                    chain["later_activity_observed_count"],
                    chain["omitted_later_activity"],
                    label(&chain["evidence_policy"])
                );
                write_evidence(&mut output, finding, usize::MAX, detailed);
            } else {
                write_evidence(&mut output, finding, 2, detailed);
            }
        }
        if detailed && grouped > 0 {
            let _ = writeln!(
                output,
                "{grouped} individual findings on this page are represented in the grouped overview above."
            );
        }
        if detailed && highlighted > 0 {
            let _ = writeln!(
                output,
                "{highlighted} individual findings on this page are represented in the source error overview above."
            );
        }
        if findings.is_empty() {
            output.push_str(
                "No findings displayed; check processing coverage and presentation limits.\n",
            );
        }
    }
    if !detailed && output.len() == summary_start {
        output.push_str("No recognized operations or measured timings on this page.\n");
    }
    if detailed {
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
    } else {
        let mut unknowns: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for assessment in report["assessments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| a["status"] != "supported")
        {
            let goal = match label(&assessment["goal"]) {
                "failures" => "operation failures",
                "slow_operations" => "slow operations",
                "incomplete_lifecycles" => "incomplete operations",
                other => other,
            };
            unknowns
                .entry(input_label(report, &assessment["scope_id"]))
                .or_default()
                .push(goal.replace('_', " "));
        }
        if !unknowns.is_empty() || !overview.unknowns.is_empty() {
            output.push_str("\nUnknown:\n");
            for (input, goals) in unknowns {
                let _ = writeln!(
                    output,
                    "- {input}: cannot assess {} from the available evidence and profile.",
                    goals.join(", ")
                );
            }
            for (input, claim) in &overview.unknowns {
                let _ = writeln!(output, "- {input}: {claim}");
            }
        }
    }
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
            "\nEvidence: {}",
            escape_controls(label(&report["artifact"]["location"]))
        );
        if detailed
            || report["presentation"]["omitted_findings"]
                .as_u64()
                .unwrap_or(0)
                > 0
        {
            let _ = writeln!(
                output,
                "SHA-256: {}",
                label(&report["artifact"]["stored_sha256"])
            );
            let _ = writeln!(
                output,
                "Next: log-analyzer evidence {} --expected-sha256 {} --collection /findings{}",
                shell_quote(label(&report["artifact"]["location"])),
                label(&report["artifact"]["stored_sha256"]),
                report["retrieval"]["next_cursor"]
                    .as_str()
                    .map_or_else(String::new, |cursor| format!(" --report-cursor {cursor}"))
            );
        }
        output.push_str(if detailed {
            "Details: --verbose or --json. Claim verification is optional (evidence --claims).\n"
        } else {
            "Details: --verbose or --json.\n"
        });
    } else {
        output.push_str(
            "Evidence artifact unavailable; retrieval cannot recover omitted findings.\n",
        );
    }
    output
}

fn shell_quote(value: &str) -> String {
    if value.chars().any(char::is_control) {
        let mut quoted = String::from("$'");
        for character in value.chars() {
            if character.is_control() {
                for byte in character.encode_utf8(&mut [0; 4]).bytes() {
                    let _ = write!(quoted, "\\{byte:03o}");
                }
            } else {
                if matches!(character, '\'' | '\\') {
                    quoted.push('\\');
                }
                quoted.push(character);
            }
        }
        quoted.push('\'');
        return quoted;
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}
