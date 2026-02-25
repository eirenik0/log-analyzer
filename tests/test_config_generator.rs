use chrono::{DateTime, Local};
use log_analyzer::config::{AnalyzerConfig, SessionLevelConfig, SessionsRules};
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
    assert_eq!(generated.sessions.levels.len(), 2);
    assert_eq!(generated.sessions.levels[0].name, "primary");
    assert_eq!(generated.sessions.levels[0].segment_prefix, "manager-");
    assert!(generated.sessions.levels[0].create_command.is_none());
    assert!(generated.sessions.levels[0].complete_commands.is_empty());
    assert_eq!(generated.sessions.levels[1].name, "secondary");
    assert_eq!(generated.sessions.levels[1].segment_prefix, "eyes-");
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
    assert!(generated.sessions.levels.is_empty());
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
    assert_eq!(
        deserialized.sessions.levels.len(),
        generated.sessions.levels.len()
    );
    for (actual, expected) in deserialized
        .sessions
        .levels
        .iter()
        .zip(generated.sessions.levels.iter())
    {
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.segment_prefix, expected.segment_prefix);
        assert_eq!(actual.create_command, expected.create_command);
        assert_eq!(actual.complete_commands, expected.complete_commands);
        assert_eq!(actual.summary_fields, expected.summary_fields);
    }
}

#[test]
fn test_preserves_template_defined_session_levels() {
    let logs = vec![make_entry(
        "core",
        "manager-ufg-1/eyes-ufg-1/check-1",
        LogEntryKind::Generic { payload: None },
    )];

    let mut base = AnalyzerConfig::default();
    base.sessions = SessionsRules {
        levels: vec![
            SessionLevelConfig {
                name: "runner".to_string(),
                segment_prefix: "manager-".to_string(),
                create_command: Some("makeManager".to_string()),
                complete_commands: vec!["getResults".to_string(), "closeBatch".to_string()],
                summary_fields: vec!["concurrency".to_string()],
            },
            SessionLevelConfig {
                name: "test".to_string(),
                segment_prefix: "eyes-".to_string(),
                create_command: Some("openEyes".to_string()),
                complete_commands: vec!["close".to_string(), "abort".to_string()],
                summary_fields: vec![],
            },
        ],
    };

    let generated = generate_config(
        &logs,
        &base,
        &GenerateConfigOptions {
            profile_name: "generated".to_string(),
        },
    );

    assert_eq!(generated.sessions.levels.len(), 2);
    assert_eq!(generated.sessions.levels[0].name, "runner");
    assert_eq!(
        generated.sessions.levels[0].create_command.as_deref(),
        Some("makeManager")
    );
    assert_eq!(
        generated.sessions.levels[0].complete_commands,
        vec!["getResults".to_string(), "closeBatch".to_string()]
    );
    assert_eq!(
        generated.sessions.levels[0].summary_fields,
        vec!["concurrency"]
    );
}
