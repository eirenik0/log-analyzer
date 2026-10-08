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
    json!({"schema_version":SCHEMA_VERSION,"build":identity(),"commands":commands,"report_schemas":{"report":"schemas/report.schema.json","capabilities":"schemas/capabilities.schema.json","investigation":"schemas/investigation.schema.json","evidence_contract_version":crate::evidence::CONTRACT_VERSION},"output_formats":formats,"parser_formats":parsers,"presets":crate::config::builtin_template_names(),"event_classification":{"schema_versions":[1,2],"kinds":["command","request","event"],"adapters":["text","structured"],"legacy_marker_compatibility":true,"command_rules":"deprecated_command_only","coverage_basis":"selected_parsed_records_before_operation_type_and_display_limits"}})
}
