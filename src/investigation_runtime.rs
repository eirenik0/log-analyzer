//! Runtime orchestration; the public contract validator remains independent.
mod artifact;
mod auto_profile;
pub(crate) mod chains;
mod claims;
mod construction;
mod finding_store;
mod findings;
mod grouped;
mod guidance;
mod indexed_artifact;
pub(crate) mod policy;
mod presentation;
mod report_checks;
mod report_stream;
mod resource_observations;
mod scope_state;
mod selection;
mod source_store;

use crate::{
    cli::{Cli, Commands, InvestigateArgs},
    config, evidence, parser,
    processing::{Budget, Limits},
    streaming::Capture,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{Seek, SeekFrom},
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

fn capture(path: &Path, budget: &mut Budget) -> Result<Capture> {
    Ok(Capture::read(path, budget)?)
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

use source_store::{Correlated, Entries};
fn correlate(
    entries: &Entries,
    config: &config::AnalyzerConfig,
    args: &InvestigateArgs,
    budget: &mut Budget,
) -> Result<Option<Correlated>> {
    use crate::correlation_stream::{CorrelationStream, Limits};
    let mut stream = CorrelationStream::new(
        config.clone(),
        Limits {
            index_bytes: args.correlation_max_index_bytes,
            disk_bytes: args.correlation_max_disk_bytes,
            record_bytes: (budget.limits.record_bytes as u64)
                .saturating_mul(64)
                .max(65536),
        },
    )?;
    for entry in entries.iter()? {
        if !budget.checkpoint("correlation", 1) {
            return Ok(None);
        }
        stream.push(entry?)?;
    }
    let mut result = Correlated::new(entries)?;
    let finished = stream.finish(
        || budget.checkpoint("correlation", 1),
        |decision| {
            if let Some(operation) = decision.operation {
                result.operations.push(&operation)?;
            }
            if let Some(unmatched) = decision.unmatched {
                result.unmatched_events.push(&unmatched)?;
            }
            Ok(())
        },
    );
    match finished {
        Ok(()) => Ok(Some(result)),
        Err(_) if budget.halted => Ok(None),
        Err(error) => Err(error),
    }
}

fn investigate(cli: &Cli, args: &InvestigateArgs) -> Result<()> {
    if args.question.len() > 100
        || args.question.len() > args.required_relationship.len()
        || args.required_relationship.len() > 100
        || args
            .question
            .iter()
            .chain(&args.required_relationship)
            .any(|item| item.trim().is_empty())
        || args
            .required_relationship
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != args.required_relationship.len()
    {
        return Err("Checklist accepts at most 100 nonempty questions and 100 nonempty, unique required relationship IDs, with at least one ID per question".into());
    }
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
    let (mut config, profile_sources) = if let Some(path) = &cli.config {
        config::load_config_from_path_with_sources(path)?
    } else {
        (
            config::load_config(None, cli.preset.as_deref())?,
            Vec::new(),
        )
    };
    let auto_directory = if args.artifact.is_none() {
        Some(
            tempfile::Builder::new()
                .prefix("log-analyzer-evidence-")
                .tempdir_in(std::env::current_dir()?)?,
        )
    } else {
        None
    };
    let artifact_path = args.artifact.clone().unwrap_or_else(|| {
        auto_directory
            .as_ref()
            .unwrap()
            .path()
            .join("evidence.json")
    });
    let mut protected = args.files.clone();
    protected.extend(profile_sources);
    protected.extend(args.cancel_file.iter().cloned());
    artifact::protect(&artifact_path, cli.output.as_deref(), &protected)?;
    let mut temporary = artifact::stage(&artifact_path)?;
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
    let project_root =
        std::fs::canonicalize(args.project_root.as_deref().unwrap_or(&capture_root))?;
    if !project_root.is_dir() {
        return Err("Project root must be a directory".into());
    }
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
    let planned_bytes = args
        .files
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .fold(0u64, |sum, info| sum.saturating_add(info.len()));
    let mut budget = Budget::new(Limits {
        input_bytes: args.input_max_bytes.unwrap_or(
            planned_bytes
                .saturating_add(1)
                .clamp(16 * 1024 * 1024, 1024 * 1024 * 1024),
        ),
        records: args.processing_max_records.unwrap_or(1_000_000),
        expanded_records: args.processing_max_expanded_records.unwrap_or(100_000),
        work_units: args.processing_max_work.unwrap_or(500_000_000),
        elapsed_ms: args.processing_max_ms.unwrap_or(180_000),
        memory_bytes: args.processing_max_memory_bytes.unwrap_or(
            planned_bytes
                .saturating_mul(512)
                .clamp(512 * 1024 * 1024, 32 * 1024 * 1024 * 1024),
        ),
        artifact_bytes: args.artifact_max_bytes.unwrap_or(1024 * 1024 * 1024),
        record_bytes: args.record_max_bytes.unwrap_or(8 * 1024 * 1024),
        cancel_file: args.cancel_file.clone(),
    });
    let query_bytes = serde_json::to_vec(cli)?.len() as u64;
    budget.reserve(
        "capture",
        configuration_bytes
            .saturating_add(query_bytes)
            .saturating_mul(16),
    );
    let automatic = cli.config.is_none() && cli.preset.is_none();
    // Capture once: detection and analysis must see exactly the same source bytes.
    let mut cached = Vec::new();
    let (mut detection, detection_parse_calls) = if automatic {
        for (ordinal, path) in args.files.iter().enumerate() {
            budget.active_scope = ordinal;
            cached.push(if budget.stop.is_some() {
                None
            } else {
                match capture(path, &mut budget) {
                    Ok(capture) => Some(capture),
                    Err(_) => {
                        budget.stop("capture", "io_error", None);
                        None
                    }
                }
            });
        }
        let directory = args
            .profiles_dir
            .clone()
            .unwrap_or_else(|| project_root.join("config"));
        let directory = if directory.is_absolute() {
            directory
        } else {
            project_root.join(directory)
        };
        artifact::protect_profile_directory(&artifact_path, cli.output.as_deref(), &directory)?;
        let (selected, detection, passes, sources) = auto_profile::detect(
            &args.files,
            &cached,
            &mut budget,
            &directory,
            args.profiles_dir.is_none(),
        );
        protected.extend(sources);
        artifact::protect(&artifact_path, cli.output.as_deref(), &protected)?;
        if let Some(selected) = selected {
            budget.reserve(
                "profile_detection",
                (serde_json::to_vec(&selected)?.len() as u64).saturating_mul(16),
            );
            config = selected;
        }
        if budget.stop.is_some() {
            // Capture failures can prevent shared discovery before it starts.
            // Profile choice is unavailable for every declared input in either case.
            // Later generic parsing can finish without recovering that analysis.
            budget.affected_scopes.extend(0..args.files.len());
        }
        (detection, passes)
    } else {
        (
            json!({"status":"explicit","profile":config.profile_name}),
            0,
        )
    };
    let profile_digest = evidence::profile_digest(&config)?;
    detection["profile_sha256"] = json!(profile_digest);
    detection["project_root"] = evidence::path_value(&project_root);
    detection["origins"] = if let Some(path) = &cli.config {
        json!([{"config":evidence::path_value(path)}])
    } else if let Some(name) = &cli.preset {
        json!([{"preset":name}])
    } else {
        detection["candidates"]
            .as_array()
            .and_then(|candidates| {
                candidates
                    .iter()
                    .find(|candidate| candidate["profile_sha256"] == profile_digest)
            })
            .map(|candidate| candidate["origins"].clone())
            .unwrap_or_else(|| json!([{"preset":"base"}]))
    };
    let mut context = evidence::Context::new(cli, &config)?;
    let mut progress = Vec::new();
    let mut captures = Vec::new();
    let mut spooled = Vec::new();
    let mut processed_ordinals = std::collections::BTreeSet::new();
    let mut parsed_records = 0usize;
    let mut parse_calls = 0usize;
    let snapshot = json!("pending-snapshot");
    let profile_digest = context.metadata(&json!({}), false, &[])["profile_sha256"].clone();
    let construction_item_bytes = (budget.limits.record_bytes as u64)
        .saturating_mul(64)
        .max(8 * 1024 * 1024);
    let construction_disk = construction::Disk::new(args.construction_max_disk_bytes);
    let mut records = construction::Rows::new(construction_disk.clone(), construction_item_bytes)?;
    let mut scopes = Vec::new();
    let mut findings =
        finding_store::Findings::new(construction_disk.clone(), construction_item_bytes)?;
    let mut populations = Vec::new();
    let mut memberships =
        construction::Memberships::new(construction_disk.clone(), construction_item_bytes)?;
    let mut sequences =
        construction::Sequences::new(construction_disk.clone(), construction_item_bytes)?;
    let mut assessments = Vec::new();
    let mut correlation_calls = 0usize;

    for (ordinal, path) in args.files.iter().enumerate() {
        budget.active_scope = ordinal;
        if budget.stop.is_some() && (!automatic || cached[ordinal].is_none()) {
            budget.affected_scopes.insert(ordinal);
            progress.push(json!({"input_ordinal":ordinal,"capture":"unread","consumed_bytes":0,"consumed_sha256":null,"remaining_bytes":null}));
            spooled.push(None);
            captures.push(json!({"input_ordinal":ordinal,"capture":"unread","encoding":"utf8","data":null,"data_omitted":true,"original_consumed_sha256":null,"stored_sha256":null}));
            continue;
        }
        let captured = if automatic {
            cached[ordinal]
                .take()
                .ok_or_else(|| "Input was not captured".into())
        } else {
            capture(path, &mut budget)
        };
        let capture = match captured {
            Ok(capture) => capture,
            Err(_) => {
                budget.stop("capture", "io_error", None);
                progress.push(json!({"input_ordinal":ordinal,"capture":"unread","consumed_bytes":0,"consumed_sha256":null,"remaining_bytes":null}));
                spooled.push(None);
                captures.push(json!({"input_ordinal":ordinal,"capture":"unread","encoding":"utf8","data":null,"data_omitted":true,"original_consumed_sha256":null,"stored_sha256":null}));
                continue;
            }
        };
        let hash = capture.sha256.clone();
        progress.push(json!({"input_ordinal":ordinal,"capture":if capture.complete {"complete"} else {"prefix"},"consumed_bytes":capture.bytes,"consumed_sha256":hash,"remaining_bytes":if capture.complete {Some(0)} else {None}}));
        let working_memory_start = budget.memory_bytes;
        let collection_start = [
            records.len(),
            findings.len(),
            populations.len(),
            memberships.len(),
            sequences.len(),
        ];
        budget.begin_stage();
        parse_calls += 1;
        let mut entries = Entries::new(
            construction_disk.clone(),
            construction_item_bytes,
            args.construction_max_disk_bytes,
        )?;
        let mut selected_count = 0usize;
        let coverage = if capture.parseable_utf8 {
            parser::visit_capture_reader(
                path,
                std::io::BufReader::with_capacity(
                    crate::streaming::CHUNK_BYTES,
                    capture.file.reopen()?,
                ),
                capture.complete,
                &config,
                &mut budget,
                |mut entry| {
                    entry.source_input_ordinal = Some(ordinal);
                    if filter.matches(&entry) {
                        selected_count += usize::from(selected(&entry, &selectors, &config));
                        entries.push(&entry).map_err(|error| {
                            parser::ParseError::InvalidLogFormat(error.to_string())
                        })?;
                    }
                    // The callback persists the record; the parser can release its allowance.
                    Ok(false)
                },
            )
        } else {
            Err(parser::ParseError::InvalidLogFormat(
                "capture is not UTF-8".into(),
            ))
        };
        let coverage = match coverage {
            Ok(mut coverage) => {
                coverage.input_bytes = capture.bytes;
                coverage.snapshot_sha256 = hash.clone();
                coverage
            }
            Err(_) => {
                budget.stop("parse", "unsupported", None);
                let mut parsed = parser::parse_capture(path, b"", false, &config, &mut budget)
                    .map_err(|e| format!("{e:?}"))?;
                parsed.coverage.nonempty_lines = usize::from(capture.bytes != 0);
                parsed.coverage.input_bytes = capture.bytes;
                parsed.coverage.snapshot_sha256 = hash.clone();
                parsed.coverage
            }
        };
        entries.seal()?;
        context.observe_selected_count(&coverage, selected_count);
        let capture_complete = capture.complete;
        captures.push(json!({"input_ordinal":ordinal,"capture":if capture.complete {"complete"} else {"prefix"},"encoding":if capture.utf8 {"utf8"}else{"bytes"},"data":null,"data_omitted":false,"original_consumed_sha256":hash,"stored_sha256":hash}));
        spooled.push(Some(capture));
        processed_ordinals.insert(ordinal);
        parsed_records += coverage.parsed_entries;
        budget.active_scope = ordinal;
        budget.begin_stage();
        for entry in entries.iter()? {
            let entry = entry?;
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            if selected(&entry, &selectors, &config) {
                records.push(&findings::record(&entry, &context, &snapshot))?;
            }
        }
        budget.begin_stage();
        correlation_calls += 1;
        let perf = match correlate(&entries, &config, args, &mut budget) {
            Ok(perf) => perf,
            Err(error) => {
                let reason =
                    if error.to_string().contains("limit") || error.to_string().contains("full") {
                        "storage_limit"
                    } else {
                        "io_error"
                    };
                budget.stop("correlation", reason, Some("correlation_state"));
                None
            }
        };
        budget.begin_stage();
        let view = selection::View::new(&entries, &selectors, &config, perf.as_ref(), &mut budget)?;
        let scope_id = format!("scope-{ordinal}");
        let structural_loss = coverage.rejected_candidates > 0
            || !coverage.normalization_diagnostics.is_empty()
            || coverage.is_unparsed();
        let semantic_status = if config.event_classifier().is_none() {
            "unsupported"
        } else if perf.is_some() {
            if view.conflicting > 0 || view.ambiguous > 0 {
                "conflicting"
            } else if view.relevant == 0
                || view.rejected > 0
                || view.classification_loss > 0
                || structural_loss
            {
                "insufficient_evidence"
            } else {
                "supported"
            }
        } else {
            "not_performed"
        };
        let coverage = perf.as_ref().map(|_| &view);
        scopes.push(json!({"id":scope_id,"input_ordinals":[ordinal],"selection":"Legacy global filters apply before correlation; exact selectors apply to retained occurrences after shared correlation. Each input is an independent run.","extent":"declared_input","completeness":if structural_loss{"partial"}else{"complete"},"analysis_completion":if perf.is_some(){"complete"}else{"not_performed"},"correlation_scope":[],"semantic_coverage":{"status":semantic_status,"relevant_records":coverage.map(|c|c.relevant),"classified_records":coverage.map(|c|c.classified),"paired_events":coverage.map(|c|c.pairs * 2),"unmatched_events":coverage.map(|c|c.unmatched),"ambiguous_events":coverage.map(|c|c.ambiguous),"rejected_events":coverage.map(|c|c.rejected),"reason":"Effective profile validated structurally and assessed against the observed sample; no automatic semantic proof or upstream completeness claim."},"upstream_completeness":"unknown"}));
        let severity_complete = capture_complete && !structural_loss && budget.stop.is_none();
        budget.begin_stage();
        findings::streamed_observed_levels(
            &view.entries,
            &scope_id,
            &context,
            &snapshot,
            severity_complete,
            &mut budget,
            &mut findings,
            &mut populations,
            &mut memberships,
        )?;
        for declaration in &config.resource_observations {
            resource_observations::analyze_stream(
                &view.entries,
                &scope_id,
                &context,
                &snapshot,
                declaration,
                &mut budget,
                &mut findings,
            )?;
        }
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
        )?;
        budget.begin_stage();
        if let Some(policy) = &config.investigation {
            chains::calculate_stream(
                policy,
                &config,
                &entries,
                &selectors,
                &scope_id,
                &context,
                &snapshot,
                !structural_loss && budget.stop.is_none(),
                &mut budget,
                &mut findings,
            )?;
            policy::calculate_stream(
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
            )?;
        }
        let scope_aliases = scope_state::analyze(
            &entries,
            &view,
            &config,
            &scope_id,
            &context,
            &snapshot,
            &mut budget,
            &mut findings,
        )?;
        if structural_loss {
            for population in populations.iter_mut().filter(|p| p["scope_id"] == scope_id) {
                population["completeness"] = json!("partial");
            }
            for sequence in sequences
                .headers_mut()
                .filter(|s| s["scope_id"] == scope_id)
            {
                sequence["completeness"] = json!("partial");
            }
        }
        let lifecycle_status = if semantic_status == "supported" && view.boundaries == 0 {
            "unsupported"
        } else if semantic_status == "supported" && view.unrecognized_boundaries > 0 {
            if view.unrecognized_boundaries == view.boundary_requirements {
                "unsupported"
            } else {
                "insufficient_evidence"
            }
        } else if semantic_status == "supported" && view.identity_only > 0 {
            "insufficient_evidence"
        } else {
            semantic_status
        };
        let timing_status = if lifecycle_status == "not_performed" {
            "insufficient_evidence"
        } else if lifecycle_status != "supported" {
            lifecycle_status
        } else if view.pairs == 0
            || view.unmatched > 0
            || view.excluded_boundaries > 0
            || !view.reliable
            || scope_aliases.as_ref().is_none_or(|aliases| *aliases != 0)
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
            assessments.push(json!({"goal":goal,"scope_id":scope_id,"status":if goal=="inspection"{if structural_loss{"insufficient_evidence"}else{"supported"}}else if goal=="slow_operations" { timing_status } else if goal=="incomplete_lifecycles"{if lifecycle_status=="not_performed"{"insufficient_evidence"}else{lifecycle_status}}else if goal=="failures" && view.classification_loss>0{semantic_status}else if goal=="failures" && !view.entries.is_empty() && !view.failures_supported{if view.some_failures_supported{"insufficient_evidence"}else{"unsupported"}}else if semantic_status=="not_performed"{"insufficient_evidence"}else{semantic_status},"reason":if goal!="inspection" && view.entries.is_empty(){"Exact selection contains zero processed records; lifecycle support cannot be established from unrelated input."}else if matches!(goal,"slow_operations"|"incomplete_lifecycles") && view.unrecognized_boundaries>0 {"The explicit profile cannot establish opposite-boundary recognition for every selected lifecycle; missing boundaries remain unavailable."}else if structural_loss {"Rejected or unparsed source candidates may contain relevant evidence; retained calculations cover only parsed selected records. See per-input coverage diagnostics."}else if matches!(goal,"slow_operations"|"incomplete_lifecycles") && view.identity_only>0 {"Selected identity-only records lack lifecycle boundary semantics; boundary-derived facts do not establish their lifecycle coverage."}else if goal=="inspection" {"Counts and source evidence describe only the selected processed population."}else{"Only explicit profile semantics and observed evidence support this assessment; domain attempts/resources/causal relationships require declared rules."},"finding_ids":[]}));
        }
        // Parsed and correlation working sets end here. Keep an allowance for the
        // JSON evidence and coverage metadata that survive this independent input.
        let retained_bytes = [&populations[collection_start[2]..]].iter().try_fold(
            context.last_input_serialized_bytes()?,
            |sum, values| -> Result<u64> {
                Ok(sum.saturating_add(serde_json::to_vec(values)?.len() as u64))
            },
        )?;
        budget.memory_bytes = working_memory_start;
        budget.reserve("calculation", retained_bytes.saturating_mul(16));
    }
    // Unread independent inputs retain their own unavailable analysis scopes.
    for ordinal in (0..args.files.len()).filter(|ordinal| !processed_ordinals.contains(ordinal)) {
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
        for sequence in sequences.headers_mut() {
            if affected.contains(sequence["scope_id"].as_str().unwrap()) {
                sequence["completeness"] = json!("partial");
            }
        }
    }
    let mut metadata = crate::build_info::metadata(&config.profile_name);
    metadata["evidence"] = context.metadata(&json!({}), cli.redact, &cli.mask_id);
    let snapshot = metadata["evidence"]["snapshot_id"].clone();
    for (index, population) in populations.iter_mut().enumerate() {
        population["membership"]["sha256"] =
            json!(memberships.digest(index, &snapshot, cli.redact)?);
    }
    metadata["profile_selection"] = auto_profile::summary(&detection);
    // Instrumentation belongs to the effective query and its digest, never to evidence text.
    metadata["evidence"]["query"]["execution"] = json!({"parse_passes":parse_calls + detection_parse_calls,"analysis_parse_passes":parse_calls,"detection_parse_passes":detection_parse_calls,"correlation_passes":correlation_calls,"input_relationship":"independent_runs","record_max_bytes":budget.limits.record_bytes,"planned_input_bytes":planned_bytes,"working_sets":"sequential_independent_inputs","correlation_state":{"index_bytes":args.correlation_max_index_bytes,"disk_bytes":args.correlation_max_disk_bytes,"backend":"temporary_files_with_threshold_sqlite_index"},"profile_selection":detection,"source_locations":source_locations});
    metadata["evidence"]["query_sha256"] = json!(evidence::digest(
        metadata["evidence"]["query"].to_string().as_bytes()
    ));
    let mut usage = budget.usage_json();
    usage["records"] = json!(parsed_records);
    let processing = json!({"status":if budget.stop.is_none(){"complete"}else{"partial"},"stop":budget.stop,"limits":budget.limits_json(),"usage":usage,"inputs":progress});
    let mut assessment_witnesses = std::collections::BTreeMap::new();
    for row in findings.iter()? {
        let row = row?;
        let kind = row.value["kind"].as_str().unwrap_or("");
        if !matches!(kind, "observation" | "measurement" | "calculated_fact") {
            continue;
        }
        let scope = row.value["scope_id"]
            .as_str()
            .ok_or("missing finding scope")?;
        let id = row.value["id"].as_str().ok_or("missing finding ID")?;
        for goal in [
            "inspection",
            "failures",
            "slow_operations",
            "incomplete_lifecycles",
        ] {
            let rank = match goal {
                "inspection" if id.ends_with("-physical-records-count") => 0,
                "inspection" if id.ends_with("-normalized-records-count") => 1,
                "failures" if id.ends_with("-failures-count") => 0,
                "failures" if id.ends_with("-observed-errors-count") => 1,
                "slow_operations" if kind == "measurement" => 0,
                "slow_operations" if id.ends_with("-paired-lifecycles-count") => 1,
                "incomplete_lifecycles"
                    if id.ends_with("-starts-count") || id.ends_with("-ends-count") =>
                {
                    0
                }
                "incomplete_lifecycles" if id.ends_with("-paired-lifecycles-count") => 1,
                _ => 2,
            };
            let witness = assessment_witnesses
                .entry((scope.to_owned(), goal))
                .or_insert((rank, row.value["id"].clone()));
            if rank < witness.0 {
                *witness = (rank, row.value["id"].clone());
            }
        }
    }
    for assessment in &mut assessments {
        let key = (
            assessment["scope_id"]
                .as_str()
                .ok_or("missing assessment scope")?
                .to_owned(),
            assessment["goal"]
                .as_str()
                .ok_or("missing assessment goal")?,
        );
        if let Some((_, id)) = assessment_witnesses.get(&key) {
            assessment["finding_ids"] = json!([id]);
        }
    }
    let retention = json!({"policy":"until_deleted","expires_at":null});
    let mut retained = json!({"contract_version":1,"investigation_contract_version":1,"report_metadata":metadata,"processing":processing,"content":"original","effective_profile":serde_json::to_value(&config)?,"effective_profile_omitted":false,"retention":retention,"verification":findings::verification(false, "available")});
    // Move large collections into the artifact; json! would serialize borrowed
    // vectors into duplicate trees and retain the originals until this call ends.
    for (key, values) in [
        ("scopes", scopes),
        ("assessments", assessments),
        ("populations", populations),
        ("findings", Vec::new()),
        ("records", Vec::new()),
        ("captured_inputs", captures),
        ("memberships", Vec::new()),
        ("sequences", sequences.metadata()),
    ] {
        retained[key] = serde_json::Value::Array(values);
    }
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
    // Validate complete private evidence even when the publication cap prevents reuse.
    // An unavailable artifact must never bypass deterministic fact verification.
    if !artifact::write_json(
        &retained,
        &artifact::ArtifactRows {
            captures: &spooled,
            records: &records,
            memberships: &memberships,
            sequences: &sequences,
            findings: &findings,
        },
        temporary.as_file_mut(),
        construction_disk.remaining(),
    )? {
        return Err("construction disk byte limit exceeded while staging checked evidence".into());
    }
    let artifact_size = temporary.as_file().metadata()?.len();
    let _staging_reservation = construction_disk.reserve(artifact_size)?;
    let private_hash = artifact::file_digest(temporary.as_file_mut())?;
    let hash = if artifact_size <= budget.limits.artifact_bytes {
        Some(private_hash.clone())
    } else {
        None
    };
    let mut report = artifact::report(&retained, &artifact_path, hash.as_deref());
    drop(retained);
    let mut indexed = indexed_artifact::IndexedArtifact::open(
        temporary.as_file().try_clone()?,
        indexed_artifact::Limits {
            item_bytes: construction_item_bytes,
            index_bytes: args.construction_max_disk_bytes,
            state_bytes: args.construction_max_disk_bytes,
            elapsed_ms: budget.limits.elapsed_ms.max(180_000),
            work_units: budget.limits.work_units.max(500_000_000),
        },
    )?;
    indexed.validate()?;
    if cli.redact
        && hash.is_some()
        && let Some(directory) = &auto_directory
    {
        report["artifact"]["location"] = json!(format!(
            "{}/evidence.json",
            directory.path().file_name().unwrap().to_string_lossy()
        ));
    }
    if hash.is_some() {
        let saved = (|| -> Result<()> {
            temporary.as_file().sync_all()?;
            temporary.persist_noclobber(&artifact_path)?;
            if let Some(directory) = auto_directory {
                let _ = directory.keep();
            }
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
    let page = report_stream::Page::build(
        report,
        &findings,
        cli,
        report_stream::Options {
            disk: construction_disk.clone(),
            item_limit: construction_item_bytes,
            snapshot: &snapshot,
            redacted: cli.redact,
            unverifiable: &captures_unverifiable_inputs,
        },
    )?;
    let overview =
        if !args.brief && matches!(cli.effective_format(), crate::cli::OutputFormat::Text) {
            Some(presentation::finding_overview(
                &page.metadata,
                config
                    .investigation_view
                    .as_ref()
                    .filter(|_| page.metadata["artifact"]["status"] != "unavailable"),
                cli.redact,
                || {
                    Ok(findings.iter()?.map(|row| {
                        row.map(|row| {
                            findings.transformed(
                                &row,
                                &snapshot,
                                cli.redact,
                                &captures_unverifiable_inputs,
                            )
                        })
                    }))
                },
            )?)
        } else {
            None
        };
    report_checks::validate(
        &page,
        &findings,
        &indexed,
        &private_hash,
        &construction_disk,
    )?;
    let mut rendered = page.render(&findings, cli, args.brief, overview.as_ref())?;
    if let Some(staged) = staged_report
        && let Err(error) = staged.save_reader(&mut rendered)
    {
        if cli.redact {
            eprintln!("Evidence artifact outcome is reported below; saving the report failed");
        } else {
            eprintln!(
                "Evidence artifact outcome is reported below; saving the report failed: {error}"
            );
        }
    }
    rendered.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut rendered, &mut std::io::stdout().lock())?;
    Ok(())
}
