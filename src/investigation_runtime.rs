//! Runtime orchestration; the public contract validator remains independent.
mod artifact;
mod findings;
mod policy;
mod selection;
use crate::{
    cli::{Cli, Commands, InvestigateArgs},
    config, evidence, parser,
    processing::{Budget, Limits},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selector {
    input_ordinal: Option<usize>,
    kind: Option<crate::config::OperationKind>,
    name: Option<String>,
    correlation_id: Option<String>,
    scope: Option<Vec<String>>,
}
impl Selector {
    pub fn matches(&self, entry: &parser::LogEntry, config: &config::AnalyzerConfig) -> bool {
        if self
            .input_ordinal
            .is_some_and(|ordinal| entry.source_input_ordinal != Some(ordinal))
        {
            return false;
        }
        let crate::event_rules::ClassifiedRecord::Event { semantics, .. } = entry
            .classification
            .as_ref()
            .unwrap_or(&crate::event_rules::ClassifiedRecord::Unclassified)
        else {
            return self.kind.is_none()
                && self.name.is_none()
                && self.correlation_id.is_none()
                && self.scope.is_none();
        };
        self.kind.is_none_or(|kind| semantics.kind == kind)
            && self
                .name
                .as_ref()
                .is_none_or(|name| semantics.name == *name)
            && self
                .correlation_id
                .as_ref()
                .is_none_or(|id| semantics.correlation_id.as_ref() == Some(id))
            && self.scope.as_ref().is_none_or(|scope| {
                crate::perf_analyzer::correlation_scope(entry, config).as_ref() == Some(scope)
            })
    }
}
pub(super) fn selected(
    entry: &parser::LogEntry,
    selectors: &[Selector],
    config: &config::AnalyzerConfig,
) -> bool {
    selectors.is_empty()
        || selectors
            .iter()
            .any(|selector| selector.matches(entry, config))
}

struct Capture {
    data: Vec<u8>,
    complete: bool,
}
fn capture(path: &Path, budget: &mut Budget) -> Result<Capture> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if !budget.checkpoint("capture", 1) {
            break;
        }
        let remaining = budget.limits.input_bytes.saturating_sub(budget.input_bytes);
        if remaining == 0 {
            budget.stop("capture", "input_limit", Some("input_bytes"));
            break;
        }
        let capacity = buffer.len().min(remaining.min(usize::MAX as u64) as usize);
        // Reservation covers captured storage, JSON escaping and artifact/report copies.
        if !budget.reserve("capture", (capacity as u64) * 16) {
            break;
        }
        let count = match file.read(&mut buffer[..capacity]) {
            Ok(count) => count,
            Err(_) => {
                budget.stop("capture", "io_error", None);
                break;
            }
        };
        if count == 0 {
            return Ok(Capture {
                data,
                complete: true,
            });
        }
        budget.input_bytes += count as u64;
        data.extend_from_slice(&buffer[..count]);
    }
    Ok(Capture {
        data,
        complete: false,
    })
}

pub(crate) fn run(cli: &Cli) -> Result<()> {
    let result = match &cli.command {
        Commands::Investigate(args) => investigate(cli, args),
        Commands::InvestigationEvidence(args) => artifact::retrieve(cli, args),
        _ => unreachable!(),
    };
    if cli.redact {
        result.map_err(|_|"Investigation failed before delivery; inspect local inputs, profile and destinations".into())
    } else {
        result
    }
}

fn investigate(cli: &Cli, args: &InvestigateArgs) -> Result<()> {
    if cli.report_cursor.is_some() {
        return Err("Use investigation-evidence to resume a retained artifact without processing sources again".into());
    }
    if args.files.len() > 32 || args.select.len() > 32 {
        return Err("Investigation accepts at most 32 inputs and 32 exact selectors".into());
    }
    let selectors: Vec<Selector> = args
        .select
        .iter()
        .map(|text| {
            if text.len() > 65536 {
                return Err("Exact selector exceeds 65536 bytes".into());
            }
            let selector: Selector = serde_json::from_str(text)?;
            if selector.input_ordinal.is_none()
                && selector.kind.is_none()
                && selector.name.is_none()
                && selector.correlation_id.is_none()
                && selector.scope.is_none()
            {
                return Err("An exact selector must declare at least one field".into());
            }
            Ok(selector)
        })
        .collect::<Result<_>>()?;
    let (config, profile_sources) = if let Some(path) = &cli.config {
        config::load_config_from_path_with_sources(path)?
    } else {
        (
            config::load_config(None, cli.preset.as_deref())?,
            Vec::new(),
        )
    };
    let mut protected = args.files.clone();
    protected.extend(profile_sources);
    protected.extend(args.cancel_file.iter().cloned());
    artifact::protect(&args.artifact, cli.output.as_deref(), &protected)?;
    let mut temporary = artifact::stage(&args.artifact)?;
    let staged_report = cli
        .output
        .as_deref()
        .map(crate::profile_mappings::StagedReport::prepare)
        .transpose()?;
    let filter = crate::build_filter(&cli.filter)?;
    let configuration_bytes = serde_json::to_vec(&config)?.len() as u64;
    if configuration_bytes > 4 * 1024 * 1024 {
        return Err("Effective profile exceeds the 4 MiB investigation configuration limit".into());
    }
    let capture_root = std::env::current_dir()?;
    let source_locations: Vec<_> = args
        .files
        .iter()
        .map(|path| {
            evidence::path_value(&if path.is_absolute() {
                path.clone()
            } else {
                capture_root.join(path)
            })
        })
        .collect();
    let mut context = evidence::Context::new(cli, &config)?;
    let mut budget = Budget::new(Limits {
        input_bytes: args.input_max_bytes,
        records: args.processing_max_records,
        expanded_records: args.processing_max_expanded_records,
        work_units: args.processing_max_work,
        elapsed_ms: args.processing_max_ms,
        memory_bytes: args.processing_max_memory_bytes,
        artifact_bytes: args.artifact_max_bytes,
        record_bytes: args.record_max_bytes,
        cancel_file: args.cancel_file.clone(),
    });
    budget.configuration_bytes = configuration_bytes;
    let query_bytes = serde_json::to_vec(cli)?.len() as u64;
    budget.reserve(
        "capture",
        configuration_bytes
            .saturating_add(query_bytes)
            .saturating_mul(16),
    );
    let mut progress = Vec::new();
    let mut captures = Vec::new();
    let mut parsed_inputs = Vec::new();
    let mut parse_calls = 0usize;
    for (ordinal, path) in args.files.iter().enumerate() {
        budget.active_scope = ordinal;
        if budget.stop.is_some() {
            budget.affected_scopes.insert(ordinal);
            progress.push(json!({"input_ordinal":ordinal,"capture":"unread","consumed_bytes":0,"consumed_sha256":null,"remaining_bytes":null}));
            captures.push(json!({"input_ordinal":ordinal,"capture":"unread","encoding":"utf8","data":null,"data_omitted":true,"original_consumed_sha256":null,"stored_sha256":null}));
            continue;
        }
        let capture = match capture(path, &mut budget) {
            Ok(capture) => capture,
            Err(_) => {
                budget.stop("capture", "io_error", None);
                progress.push(json!({"input_ordinal":ordinal,"capture":"unread","consumed_bytes":0,"consumed_sha256":null,"remaining_bytes":null}));
                captures.push(json!({"input_ordinal":ordinal,"capture":"unread","encoding":"utf8","data":null,"data_omitted":true,"original_consumed_sha256":null,"stored_sha256":null}));
                continue;
            }
        };
        let hash = evidence::digest(&capture.data);
        progress.push(json!({"input_ordinal":ordinal,"capture":if capture.complete {"complete"} else {"prefix"},"consumed_bytes":capture.data.len(),"consumed_sha256":hash,"remaining_bytes":if capture.complete {Some(0)} else {None}}));
        budget.begin_stage();
        parse_calls += 1;
        let mut parsed = match parser::parse_capture(
            path,
            &capture.data,
            capture.complete,
            &config,
            &mut budget,
        ) {
            Ok(parsed) => parsed,
            Err(_) => {
                budget.stop("parse", "unsupported", None);
                let mut parsed = parser::parse_capture(path, b"", false, &config, &mut budget)
                    .map_err(|e| format!("{e:?}"))?;
                parsed.coverage.nonempty_lines = usize::from(!capture.data.is_empty());
                parsed.coverage.input_bytes = capture.data.len() as u64;
                parsed.coverage.snapshot_sha256 = hash.clone();
                parsed
            }
        };
        for entry in &mut parsed.entries {
            entry.source_input_ordinal = Some(ordinal);
        }
        context.observe_selected(&parsed.coverage, &parsed.entries, |entry| {
            selected(entry, &selectors, &config)
        });
        let (encoding, data) = match String::from_utf8(capture.data) {
            Ok(text) => ("utf8", json!(text)),
            Err(error) => {
                let bytes = error.into_bytes();
                if budget.reserve("artifact_write", (bytes.len() as u64).saturating_mul(64)) {
                    ("bytes", json!(bytes))
                } else {
                    ("omitted_bytes", serde_json::Value::Null)
                }
            }
        };
        captures.push(json!({"input_ordinal":ordinal,"capture":if capture.complete {"complete"} else {"prefix"},"encoding":if encoding=="omitted_bytes"{"bytes"}else{encoding},"data":data,"data_omitted":encoding=="omitted_bytes","original_consumed_sha256":hash,"stored_sha256":if encoding=="omitted_bytes"{None}else{Some(hash.clone())}}));
        parsed_inputs.push(parsed);
    }
    let mut metadata = crate::build_info::metadata(&config.profile_name);
    metadata["evidence"] = context.metadata(&json!({}), cli.redact, &cli.mask_id);
    let snapshot = metadata["evidence"]["snapshot_id"].clone();
    let profile_digest = metadata["evidence"]["profile_sha256"].clone();
    let mut records = Vec::new();
    let mut scopes = Vec::new();
    let mut findings = Vec::new();
    let mut populations = Vec::new();
    let mut memberships = Vec::new();
    let mut sequences = Vec::new();
    let mut assessments = Vec::new();
    let mut correlation_calls = 0usize;
    for (ordinal, parsed) in parsed_inputs.iter().enumerate() {
        budget.active_scope = ordinal;
        budget.begin_stage();
        let mut entries = Vec::new();
        for entry in parsed.entries.iter().filter(|entry| filter.matches(entry)) {
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            if selected(entry, &selectors, &config) {
                records.push(findings::record(entry, &context, &snapshot));
            }
            entries.push(entry.clone());
        }
        budget.begin_stage();
        correlation_calls += 1;
        let perf = crate::perf_analyzer::analyze_performance_checked(
            &entries,
            &crate::comparator::LogFilter::new(),
            None,
            &config,
            |work| budget.checkpoint("correlation", work),
        );
        budget.begin_stage();
        let scope_aliases = crate::profile_validation::scope_adequacy(&entries, &config, |work| {
            budget.checkpoint("calculation", work)
        });
        budget.begin_stage();
        let view = selection::View::new(&entries, &selectors, &config, perf.as_ref(), &mut budget);
        let scope_aliases = scope_aliases.map(|aliases| {
            aliases
                .into_iter()
                .filter(|alias| view.intersects_alias(alias))
                .collect::<Vec<_>>()
        });
        let scope_id = format!("scope-{ordinal}");
        let semantic_status = if config.event_classifier().is_none() {
            "unsupported"
        } else if perf.is_some() {
            if view.conflicting > 0 || view.ambiguous > 0 {
                "conflicting"
            } else if view.relevant == 0 || view.rejected > 0 {
                "insufficient_evidence"
            } else {
                "supported"
            }
        } else {
            "not_performed"
        };
        let coverage = perf.as_ref().map(|_| &view);
        scopes.push(json!({"id":scope_id,"input_ordinals":[ordinal],"selection":"Legacy global filters apply before correlation; exact selectors apply to retained occurrences after shared correlation. Each input is an independent run.","extent":"declared_input","completeness":"complete","analysis_completion":if perf.is_some(){"complete"}else{"not_performed"},"correlation_scope":[],"semantic_coverage":{"status":semantic_status,"relevant_records":coverage.map(|c|c.relevant),"classified_records":coverage.map(|c|c.classified),"paired_events":coverage.map(|c|c.pairs * 2),"unmatched_events":coverage.map(|c|c.unmatched),"ambiguous_events":coverage.map(|c|c.ambiguous),"rejected_events":coverage.map(|c|c.rejected),"reason":"Explicit effective profile validated structurally and assessed against the observed sample; no automatic semantic proof or upstream completeness claim."},"upstream_completeness":"unknown"}));
        budget.begin_stage();
        findings::build(
            &entries,
            if config.event_classifier().is_some() {
                perf.as_ref()
            } else {
                None
            },
            &view,
            &scope_id,
            &context,
            &snapshot,
            &profile_digest,
            args.threshold_ms,
            &mut budget,
            &mut findings,
            &mut populations,
            &mut memberships,
            &mut sequences,
        );
        budget.begin_stage();
        if let Some(policy) = &config.investigation {
            policy::calculate(
                policy,
                &config,
                &entries,
                &selectors,
                &scope_id,
                &context,
                &snapshot,
                &mut budget,
                &mut findings,
                &mut populations,
                &mut memberships,
            );
        }
        if let Some(aliases) = &scope_aliases {
            findings::scope_aliases(
                aliases,
                &entries,
                &scope_id,
                &context,
                &snapshot,
                &mut budget,
                &mut findings,
            );
        }
        let timing_status = if semantic_status == "not_performed" {
            "insufficient_evidence"
        } else if semantic_status != "supported" {
            semantic_status
        } else if view.pairs == 0
            || view.unmatched > 0
            || view.excluded_boundaries > 0
            || !view.reliable
            || scope_aliases
                .as_ref()
                .is_none_or(|aliases| !aliases.is_empty())
        {
            "insufficient_evidence"
        } else {
            "supported"
        };
        for goal in [
            "inspection",
            "failures",
            "slow_operations",
            "incomplete_lifecycles",
        ] {
            assessments.push(json!({"goal":goal,"scope_id":scope_id,"status":if goal=="inspection"{if parsed.coverage.is_unparsed(){"insufficient_evidence"}else{"supported"}}else if goal=="slow_operations" { timing_status } else if goal=="failures" && !view.entries.is_empty() && !view.outcomes_supported{"unsupported"}else if semantic_status=="not_performed"{"insufficient_evidence"}else{semantic_status},"reason":if goal!="inspection" && view.entries.is_empty(){"Exact selection contains zero processed records; lifecycle support cannot be established from unrelated input."}else if goal=="inspection" {"Counts and source evidence describe only the selected processed population."}else{"Only explicit profile semantics and observed evidence support this assessment; domain attempts/resources/causal relationships require declared rules."},"finding_ids":findings.iter().filter(|finding|finding["scope_id"]==scope_id).map(|finding|finding["id"].clone()).collect::<Vec<_>>()}));
        }
    }
    // Unread independent inputs retain their own unavailable analysis scopes.
    for ordinal in parsed_inputs.len()..args.files.len() {
        let scope_id = format!("scope-{ordinal}");
        budget.affected_scopes.insert(ordinal);
        scopes.push(json!({"id":scope_id,"input_ordinals":[ordinal],"selection":"Source snapshot could not be processed","extent":"processed_population","completeness":"unavailable","analysis_completion":"not_performed","correlation_scope":[],"semantic_coverage":{"status":"not_performed","relevant_records":null,"classified_records":null,"paired_events":null,"unmatched_events":null,"ambiguous_events":null,"rejected_events":null,"reason":"Capture unavailable"},"upstream_completeness":"unknown"}));
        for goal in [
            "inspection",
            "failures",
            "slow_operations",
            "incomplete_lifecycles",
        ] {
            assessments.push(json!({"goal":goal,"scope_id":scope_id,"status":"insufficient_evidence","reason":"The declared input could not be processed before capture stopped.","finding_ids":[]}));
        }
    }
    if let Some(stop) = &mut budget.stop {
        let affected: std::collections::BTreeSet<_> = budget
            .affected_scopes
            .iter()
            .map(|ordinal| format!("scope-{ordinal}"))
            .collect();
        stop["scope_ids"] = json!(affected);
        for scope in &mut scopes {
            if affected.contains(scope["id"].as_str().unwrap()) {
                if scope["completeness"] == "complete" {
                    scope["completeness"] = json!("partial");
                }
                if scope["analysis_completion"] == "complete" {
                    scope["analysis_completion"] = json!("partial");
                }
            }
        }
        for assessment in &mut assessments {
            if affected.contains(assessment["scope_id"].as_str().unwrap())
                && assessment["status"] == "supported"
            {
                assessment["status"] = json!("insufficient_evidence");
                assessment["reason"] = json!(
                    "Processing stopped before a complete assessment of this input; retained calculations describe only its processed population."
                );
            }
        }
        for population in &mut populations {
            if affected.contains(population["scope_id"].as_str().unwrap()) {
                population["completeness"] = json!("partial");
            }
        }
        for sequence in &mut sequences {
            if affected.contains(sequence["scope_id"].as_str().unwrap()) {
                sequence["completeness"] = json!("partial");
            }
        }
    }
    // Instrumentation belongs to the effective query and its digest, never to evidence text.
    metadata["evidence"]["query"]["execution"] = json!({"parse_passes":parse_calls,"correlation_passes":correlation_calls,"input_relationship":"independent_runs","record_max_bytes":args.record_max_bytes,"profile_selection":"explicit_or_generic_base","source_locations":source_locations});
    metadata["evidence"]["query_sha256"] = json!(evidence::digest(
        metadata["evidence"]["query"].to_string().as_bytes()
    ));
    let mut usage = budget.usage_json();
    usage["records"] = json!(parsed_inputs.iter().map(|p| p.entries.len()).sum::<usize>());
    let processing = json!({"status":if budget.stop.is_none(){"complete"}else{"partial"},"stop":budget.stop,"limits":budget.limits_json(),"usage":usage,"inputs":progress});
    let retention = json!({"policy":"until_deleted","expires_at":null});
    let mut retained = json!({"contract_version":1,"investigation_contract_version":1,"report_metadata":metadata,"processing":processing,"scopes":scopes,"assessments":assessments,"populations":populations,"findings":findings,"records":records,"captured_inputs":captures,"memberships":memberships,"sequences":sequences,"content":"original","effective_profile":serde_json::to_value(&config)?,"effective_profile_omitted":false,"retention":retention,"verification":findings::verification(false, "available")});
    if cli.redact {
        artifact::redact(&mut retained);
    }
    let captures_unverifiable_inputs: std::collections::BTreeSet<_> = retained["captured_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|capture| capture["encoding"] == "bytes")
        .filter_map(|capture| capture["input_ordinal"].as_u64())
        .collect();
    if retained["captured_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|capture| capture["encoding"] == "bytes")
    {
        if retained["captured_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capture| capture["data_omitted"] == true && capture["capture"] != "unread")
        {
            retained["content"] = json!("redacted");
        }
        artifact::source_verification_unavailable(
            &mut retained,
            &captures_unverifiable_inputs,
            "A captured byte stream is not complete UTF-8; source text verification is unavailable.",
        );
    }
    let bytes = artifact::bounded_serialization(&retained, budget.limits.artifact_bytes)?;
    let mut report = artifact::report(&retained, &args.artifact, bytes.as_deref());
    if let Some(bytes) = bytes {
        // Validate calculations/cross-references against the exact bytes before publishing.
        crate::investigation::validate_relations(&report, Some(&bytes))?;
        let saved = (|| -> Result<()> {
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            temporary.persist_noclobber(&args.artifact)?;
            Ok(())
        })();
        if saved.is_err() {
            artifact::unavailable(
                &mut report,
                "Artifact write failed; no reusable evidence artifact was published",
            );
        }
    } else {
        artifact::unavailable(&mut report, "artifact byte limit exceeded");
    }
    artifact::present(&mut report, cli, 0)?;
    if report["artifact"]["status"] == "unavailable" {
        artifact::reconcile_unavailable(&mut report, cli)?;
    }
    crate::investigation::validate_relations(&report, None)?;
    if let Some(staged) = staged_report
        && let Err(error) = staged.save_compact(&report)
    {
        if cli.redact {
            eprintln!("Evidence artifact outcome is reported below; saving the report failed");
        } else {
            eprintln!(
                "Evidence artifact outcome is reported below; saving the report failed: {error}"
            );
        }
    }
    let rendered = serde_json::to_string(&report)?;
    std::io::stdout()
        .lock()
        .write_all(format!("{rendered}\n").as_bytes())?;
    Ok(())
}
