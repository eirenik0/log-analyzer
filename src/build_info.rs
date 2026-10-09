use clap::{CommandFactory, ValueEnum};
use serde_json::{Value, json};

pub const SCHEMA_VERSION: u32 = 1;

pub fn identity() -> Value {
    let revision = env!("LOG_ANALYZER_REVISION");
    json!({"package_version":env!("CARGO_PKG_VERSION"), "source_revision":if revision.is_empty() {None} else {Some(revision)}, "source_state":env!("LOG_ANALYZER_BUILD_STATE")})
}

pub fn metadata(profile: &str) -> Value {
    json!({"schema_version":SCHEMA_VERSION,"build":identity(),"active_profile":profile,"event_classification_contract":2})
}

pub fn capabilities() -> Value {
    let command = crate::cli::Cli::command();
    let commands: Vec<_> = command
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(|c| c.get_name())
        .collect();
    let formats: Vec<_> = crate::cli::OutputFormat::value_variants()
        .iter()
        .map(|f| f.to_possible_value().unwrap().get_name().to_string())
        .collect();
    let parsers: Vec<_> = [
        crate::config::LogFormat::Auto,
        crate::config::LogFormat::Classic,
        crate::config::LogFormat::RustTracing,
        crate::config::LogFormat::Syslog,
        crate::config::LogFormat::JsonLines,
    ]
    .iter()
    .map(|p| serde_json::to_value(p).unwrap())
    .collect();
    json!({"schema_version":SCHEMA_VERSION,"build":identity(),"commands":commands,"report_schemas":{"report":serde_json::from_str::<Value>(include_str!("../schemas/report.schema.json")).expect("embedded report schema is valid JSON"),"capabilities":serde_json::from_str::<Value>(include_str!("../schemas/capabilities.schema.json")).expect("embedded capabilities schema is valid JSON"),"profile_association":serde_json::from_str::<Value>(include_str!("../schemas/profile-association.schema.json")).expect("embedded association schema is valid JSON"),"profile_expectations":serde_json::from_str::<Value>(include_str!("../schemas/profile-expectations.schema.json")).expect("embedded expected facts schema is valid JSON"),"investigation":serde_json::from_str::<Value>(include_str!("../schemas/investigation.schema.json")).expect("embedded investigation schema is valid JSON"),"evidence_artifact":serde_json::from_str::<Value>(include_str!("../schemas/evidence-artifact.schema.json")).expect("embedded artifact schema is valid JSON"),"investigation_contract_versions":[crate::investigation::CONTRACT_VERSION],"evidence_contract_version":crate::evidence::CONTRACT_VERSION},"investigation_contracts":{"versions":[crate::investigation::CONTRACT_VERSION],"artifact_version":crate::investigation::ARTIFACT_VERSION,"command_available":false,"artifact_retrieval_available":false,"schemas":{"1":"investigation","artifact":"evidence_artifact"},"occurrence_identity":["snapshot_id","input_ordinal","reference_id"]},"bounded_reports":{"version":1,"commands":["info","search","extract","perf","trace","process","compare","diff","llm-diff","errors","validate-profile","resolve-profile"],"format":"compact_json","budget_units":["serialized_utf8_bytes","serialized_unicode_scalars","presentation_items"],"default_page_items":100,"cursor_binding":["input_snapshot","effective_profile","effective_query","redaction","complete_report_sha256"],"source_records":"/evidence_records","metadata_over_budget":"explicit_exception","atomic_oversized_item":"increase_budget_or_complete_output","complete_flag":"--complete-output"},"profile_validation":{"version":1,"command":"validate-profile","purposes":["recognition","timing"],"statuses":["supported","unsupported","conflicting","insufficient_evidence"],"expected_facts_schema":"profile_expectations","candidate_activation":false,"success_exit":"supported_on_observed_sample_only"},"profile_resolution":{"version":1,"command":"resolve-profile","candidate_activation":false,"automatic_basis":"complete_requested_population_semantic_assertions","association":"read_only_version_1","precedence":["explicit","revalidated_association","unique_asserted_candidate"]},"output_formats":formats,"parser_formats":parsers,"presets":crate::config::builtin_template_names(),"event_classification":{"schema_versions":[1,2],"kinds":["command","request","event"],"adapters":["text","structured"],"legacy_marker_compatibility":true,"command_rules":"deprecated_command_only","coverage_basis":"selected_parsed_records_before_operation_type_and_display_limits"}})
}
