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
    capability_report(command_names())
}

fn command_names() -> Vec<String> {
    let command = crate::cli::Cli::command();
    command
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(|c| c.get_name().to_owned())
        .collect()
}

fn capability_report(commands: Vec<String>) -> Value {
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
    let mut report = serde_json::Map::new();
    report.insert("schema_version".into(), json!(SCHEMA_VERSION));
    report.insert("build".into(), json!(identity()));
    report.insert("commands".into(), json!(commands));
    report.insert("report_schemas".into(), report_schemas_capability());
    report.insert(
        "investigation_contracts".into(),
        investigation_contracts_capability(),
    );
    report.insert("bounded_reports".into(), bounded_reports_capability());
    report.insert(
        "profile_preparation".into(),
        profile_preparation_capability(),
    );
    report.insert("profile_validation".into(), profile_validation_capability());
    report.insert("profile_resolution".into(), profile_resolution_capability());
    report.insert("profile_mappings".into(), profile_mappings_capability());
    report.insert("output_formats".into(), json!(formats));
    report.insert("parser_formats".into(), json!(parsers));
    report.insert(
        "presets".into(),
        json!(crate::config::builtin_template_names()),
    );
    report.insert(
        "profiles".into(),
        json!({
            "option":"profile", "environment":"LOG_ANALYZER_PROFILE",
            "builtins":crate::config::builtin_template_names(),
            "selection":"builtin_name_or_file_path",
            "compatibility_aliases":["config","preset"]
        }),
    );
    report.insert(
        "event_classification".into(),
        event_classification_capability(),
    );
    Value::Object(report)
}

fn report_schemas_capability() -> Value {
    json!({"report":serde_json::from_str::<Value>(include_str!("../schemas/report.schema.json")).expect("embedded report schema is valid JSON"),"capabilities":serde_json::from_str::<Value>(include_str!("../schemas/capabilities.schema.json")).expect("embedded capabilities schema is valid JSON"),"profile_mappings":serde_json::from_str::<Value>(include_str!("../schemas/profile-mappings.schema.json")).expect("embedded mapping schema is valid JSON"),"profile_association":serde_json::from_str::<Value>(include_str!("../schemas/profile-association.schema.json")).expect("embedded association schema is valid JSON"),"profile_expectations":serde_json::from_str::<Value>(include_str!("../schemas/profile-expectations.schema.json")).expect("embedded expected facts schema is valid JSON"),"investigation":serde_json::from_str::<Value>(include_str!("../schemas/investigation.schema.json")).expect("embedded investigation schema is valid JSON"),"evidence_artifact":serde_json::from_str::<Value>(include_str!("../schemas/evidence-artifact.schema.json")).expect("embedded artifact schema is valid JSON"),"investigation_contract_versions":[crate::investigation::CONTRACT_VERSION],"evidence_contract_version":crate::evidence::CONTRACT_VERSION})
}

fn investigation_contracts_capability() -> Value {
    json!({"versions":[crate::investigation::CONTRACT_VERSION],"artifact_version":crate::investigation::ARTIFACT_VERSION,"command_available":true,"artifact_retrieval_available":true,"profile_detection":{"default":true,"method":"unique_profile_lifecycle_grammar","overrides":["profile","config","preset"],"sources":["builtin","config_directory"],"default_directory":"config","directory_option":"profiles-dir"},"schemas":{"1":"investigation","artifact":"evidence_artifact"},"occurrence_identity":["snapshot_id","input_ordinal","reference_id"]})
}

fn bounded_reports_capability() -> Value {
    json!({"version":1,"commands":["info","search","extract","perf","trace","process","compare","diff","llm-diff","errors","validate-profile","resolve-profile"],"format":"compact_json","budget_units":["serialized_utf8_bytes","serialized_unicode_scalars","presentation_items"],"default_page_items":100,"cursor_binding":["input_snapshot","effective_profile","effective_query","redaction","complete_report_sha256"],"source_records":"/evidence_records","metadata_over_budget":"explicit_exception","atomic_oversized_item":"increase_budget_or_complete_output","complete_flag":"--complete-output"})
}

fn profile_preparation_capability() -> Value {
    json!({"version":1,"command":"prepare-profile","candidate_activation":false,"candidate_overwrite":false,"validation":"observed_sample_and_optional_independent_assertions","retries":false,"presentation":"bounded_representatives_with_explicit_omissions","common_cursors":false})
}

fn profile_validation_capability() -> Value {
    json!({"version":1,"command":"validate-profile","purposes":["recognition","timing"],"statuses":["supported","unsupported","conflicting","insufficient_evidence"],"expected_facts_schema":"profile_expectations","candidate_activation":false,"success_exit":"supported_on_observed_sample_only"})
}

fn profile_resolution_capability() -> Value {
    json!({"version":2,"retained_schema_versions":[1,2],"command":"resolve-profile","candidate_activation":false,"automatic_basis":"complete_requested_population_semantic_assertions","association":"read_only_version_1","precedence":["explicit","revalidated_association","revalidated_project_mapping","revalidated_user_mapping","unique_asserted_candidate"]})
}

fn profile_mappings_capability() -> Value {
    json!({"version":1,"command":"profile-mappings","actions":["inspect","remember","replace","forget"],"scopes":["project","user"],"raw_evidence_persisted":false,"mutation_requires":"explicit_command","remember_replace_require":"current_independent_validation","replace_forget_guard":"entry_digest"})
}

fn event_classification_capability() -> Value {
    json!({"schema_versions":[1,2],"kinds":["command","request","event"],"adapters":["text","structured"],"legacy_marker_compatibility":true,"command_rules":"deprecated_command_only","coverage_basis":"selected_parsed_records_before_operation_type_and_display_limits"})
}

#[cfg(test)]
mod tests {
    #[test]
    fn capabilities_fit_a_windows_sized_stack() {
        let capabilities = std::thread::Builder::new()
            .name("capabilities".into())
            .stack_size(1024 * 1024)
            .spawn(super::capabilities)
            .unwrap()
            .join()
            .unwrap();
        assert!(
            capabilities["commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == "profile-mappings")
        );
        assert!(capabilities["report_schemas"]["profile_mappings"].is_object());
    }
    #[test]
    fn command_names_fit_a_windows_sized_stack() {
        let names = std::thread::Builder::new()
            .name("capability_names".into())
            .stack_size(1024 * 1024)
            .spawn(super::command_names)
            .unwrap()
            .join()
            .unwrap();
        assert!(names.iter().any(|name| name == "prepare-profile"));
    }
    #[test]
    fn capability_report_fits_a_windows_sized_stack() {
        let report = std::thread::Builder::new()
            .name("capability_report".into())
            .stack_size(1024 * 1024)
            .spawn(|| super::capability_report(Vec::new()))
            .unwrap()
            .join()
            .unwrap();
        assert!(report["report_schemas"]["report"].is_object());
        assert_eq!(report["profile_preparation"]["command"], "prepare-profile");
    }
}
