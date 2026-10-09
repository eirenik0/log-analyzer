//! Stateless discovery: observed compatibility and independently asserted semantics
//! remain separate, and resolution never activates or writes a candidate.
use crate::{
    cli::{Cli, Commands},
    config::{self, AnalyzerConfig, OperationKind},
    evidence, parser,
    profile_validation::{self},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Association {
    version: u32,
    profile: ProfileChoice,
    sources: Vec<SourceShape>,
    event_contract: u32,
    structural_contract: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileChoice {
    preset: Option<String>,
    config: Option<PathBuf>,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceShape {
    file: String,
    selected_parser: config::LogFormat,
}

pub(crate) fn generated_association_reason(reason: &str) -> bool {
    matches!(
        reason,
        "association_contract_changed"
            | "association_source_scope_mismatch"
            | "association_requires_exactly_one_profile_selector"
            | "association_profile_digest_changed"
            | "source_structure_unavailable"
            | "source_structure_changed_or_incompatible"
            | "empty_source_structure_unverified"
            | "explicit_configuration_wins"
    )
}

fn identity(config: &AnalyzerConfig, origin: &str, choice: Value) -> Value {
    json!({"name":config.profile_name,"sha256":evidence::profile_digest(config).expect("config serializes"),"origin":origin,"choice":choice})
}

pub fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let Commands::ResolveProfile {
        files,
        kind,
        purpose,
        expected,
        candidate_config,
        association,
    } = &cli.command
    else {
        unreachable!()
    };
    let paths: BTreeSet<_> = candidate_config.iter().collect();
    if paths.len() > 16 {
        return Err("At most 16 candidate-config alternatives are allowed".into());
    }
    let kind = match kind {
        crate::cli::OperationType::Request => OperationKind::Request,
        crate::cli::OperationType::Command => OperationKind::Command,
        crate::cli::OperationType::Event => OperationKind::Event,
    };
    let filter = crate::build_filter(&cli.filter)?;
    let (facts, facts_digest) = profile_validation::load_expectations(expected.as_deref())?;
    let generic = config::default_config();
    crate::output::set_metadata(crate::build_info::metadata(&generic.profile_name), false);
    crate::output::set_evidence(evidence::Context::new(cli, generic)?);
    let inspected = crate::read_analysis_files_impl(
        files,
        generic,
        &filter,
        crate::OutputFormat::Json,
        None,
        true,
    )?;
    let snapshots: Vec<_> = inspected
        .coverage
        .files
        .iter()
        .map(|f| (f.snapshot_sha256.clone(), f.input_bytes))
        .collect();
    let mut candidates = Vec::new();
    let explicit = cli.config.is_some() || cli.preset.is_some();
    let mut mapping = json!({"status":"not_supplied","reason":null});
    let mut choices: Vec<(String, Value, Result<AnalyzerConfig, String>)> = Vec::new();
    if explicit {
        let choice =
            json!({"config":cli.config.as_deref().map(evidence::path_label),"preset":cli.preset});
        choices.push((
            "explicit".into(),
            choice,
            config::load_config(cli.config.as_deref(), cli.preset.as_deref())
                .map_err(|e| e.to_string()),
        ));
        if association.is_some() {
            mapping = json!({"status":"bypassed_by_explicit_choice","reason":"explicit_configuration_wins"});
        }
    } else if let Some(path) = association {
        match load_association(path, files) {
            Err(reason) => {
                mapping = json!({"status":"invalid","reason":if generated_association_reason(&reason){reason}else{crate::output::source_path(&reason)}})
            }
            Ok((saved, config, choice)) => {
                mapping = json!({"status":"pending_revalidation","reason":null});
                let expected_shapes: Vec<_> = saved
                    .sources
                    .iter()
                    .map(|s| json!({"file":s.file,"selected_parser":s.selected_parser}))
                    .collect();
                choices.push((
                    "association".into(),
                    json!({"selector":choice,"expected_shapes":expected_shapes}),
                    Ok(config),
                ));
            }
        }
    }
    let mut builtin_names = config::builtin_template_names().to_vec();
    builtin_names.sort();
    for name in builtin_names {
        choices.push((
            "builtin".into(),
            json!({"preset":name}),
            Ok(config::load_builtin_template(name).expect("known built-in")),
        ));
    }
    for path in paths {
        choices.push((
            "candidate_config".into(),
            json!({"config":evidence::path_label(path)}),
            config::load_config_from_path(path).map_err(|e| e.to_string()),
        ));
    }
    'candidate: for (origin, choice, loaded) in choices {
        match loaded {
            Err(error)=>candidates.push(json!({"origin":origin,"choice":choice,"status":"invalid_configuration","error":crate::output::source_path(&error),"identity":null,"eligible":false})),
            Ok(config)=> {
                let mut inputs=Vec::new();let mut coverage=Vec::new();
                let mut context=evidence::Context::new(cli,&config)?;
                for (ordinal,path) in files.iter().enumerate() {
                    let mut parsed=match parser::parse_log_file_report(path,&config) {
                        Ok(parsed)=>parsed,
                        Err(error)=>{
                            if origin=="association" {mapping=json!({"status":"invalid","reason":"source_structure_unavailable"});}
                            candidates.push(json!({"origin":origin,"choice":choice,"identity":identity(&config,&origin,choice.clone()),"status":"input_structure_unavailable","error":crate::output::source_path(&format!("{error:?}")),"eligible":false}));
                            continue 'candidate;
                        }
                    };
                    if (parsed.coverage.snapshot_sha256.clone(),parsed.coverage.input_bytes)!=snapshots[ordinal] {return Err("Input changed during profile discovery; restart on immutable inputs".into());}
                    for entry in &mut parsed.entries {entry.source_input_ordinal=Some(ordinal);}
                    crate::output::observe_entries(&parsed.entries);
                    context.observe(&parsed.coverage,&parsed.entries);
                    coverage.push(parsed.coverage);inputs.push(parsed.entries);
                }
                let parsed=coverage.iter().all(|f| !f.is_unparsed());
                let has_entries=inputs.iter().flatten().next().is_some();
                let selected_population=inputs.iter().flatten().any(|e|filter.matches(e));
                let complete=parsed && coverage.iter().all(|f|f.rejected_candidates==0 && f.structural_diagnostics.unsupported_python_headers==0 && f.structural_diagnostics.diagnostic_count==0);
                let mut validation=profile_validation::analyze(&inputs,&config,&filter,kind,*purpose,facts.as_ref(),facts_digest.clone());
                if !parsed {validation["profile_validation"]["suitability"]=json!({"status":"insufficient_evidence","reason":"input_structure_unavailable","basis":"observed_sample_only","semantic_correctness":"not_established_by_match_count"});}
                if parsed && has_entries && !selected_population && validation["profile_validation"]["expected_facts"]["status"]!="failed" {
                    validation["profile_validation"]["suitability"]["reason"]=json!("zero_filter_matches");
                }
                let semantic=profile_validation::selection_evidence(&inputs,&filter,kind,*purpose,facts.as_ref(),&validation["profile_validation"]);
                let mut association_valid=true;
                if origin=="association" {
                    let shapes:Vec<_>=coverage.iter().map(|f|json!({"file":f.file,"selected_parser":f.selected_parser})).collect();
                    let empty_sources:Vec<_>=coverage.iter().enumerate().filter_map(|(i,f)|(f.nonempty_lines==0).then_some(i)).collect();
                    let incompatible=coverage.iter().enumerate().any(|(i,f)|f.nonempty_lines>0 && (f.is_unparsed() || f.rejected_candidates>0 || f.structural_diagnostics.diagnostic_count>0 || choice["expected_shapes"][i]!=shapes[i]));
                    association_valid=!incompatible && empty_sources.is_empty() && complete;
                    mapping=if incompatible {json!({"status":"invalid","reason":"source_structure_changed_or_incompatible"})}
                        else if !empty_sources.is_empty() {json!({"status":"insufficient_evidence","reason":"empty_source_structure_unverified","input_ordinals":empty_sources})}
                        else {json!({"status":"revalidated","reason":null})};
                    mapping["semantic_proof"]=json!("independent_assertions_required_for_automatic_selection");
                }
                let eligible=complete && association_valid && semantic["status"]=="sufficient_on_assertion_covered_sample";
                context.annotate(&mut validation);
                let recognition=profile_validation::analyze(&inputs,&config,&filter,kind,profile_validation::Purpose::Recognition,None,None);
                let timing=profile_validation::analyze(&inputs,&config,&filter,kind,profile_validation::Purpose::Timing,None,None);
                let support=json!({"recognition":recognition["profile_validation"]["suitability"],"timing":timing["profile_validation"]["suitability"],"correlation_scope_diagnostics":validation["profile_validation"]["totals"]["scope_alias_groups"]});
                let candidate_evidence=context.metadata(&validation,cli.redact,&cli.mask_id);
                let id=identity(&config,&origin,choice.clone());
                candidates.push(json!({"origin":origin,"choice":choice,"identity":id,"status":"evaluated","eligible":eligible,
                    "parsing":{"status":if parsed && !has_entries{"empty_input"}else if complete{"no_reported_structural_rejections"}else if parsed{"reported_structural_rejections"}else{"unavailable"},"coverage":coverage},
                    "support":support,"evidence_records":context.records(),"semantic_evidence":semantic,"profile_validation":validation["profile_validation"],"evidence":candidate_evidence}));
            }
        }
    }
    // Identical effective configurations are one alternative; distinct rules remain
    // ambiguous even when the selected observed sample yields equal outputs.
    let mut eligible_digests = BTreeSet::new();
    let eligible: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            (c["eligible"] == true
                && eligible_digests.insert(c["identity"]["sha256"].as_str().unwrap().to_owned()))
            .then_some(i)
        })
        .collect();
    let remembered = candidates
        .iter()
        .position(|c| c["origin"] == "association" && c["eligible"] == true);
    let (status, selected, provenance) = if explicit {
        if candidates[0]["status"] == "invalid_configuration" {
            ("invalid_explicit_choice", None, "explicit")
        } else if candidates[0]["profile_validation"]["suitability"]["status"] != "supported"
            || candidates[0]["parsing"]["status"] != "no_reported_structural_rejections"
        {
            ("unsupported_explicit_choice", Some(0), "explicit")
        } else {
            ("selected", Some(0), "explicit")
        }
    } else if let Some(i) = remembered {
        ("selected", Some(i), "association")
    } else if eligible.len() == 1 {
        (
            "selected",
            Some(eligible[0]),
            "independently_asserted_candidate",
        )
    } else if eligible.len() > 1 {
        ("ambiguous", None, "none")
    } else {
        ("insufficient_evidence", None, "none")
    };
    let mut report = json!({"coverage":inspected.coverage,"profile_resolution":{"version":1,"status":status,"requested":{"kind":kind,"purpose":purpose},"selection_provenance":provenance,
        "selected":selected.map(|i|&candidates[i]["identity"]),"selected_candidate_index":selected,"eligible_distinct_profiles":eligible.len(),"association":mapping,"candidates":candidates,
        "candidate_activation":false,"generic_inspection":{"profile":"base","status":inspected.coverage.status,"records":inspected.coverage.parsed_entries,"filter_matches":inspected.coverage.filter_matches},
        "next_steps":if status=="selected"{vec!["Use the selected explicit preset/config in subsequent analysis; no profile was activated"]}else{vec!["Inspect candidate validation and structural diagnostics", "Supply independently known requested-kind identity, phase, scope and timing-boundary assertions; or validate an explicit candidate"]},
        "limitations":["All support is scoped to the consumed sample and supplied assertions", "Unclassified records and missed lifecycles remain semantically unknown", "Capture completeness, clock synchronization and causality are not established", "Association files are read-only; project/user persistence management is separate", "Candidates may parse inputs separately; work and memory are not globally bounded"]}});
    // The outer metadata describes generic inspection. Each alternative owns its
    // consumed-byte/profile identity and source references inside candidates.
    report["profile_resolution"]["metadata_scope"] =
        json!("outer_generic_inspection_candidate_specific_evidence");
    let rendered = serde_json::to_string_pretty(&report)?;
    crate::output::print(format_args!("{rendered}\n"));
    if let Some(path) = &cli.output {
        crate::write_output_file(path, &rendered)?;
    }
    if status != "selected" {
        return Err("Profile resolution did not select a supported profile; inspect candidates, assumptions and next steps".into());
    }
    Ok(())
}

fn load_association(
    path: &Path,
    files: &[PathBuf],
) -> Result<(Association, AnalyzerConfig, Value), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("association_unreadable: {e}"))?;
    let saved: Association =
        serde_json::from_slice(&bytes).map_err(|e| format!("association_malformed: {e}"))?;
    if saved.version != 1 || saved.event_contract != 2 || saved.structural_contract != 1 {
        return Err("association_contract_changed".into());
    }
    if saved.sources.len() != files.len()
        || saved
            .sources
            .iter()
            .zip(files)
            .any(|(s, f)| s.file != evidence::path_label(f))
    {
        return Err("association_source_scope_mismatch".into());
    }
    if saved.profile.preset.is_some() == saved.profile.config.is_some() {
        return Err("association_requires_exactly_one_profile_selector".into());
    }
    let config_path = saved.profile.config.as_ref().map(|p| {
        if p.is_absolute() {
            p.clone()
        } else {
            path.parent().unwrap_or(Path::new(".")).join(p)
        }
    });
    let config = config::load_config(config_path.as_deref(), saved.profile.preset.as_deref())
        .map_err(|e| format!("association_profile_unavailable: {e}"))?;
    let choice = json!({"preset":saved.profile.preset,"config":config_path.as_deref().map(evidence::path_label)});
    if identity(&config, "association", choice.clone())["sha256"] != saved.profile.sha256 {
        return Err("association_profile_digest_changed".into());
    }
    Ok((saved, config, choice))
}
