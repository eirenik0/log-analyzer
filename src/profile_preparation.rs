//! One-shot candidate creation; domain semantics remain supplied knowledge.
use crate::{
    cli::{Cli, Commands},
    config::{self, OperationKind},
    config_generator, evidence, parser,
    profile_mappings::{StagedReport, destination_identity, same_destination},
    profile_validation,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ATOMIC_STRING_CHARS: usize = 512;
const REPRESENTATIVE_CHARS: usize = 4096;
fn oversized(value: &Value) -> bool {
    fn long_atom(value: &Value) -> bool {
        match value {
            Value::String(text) => text.chars().count() > ATOMIC_STRING_CHARS,
            Value::Array(items) => items.iter().any(long_atom),
            Value::Object(items) => items.values().any(long_atom),
            _ => false,
        }
    }
    long_atom(value) || value.to_string().chars().count() > REPRESENTATIVE_CHARS
}
fn bound(value: &mut Value, limit: usize, path: &str, omissions: &mut Vec<Value>) {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    // These subtrees contain exact facts, configuration or retrieval identities.
    if path == "/profile_preparation/structure"
        || matches!(
            leaf,
            "expected" | "before" | "after" | "scope" | "source" | "evidence_ref"
        )
        || (leaf == "observed"
            && (path.contains("/expected_results/") || path.contains("/missing_information/")))
    {
        return;
    }
    match value {
        Value::Array(items) => {
            let bounded = matches!(
                leaf,
                "records"
                    | "diagnostics"
                    | "operations"
                    | "expected_results"
                    | "suggestions"
                    | "witnesses"
                    | "heuristic_changes"
                    | "missing_information"
                    | "components"
                    | "commands"
                    | "requests"
                    | "fields"
            );
            let total = items.len();
            let mut oversize = 0;
            let mut count_limit = 0;
            let mut retained = Vec::new();
            for mut item in std::mem::take(items) {
                let mut child_omissions = Vec::new();
                bound(
                    &mut item,
                    limit,
                    &format!("{path}/{}", retained.len()),
                    &mut child_omissions,
                );
                if bounded && oversized(&item) {
                    oversize += 1;
                } else if bounded && retained.len() >= limit {
                    count_limit += 1;
                } else {
                    retained.push(item);
                    omissions.extend(child_omissions);
                }
            }
            *items = retained;
            if oversize + count_limit > 0 {
                omissions.push(json!({"path":path,"unit":"items","total":total,"omitted":oversize+count_limit,"oversized_items":oversize,"count_limit_items":count_limit}));
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                let key = key.replace('~', "~0").replace('/', "~1");
                bound(item, limit, &format!("{path}/{key}"), omissions);
            }
        }
        _ => (),
    }
}
fn structural_failure(coverage: &parser::ParseCoverage) -> bool {
    coverage.is_unparsed()
        || coverage.rejected_candidates > 0
        || coverage.structural_diagnostics.diagnostic_count > 0
        || coverage.structural_diagnostics.unsupported_python_headers > 0
}
fn parse(
    files: &[PathBuf],
    cli: &Cli,
    config: &config::AnalyzerConfig,
) -> Result<(
    Vec<Vec<parser::LogEntry>>,
    Vec<parser::ParseCoverage>,
    evidence::Context,
)> {
    let mut context = evidence::Context::new(cli, config)?;
    let mut inputs = Vec::new();
    let mut coverage = Vec::new();
    for (ordinal, path) in files.iter().enumerate() {
        let mut parsed = parser::parse_log_file_report(path, config)
            .map_err(|e| format!("Failed to read preparation input: {e:?}"))?;
        for entry in &mut parsed.entries {
            entry.source_input_ordinal = Some(ordinal);
        }
        crate::output::observe_entries(&parsed.entries);
        context.observe(&parsed.coverage, &parsed.entries);
        coverage.push(parsed.coverage);
        inputs.push(parsed.entries);
    }
    Ok((inputs, coverage, context))
}
fn snapshots(coverage: &[parser::ParseCoverage]) -> Vec<(&str, u64)> {
    coverage
        .iter()
        .map(|c| (c.snapshot_sha256.as_str(), c.input_bytes))
        .collect()
}
fn protect(candidate: &Path, report: Option<&Path>, sources: &[PathBuf]) -> Result<()> {
    let candidate = destination_identity(candidate)?;
    if candidate.exists() {
        return Err("Candidate destination already exists; choose a new file".into());
    }
    let report = report.map(destination_identity).transpose()?;
    if report
        .as_ref()
        .is_some_and(|p| same_destination(p, &candidate, false))
    {
        return Err("Candidate and report destinations must be separate".into());
    }
    for source in sources {
        let source = destination_identity(source)?;
        if same_destination(&candidate, &source, false)
            || report
                .as_ref()
                .is_some_and(|p| same_destination(p, &source, true))
        {
            return Err(
                "Candidate/report destination conflicts with an input or starting profile".into(),
            );
        }
    }
    Ok(())
}
fn missing(
    validation: &Value,
    coverage: &[parser::ParseCoverage],
    config: &config::AnalyzerConfig,
) -> Vec<Value> {
    let mut items = Vec::new();
    let rules = config
        .event_classifier()
        .map(|r| serde_json::to_value(r.schema()).expect("rules serialize"));
    for (ordinal, source) in coverage
        .iter()
        .enumerate()
        .filter(|(_, c)| structural_failure(c))
    {
        items.push(json!({"category":"unsupported_structure","reason":"consumed_input_has_unavailable_or_rejected_structure","fields":[],"witnesses":[{"input_ordinal":ordinal,"structural_diagnostics":source.structural_diagnostics,"normalization_diagnostics":source.normalization_diagnostics}],"next_step":"Supply a supported parser or explicit normalization; an event-rule candidate cannot establish missing grammar","unknown":"Intended structure and capture completeness require application knowledge"}));
    }
    let v = &validation["profile_validation"];
    for diagnostic in v["diagnostics"].as_array().into_iter().flatten() {
        let reason = diagnostic["reason"]
            .as_str()
            .unwrap_or("unavailable_boundaries");
        let category = match reason {
            "scope_adequacy_unknown" | "ambiguous_pairing" => "scope_ambiguity",
            "conflicting_rules" => "conflicting_rules",
            "intentional_start_only" => "missing_boundary",
            _ => "missing_boundary",
        };
        items.push(json!({"category":category,"reason":reason,"fields":[],"witnesses":[diagnostic],"next_step":"Review intended identities, scope, boundaries and capture before editing a separate candidate","unknown":"An absent end is not proof of a hang or failure; start-only events may intentionally lack an end"}));
    }
    for record in v["records"].as_array().into_iter().flatten() {
        let status = record["classification"]["status"].as_str();
        let category = match status {
            Some("invalid") => Some("missing_identity_field"),
            Some("conflict") => Some("conflicting_rules"),
            Some("unclassified") => Some("unrecognized_requested_kind"),
            _ => None,
        };
        if let Some(category) = category {
            let mut fields = Vec::new();
            for diagnostic in record["classification"]["diagnostics"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(target) = diagnostic["target"].as_str() {
                    fields.push(json!(target));
                    for rule in rules
                        .as_ref()
                        .and_then(|r| r["rules"].as_array())
                        .into_iter()
                        .flatten()
                        .filter(|r| r["id"] == diagnostic["rule_id"])
                    {
                        source_fields(&rule["mapping"][target], &mut fields);
                    }
                }
            }
            fields.sort_by_key(Value::to_string);
            fields.dedup();
            items.push(json!({"category":category,"reason":"sample_classification_requires_review","fields":fields,"witnesses":[record],"next_step":"Supply intended event family, name, phase, correlation identity and scope; edit only a separate candidate","unknown":"Observed wording and match counts do not establish event meaning"}));
        }
    }
    for check in v["expected_results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["status"] == "failed")
    {
        items.push(json!({"category":"assertion_mismatch","reason":"independent_expected_fact_failed","fields":[],"witnesses":[check],"next_step":"Review the candidate against independently established facts; do not regenerate truth from its output","unknown":"The candidate may be unsuitable; no retry loop manufactures support"}));
    }
    if v["requested_coverage"]["classification"]["classified_records"] == 0 && items.is_empty() {
        items.push(json!({"category":"unrecognized_requested_kind","reason":"requested_lifecycle_not_recognized","fields":["kind","name","phase","correlation_id","scope"],"witnesses":[],"next_step":"Supply independently known lifecycle semantics before editing rules","unknown":"Application meaning cannot be inferred from logs alone"}));
    }
    items
}

fn source_fields(value: &Value, fields: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            for item in items {
                source_fields(item, fields);
            }
        }
        Value::Object(item) => {
            if let Some(field) = item.get("field").and_then(Value::as_str) {
                fields.push(json!(field));
            }
            if let Some(alternatives) = item.get("fields").and_then(Value::as_array) {
                fields.extend(alternatives.iter().cloned());
            }
        }
        _ => (),
    }
}

pub(crate) fn run(cli: &Cli) -> Result<()> {
    let Commands::PrepareProfile(args) = &cli.command else {
        unreachable!()
    };
    let crate::cli::PrepareProfileArgs {
        files,
        candidate_output,
        kind,
        purpose,
        expected,
        template,
        profile_name,
        witness_limit,
    } = args;
    let (base, profile_sources) = match template {
        Some(path) if path.exists() => config::load_config_from_path_with_sources(path)?,
        Some(path) => (
            config::load_builtin_template(path.to_str().ok_or("Template name must be UTF-8")?)
                .ok_or("Starting template not found")?,
            Vec::new(),
        ),
        None if cli.config.is_some() => {
            config::load_config_from_path_with_sources(cli.config.as_deref().unwrap())?
        }
        None => (
            config::load_config(None, cli.preset.as_deref())?,
            Vec::new(),
        ),
    };
    let mut protected = files.clone();
    protected.extend(profile_sources);
    protected.extend(expected.iter().cloned());
    protect(candidate_output, cli.output.as_deref(), &protected)?;
    let staged_report = cli
        .output
        .as_deref()
        .map(StagedReport::prepare)
        .transpose()?;
    let filter = crate::build_filter(&cli.filter)?;
    let (facts, facts_digest) = profile_validation::load_expectations(expected.as_deref())?;
    let (observed, initial_coverage, _) = parse(files, cli, &base)?;
    let logs: Vec<_> = observed.iter().flatten().cloned().collect();
    let mut generated = config_generator::generate_config(
        &logs,
        &base,
        &config_generator::GenerateConfigOptions {
            profile_name: profile_name
                .clone()
                .unwrap_or_else(|| "prepared-profile".into()),
        },
    );
    if let Some(first) = initial_coverage.first().map(|c| c.selected_parser)
        && initial_coverage
            .iter()
            .all(|c| c.selected_parser == first && !structural_failure(c))
    {
        generated.parser.format = first;
    }
    let text = toml::to_string_pretty(&generated)?;
    let empty = initial_coverage.iter().all(|c| c.nonempty_lines == 0);
    let wholly_unsupported = !initial_coverage.iter().any(|c| c.parsed_entries > 0);
    let mut candidate_file = if wholly_unsupported {
        None
    } else {
        let parent = candidate_output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        Some(file)
    };
    let candidate_config = if let Some(file) = &candidate_file {
        config::load_config_from_path(file.path())?
    } else {
        generated
    };
    let (inputs, coverage, context) = parse(files, cli, &candidate_config)?;
    if snapshots(&initial_coverage) != snapshots(&coverage) {
        return Err("Input changed during preparation; no candidate was committed".into());
    }
    let operation_kind = match kind {
        crate::cli::OperationType::Request => OperationKind::Request,
        crate::cli::OperationType::Command => OperationKind::Command,
        crate::cli::OperationType::Event => OperationKind::Event,
    };
    let mut validation = profile_validation::analyze(
        &inputs,
        &candidate_config,
        &filter,
        operation_kind,
        *purpose,
        facts.as_ref(),
        facts_digest,
    );
    if coverage.iter().any(structural_failure) {
        validation["profile_validation"]["suitability"] = json!({"status":"insufficient_evidence","reason":"input_structure_unavailable","basis":"observed_sample_only","semantic_correctness":"not_established_by_match_count"});
    }
    let proof = profile_validation::selection_evidence(
        &inputs,
        &filter,
        operation_kind,
        *purpose,
        facts.as_ref(),
        &validation["profile_validation"],
    );
    let missing_information = missing(&validation, &coverage, &candidate_config);
    validation["profile_validation"]
        .as_object_mut()
        .unwrap()
        .remove("effective_rules");
    let base_value = serde_json::to_value(&base)?;
    let candidate_value = serde_json::to_value(&candidate_config)?;
    let observed_witnesses:Vec<_> = logs.iter().map(|entry|json!({"source":evidence::source(entry),"component":entry.component,"component_id":entry.component_id,"observed_command":match &entry.kind {parser::LogEntryKind::Command {command,..}=>Some(command),_=>None},"observed_request":match &entry.kind {parser::LogEntryKind::Request {request,..}=>Some(request),_=>None}})).collect();
    let heuristics:Vec<_> = ["parser","sessions"].into_iter().filter(|key|base_value[*key]!=candidate_value[*key]).map(|key|json!({"section":key,"basis":"heuristic_observation","verified":false,"before":base_value[key],"after":candidate_value[key],"witnesses":observed_witnesses,"unknown":"Review inferred parser/module mapping or session prefixes against application knowledge"})).collect();
    let follow_up = if empty {
        "Supply a nonempty sample and independently known domain facts before preparing a candidate"
    } else if wholly_unsupported {
        "Retrieve input structure with info diagnostics; supply a supported parser or explicit normalization before preparing a candidate"
    } else {
        "Retrieve omitted sample evidence with profile validate and common report cursors using the saved candidate; for unparsed input use info diagnostics"
    };
    let mut report = json!({"profile_preparation":{"version":1,"requested":{"kind":operation_kind,"purpose":purpose},"candidate":if wholly_unsupported {Value::Null} else {json!({"path":evidence::path_label(candidate_output),"sha256":evidence::profile_digest(&candidate_config)?,"status":"saved","activation":false})},"creation":{"status":if empty {"not_created_empty_input"} else if wholly_unsupported {"not_created_unsupported_structure"} else {"saved"}},"structure":{"status":if coverage.iter().any(structural_failure){"unsupported"}else if logs.is_empty(){"unverified_empty"}else{"observed_compatible"},"files":coverage},"provenance":{"inherited":{"profile":base.profile_name,"sha256":evidence::profile_digest(&base)?,"lifecycle_rules":"preserved_from_supplied_starting_point_not_verified_by_matching"},"observed":{"components":candidate_config.profile.known_components,"commands":candidate_config.profile.known_commands,"requests":candidate_config.profile.known_requests,"witnesses":observed_witnesses,"basis":"parsed_sample_inventory_not_semantic_truth"},"heuristic_changes":heuristics},"sample_validation":validation["profile_validation"],"semantic_proof":proof,"missing_information":missing_information,"presentation":{"witness_limit":witness_limit,"atomic_string_unicode_scalars":ATOMIC_STRING_CHARS,"representative_unicode_scalars":REPRESENTATIVE_CHARS,"metadata_budget_exception":"Outcome metadata, coverage totals and opaque retrieval identities remain exact","follow_up":follow_up,"omissions":[]},"report_save":{"status":if staged_report.is_some(){"succeeded"}else{"not_requested"}},"limitations":["Candidate creation is distinct from validation support and never activates a profile","Observed sample support does not establish capture completeness or intended event meaning","No automatic semantic repair or retry loop is performed","Representative count and atomic size limits bound presentation, not processing memory or total metadata"]}});
    context.annotate(&mut report);
    let mut omissions = Vec::new();
    for field in [
        "structure",
        "provenance",
        "sample_validation",
        "semantic_proof",
        "missing_information",
    ] {
        bound(
            &mut report["profile_preparation"][field],
            *witness_limit as usize,
            &format!("/profile_preparation/{field}"),
            &mut omissions,
        );
    }
    report["profile_preparation"]["presentation"]["omissions"] = json!(omissions);
    // All parsing, validation and report construction precedes file creation.
    if let Some(file) = candidate_file.take() {
        file.persist_noclobber(candidate_output)
            .map_err(|e| e.error)?;
    }
    crate::output::set_metadata(
        crate::build_info::metadata(&candidate_config.profile_name),
        false,
    );
    crate::output::set_evidence(context);
    let report = deliver(report, !wholly_unsupported, staged_report)?;
    crate::output::print(format_args!("{}\n", serde_json::to_string_pretty(&report)?));
    Ok(())
}

fn deliver(mut report: Value, created: bool, staged_report: Option<StagedReport>) -> Result<Value> {
    if let Some(staged) = staged_report
        && let Err(error) = staged.save(&report)
    {
        if !created {
            return Err(error);
        }
        report["profile_preparation"]["report_save"]["status"] = json!("failed");
        eprintln!(
            "Warning: candidate created, but report saving failed. Do not retry creation; run profile validate on the saved candidate with a separate output path."
        );
    }
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postcreation_save_failure_preserves_candidate_and_discloses_delivery_failure() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate.toml");
        let destination = dir.path().join("report.json");
        let staged = StagedReport::prepare(&destination).unwrap();
        std::fs::write(&candidate, "profile_name = 'generic'\n").unwrap();
        std::fs::create_dir(&destination).unwrap();
        let report = deliver(json!({"profile_preparation":{"creation":{"status":"saved"},"report_save":{"status":"succeeded"}}}),true,Some(staged)).unwrap();
        assert_eq!(report["profile_preparation"]["creation"]["status"], "saved");
        assert_eq!(
            report["profile_preparation"]["report_save"]["status"],
            "failed"
        );
        assert!(candidate.exists());
    }
    #[test]
    fn oversized_witnesses_are_omitted_without_modifying_atomic_facts_or_locations() {
        let source = json!({"row_path":"/🧪λ".repeat(600),"evidence_ref":{"reference_id":"fixed","row_path":"/🧪λ".repeat(600)}});
        let mut value = json!({"records":[{"source":source,"classification":{"scope":["one","two"]}},{"source":{"row_path":"/rows/0"},"classification":{"scope":["one","two"]}}]});
        let original = value.clone();
        let mut omissions = Vec::new();
        bound(&mut value, 1, "/sample", &mut omissions);
        assert_eq!(value["records"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["records"][0]["classification"]["scope"],
            json!(["one", "two"])
        );
        assert_eq!(value["records"][0]["source"]["row_path"], "/rows/0");
        assert_eq!(omissions[0]["oversized_items"], 1);
        assert_eq!(
            original["records"][0]["source"]["evidence_ref"]["reference_id"],
            "fixed"
        );
        assert_eq!(
            original["records"][0]["source"]["row_path"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            1800
        );
    }
    #[test]
    fn nested_omissions_follow_retained_indices_and_exclude_dropped_parents() {
        let mut value = json!({"records":[{"id":"x".repeat(600),"witnesses":[1,2]},{"id":"short","witnesses":[3,4]},{"id":"last","witnesses":[5,6]}]});
        let mut omissions = Vec::new();
        bound(&mut value, 1, "/sample", &mut omissions);
        assert_eq!(value["records"][0]["id"], "short");
        assert_eq!(omissions.len(), 2);
        assert_eq!(omissions[0]["path"], "/sample/records/0/witnesses");
        assert_eq!(omissions[1]["oversized_items"], 1);
        assert_eq!(omissions[1]["count_limit_items"], 1);
    }
    #[test]
    fn atomic_values_and_coverage_are_preserved() {
        let atom = json!({"fields":[1,2],"commands":[1,2],"records":[1,2]});
        let mut value = json!({"expected_results":[{"expected":atom,"observed":atom}],"heuristic_changes":[{"before":atom,"after":atom}]});
        let original = value.clone();
        let mut omissions = Vec::new();
        bound(
            &mut value,
            1,
            "/profile_preparation/sample_validation",
            &mut omissions,
        );
        assert_eq!(value, original);
        assert!(omissions.is_empty());
        let mut coverage = json!({"files":[{"structural_diagnostics":{"diagnostic_count":2,"omitted_diagnostics":0,"diagnostics":[1,2]}},{"input_ordinal":1}]});
        let original = coverage.clone();
        bound(
            &mut coverage,
            1,
            "/profile_preparation/structure",
            &mut omissions,
        );
        assert_eq!(coverage, original);
        assert!(omissions.is_empty());
    }
}
