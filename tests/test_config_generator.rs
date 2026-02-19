use chrono::{DateTime, Local};
use log_analyzer::config::AnalyzerConfig;
use log_analyzer::config_generator::{GenerateConfigOptions, generate_config};
use log_analyzer::parser::{LogEntry, LogEntryKind, RequestDirection};

fn test_timestamp() -> DateTime<Local> {
    "2025-04-03T21:35:06.000Z"
        .parse::<DateTime<Local>>()
        .expect("valid RFC3339 timestamp")
}

fn make_entry(component: &str, component_id: &str, kind: LogEntryKind) -> LogEntry {
    LogEntry {
        component: component.to_string(),
        component_id: component_id.to_string(),
        timestamp: test_timestamp(),
        level: "INFO".to_string(),
        message: "message".to_string(),
        raw_logline: "raw".to_string(),
        kind,
        source_line_number: 1,
    }
}

#[test]
fn test_collects_unique_components() {
    let logs = vec![
        make_entry(
            "core-universal",
            "",
            LogEntryKind::Generic { payload: None },
        ),
        make_entry("socket", "", LogEntryKind::Generic { payload: None }),
        make_entry(
            "core-universal",
            "",
            LogEntryKind::Generic { payload: None },
        ),
    ];

    let generated = generate_config(
        &logs,
        &AnalyzerConfig::default(),
        &GenerateConfigOptions {
            profile_name: "generated".to_string(),
        },
    );

    assert_eq!(
        generated.profile.known_components,
        vec!["core-universal".to_string(), "socket".to_string()]
    );
}

#[test]
fn test_collects_commands_and_requests() {
    let logs = vec![
        make_entry(
            "core",
            "",
            LogEntryKind::Command {
                command: "openEyes".to_string(),
                settings: None,
            },
        ),
        make_entry(
            "core",
            "",
            LogEntryKind::Command {
                command: "openEyes".to_string(),
                settings: None,
            },
        ),
        make_entry(
            "core",
            "",
            LogEntryKind::Command {
                command: "closeEyes".to_string(),
                settings: None,
            },
        ),
        make_entry(
            "core",
            "",
            LogEntryKind::Request {
                request: "render".to_string(),
                request_id: None,
                endpoint: None,
                direction: RequestDirection::Send,
                payload: None,
            },
        ),
        make_entry(
            "core",
            "",
            LogEntryKind::Request {
                request: "startSession".to_string(),
                request_id: None,
                endpoint: None,
                direction: RequestDirection::Receive,
                payload: None,
            },
        ),
    ];

    let generated = generate_config(
        &logs,
        &AnalyzerConfig::default(),
        &GenerateConfigOptions {
            profile_name: "generated".to_string(),
        },
    );

    assert_eq!(
        generated.profile.known_commands,
        vec!["closeEyes".to_string(), "openEyes".to_string()]
    );
    assert_eq!(
        generated.profile.known_requests,
        vec!["render".to_string(), "startSession".to_string()]
    );
}

#[test]
fn test_detects_session_prefixes() {
    let logs = vec![
        make_entry(
            "core",
            "manager-ufg-1/eyes-ufg-1/check-1",
            LogEntryKind::Generic { payload: None },
        ),
        make_entry(
            "core",
            "manager-ufg-2/eyes-ufg-2/render-1",
            LogEntryKind::Generic { payload: None },
        ),
        make_entry(
            "core",
            "manager-ufg-3/task-1",
            LogEntryKind::Generic { payload: None },
        ),
    ];

    let generated = generate_config(
        &logs,
        &AnalyzerConfig::default(),
        &GenerateConfigOptions {
            profile_name: "generated".to_string(),
        },
    );

    assert_eq!(generated.profile.session_prefixes.primary, "manager-");
    assert_eq!(generated.profile.session_prefixes.secondary, "eyes-");
}

#[test]
fn test_empty_component_ids_yield_empty_prefixes() {
    let logs = vec![
        make_entry("core", "", LogEntryKind::Generic { payload: None }),
        make_entry(
            "core",
            "withoutdash/alsowithoutdash",
            LogEntryKind::Generic { payload: None },
        ),
    ];

    let generated = generate_config(
        &logs,
        &AnalyzerConfig::default(),
        &GenerateConfigOptions {
            profile_name: "generated".to_string(),
        },
    );

    assert!(generated.profile.session_prefixes.primary.is_empty());
    assert!(generated.profile.session_prefixes.secondary.is_empty());
}

#[test]
fn test_inherits_parser_rules_from_base() {
    let mut base = AnalyzerConfig::default();
    base.parser.event_emit_markers = vec!["EMIT_CUSTOM".to_string()];
    base.perf.command_start_markers = vec!["BEGIN_CUSTOM".to_string()];
    base.profile.known_components = vec!["legacy-component".to_string()];

    let logs = vec![make_entry(
        "new-component",
        "",
        LogEntryKind::Generic { payload: None },
    )];

    let generated = generate_config(
        &logs,
        &base,
        &GenerateConfigOptions {
            profile_name: "generated".to_string(),
        },
    );

    assert_eq!(
        generated.parser.event_emit_markers,
        vec!["EMIT_CUSTOM".to_string()]
    );
    assert_eq!(
        generated.perf.command_start_markers,
        vec!["BEGIN_CUSTOM".to_string()]
    );
    assert_eq!(
        generated.profile.known_components,
        vec!["new-component".to_string()]
    );
}

#[test]
fn test_serialization_roundtrip() {
    let logs = vec![
        make_entry(
            "core",
            "manager-ufg-1/eyes-ufg-1",
            LogEntryKind::Command {
                command: "openEyes".to_string(),
                settings: None,
            },
        ),
        make_entry(
            "core",
            "manager-ufg-2/eyes-ufg-2",
            LogEntryKind::Request {
                request: "render".to_string(),
                request_id: None,
                endpoint: None,
                direction: RequestDirection::Send,
                payload: None,
            },
        ),
    ];

    let generated = generate_config(
        &logs,
        &AnalyzerConfig::default(),
        &GenerateConfigOptions {
            profile_name: "roundtrip-profile".to_string(),
        },
    );

    let serialized = toml::to_string_pretty(&generated).expect("serialize config");
    let deserialized: AnalyzerConfig = toml::from_str(&serialized).expect("deserialize config");

    assert_eq!(deserialized.profile_name, generated.profile_name);
    assert_eq!(
        deserialized.parser.command_prefix,
        generated.parser.command_prefix
    );
    assert_eq!(
        deserialized.perf.command_completion_markers,
        generated.perf.command_completion_markers
    );
    assert_eq!(
        deserialized.profile.known_components,
        generated.profile.known_components
    );
    assert_eq!(
        deserialized.profile.known_commands,
        generated.profile.known_commands
    );
    assert_eq!(
        deserialized.profile.known_requests,
        generated.profile.known_requests
    );
    assert_eq!(
        deserialized.profile.session_prefixes.primary,
        generated.profile.session_prefixes.primary
    );
    assert_eq!(
        deserialized.profile.session_prefixes.secondary,
        generated.profile.session_prefixes.secondary
    );
}
