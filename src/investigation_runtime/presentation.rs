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
const MAX_OVERVIEW_GROUPS: usize = 20;
use crate::investigation_view::{Aggregate, FindingGroup, InvestigationView, Measurement};

pub(super) struct FindingOverview {
    source_errors: String,
    highlighted: BTreeSet<String>,
    rendered: String,
    groups: Vec<(FindingGroup, BTreeSet<String>)>,
}
/// Native source errors must remain visible when other findings fill the page.
fn source_errors(report: &Value) -> (String, BTreeSet<String>) {
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
        let _ = write!(output, "- [{scope_id}]");
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
            write_evidence(&mut output, finding, 1);
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
    if !output.is_empty() {
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
        Value::String(value) => value.replace(['\n', '\r'], " "),
        value => value.to_string(),
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
            return rendered;
        };
        rendered.push_str(&dimension_text(value.pointer(path).unwrap_or(&Value::Null)));
        rest = suffix;
    }
    rendered.push_str(rest);
    rendered
}
/// Profile-defined views of all processed findings; individual pages stay unchanged.
pub(super) fn finding_overview(
    report: &Value,
    view: Option<&InvestigationView>,
    redacted: bool,
) -> FindingOverview {
    let (source_errors, highlighted) = source_errors(report);
    let mut overview = FindingOverview {
        source_errors,
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
        let _ = writeln!(overview.rendered, "\n{title} (processed findings):");
        for group in &groups {
            let _ = write!(
                overview.rendered,
                "- [{}] ",
                label(&group.representative["scope_id"])
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
                    dimension.label.clone()
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
                    metric.label.clone()
                };
                if let Some(value) = measured.result(metric.aggregate) {
                    let _ = writeln!(
                        overview.rendered,
                        "  {name}: {value:.2} {} ({} measured, {} unavailable)",
                        if redacted {
                            "configured unit"
                        } else {
                            &metric.unit
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
                .take(3)
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
    if !overview.rendered.is_empty() {
        overview.rendered.push_str("Groups may overlap. Counts are findings, not unique resources, records or failures. Measured summaries use only matching reported values; missing data stays unknown. Individual findings and citations remain in retained evidence.\n");
    }
    overview
}
fn write_evidence(output: &mut String, finding: &Value, max: usize) {
    if let Some(excerpts) = finding["evidence"].as_array() {
        for excerpt in excerpts.iter().take(max) {
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
pub(super) fn text(report: &Value, overview: &FindingOverview) -> String {
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
    output.push_str(&overview.source_errors);
    output.push_str(&overview.rendered);
    output.push_str("\nObserved findings (individual findings page):\n");
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
            write_evidence(&mut output, finding, 2);
        }
        if grouped > 0 {
            let _ = writeln!(
                output,
                "{grouped} individual findings on this page are represented in the grouped overview above."
            );
        }
        if highlighted > 0 {
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
            "Next: log-analyzer evidence {} --expected-sha256 {} --collection /findings{}",
            shell_quote(label(&report["artifact"]["location"])),
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
