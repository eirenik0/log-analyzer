use chrono::{DateTime, Local};
use log_analyzer::comparator::LogFilter;
use log_analyzer::config::{
    AnalyzerConfig, analyze_profile, builtin_template_names, default_config, load_builtin_template,
    load_config_from_path,
};
use log_analyzer::parser::{LogEntry, LogEntryKind, RequestDirection, parse_log_entry_with_config};
use log_analyzer::perf_analyzer::analyze_performance_with_config;
use std::path::Path;

#[test]
fn test_parse_with_custom_event_marker() {
    let mut config = AnalyzerConfig::default();
    config.parser.event_emit_markers = vec!["EMIT".to_string()];
    config.parser.event_receive_markers = vec!["RECV".to_string()];
    config.parser.event_payload_separator = "payload".to_string();

    let log_line =
        r#"service-a | 2025-04-03T21:35:06.157Z [INFO ] EMIT "Cache.hit" payload {"key":"abc"}"#;

    let record = parse_log_entry_with_config(log_line, 1, &config).expect("parse should succeed");

    match record.kind {
        LogEntryKind::Event {
            event_type,
            direction,
            payload,
        } => {
            assert_eq!(event_type, "Cache.hit");
            assert_eq!(direction.to_string(), "Emit");
            assert_eq!(
                payload,
                Some(serde_json::json!({
                    "key": "abc"
                }))
            );
        }
        _ => panic!("expected event log"),
    }
}

#[test]
fn test_request_direction_can_be_overridden_by_config() {
    let mut config = AnalyzerConfig::default();
    config.parser.request_send_markers = vec!["queued".to_string()];
    config.parser.request_receive_markers = vec!["done".to_string()];

    let send_line =
        r#"svc | 2025-04-03T21:35:06.157Z [INFO ] Request "fetch" queued with body {"id":1}"#;
    let recv_line = r#"svc | 2025-04-03T21:35:07.157Z [INFO ] Request "fetch" done with body {"statusCode":200}"#;

    let send =
        parse_log_entry_with_config(send_line, 1, &config).expect("send parse should succeed");
    let recv =
        parse_log_entry_with_config(recv_line, 2, &config).expect("recv parse should succeed");

    match send.kind {
        LogEntryKind::Request { direction, .. } => {
            assert_eq!(direction, RequestDirection::Send);
        }
        _ => panic!("expected request log"),
    }

    match recv.kind {
        LogEntryKind::Request { direction, .. } => {
            assert_eq!(direction, RequestDirection::Receive);
        }
        _ => panic!("expected request log"),
    }
}

#[test]
fn test_perf_markers_can_be_overridden_by_config() {
    let mut config = AnalyzerConfig::default();
    config.perf.command_start_markers = vec!["BEGIN".to_string()];
    config.perf.command_completion_markers = vec!["DONE".to_string()];

    let start_time = "2025-04-03T21:35:06.000Z"
        .parse::<DateTime<Local>>()
        .unwrap();
    let end_time = "2025-04-03T21:35:07.000Z"
        .parse::<DateTime<Local>>()
        .unwrap();

    let start = LogEntry {
        component: "svc".to_string(),
        component_id: "session-1".to_string(),
        timestamp: start_time,
        level: "INFO".to_string(),
        message: "Command \"sync\" BEGIN".to_string(),
        raw_logline: "Command \"sync\" BEGIN".to_string(),
        kind: LogEntryKind::Command {
            command: "sync".to_string(),
            settings: None,
        },
        source_line_number: 1,
    };

    let end = LogEntry {
        component: "svc".to_string(),
        component_id: "session-1".to_string(),
        timestamp: end_time,
        level: "INFO".to_string(),
        message: "Command \"sync\" DONE".to_string(),
        raw_logline: "Command \"sync\" DONE".to_string(),
        kind: LogEntryKind::Command {
            command: "sync".to_string(),
            settings: None,
        },
        source_line_number: 2,
    };

    let logs = vec![start, end];
    let results =
        analyze_performance_with_config(&logs, &LogFilter::new(), Some("Command"), &config);

    assert_eq!(results.operations.len(), 1);
    assert_eq!(results.operations[0].name, "sync");
    assert_eq!(results.operations[0].duration_ms, 1000);
}

#[test]
fn test_profile_insights_can_be_configured_without_external_profile_file() {
    let mut config = AnalyzerConfig::default();
    config.profile.known_components = vec!["core".to_string()];
    config.profile.known_commands = vec!["sync".to_string()];
    config.profile.known_requests = vec!["fetchUser".to_string()];
    config.profile.session_prefixes.primary = "trace-".to_string();
    config.profile.session_prefixes.secondary = "span-".to_string();

    let command_entry = LogEntry {
        component: "custom-service".to_string(),
        component_id: "trace-abc/span-xyz/step-1".to_string(),
        timestamp: "2025-04-03T21:35:06.000Z"
            .parse::<DateTime<Local>>()
            .unwrap(),
        level: "INFO".to_string(),
        message: "Command \"customOp\" is called".to_string(),
        raw_logline: "Command \"customOp\" is called".to_string(),
        kind: LogEntryKind::Command {
            command: "customOp".to_string(),
            settings: None,
        },
        source_line_number: 1,
    };

    let request_entry = LogEntry {
        component: "core".to_string(),
        component_id: "trace-def/span-uvw/step-2".to_string(),
        timestamp: "2025-04-03T21:35:07.000Z"
            .parse::<DateTime<Local>>()
            .unwrap(),
        level: "INFO".to_string(),
        message: "Request \"customReq\" will be sent".to_string(),
        raw_logline: "Request \"customReq\" will be sent".to_string(),
        kind: LogEntryKind::Request {
            request: "customReq".to_string(),
            request_id: None,
            endpoint: None,
            direction: RequestDirection::Send,
            payload: None,
        },
        source_line_number: 2,
    };

    let insights = analyze_profile(&[command_entry, request_entry], &config);
    assert!(insights.unknown_components.contains("custom-service"));
    assert!(insights.unknown_commands.contains("customOp"));
    assert!(insights.unknown_requests.contains("customReq"));
    assert!(insights.primary_sessions.contains("trace-abc"));
    assert!(insights.secondary_sessions.contains("span-xyz"));
}

#[test]
fn test_all_profile_templates_are_valid_toml() {
    let files = [
        "config/profiles/base.toml",
        "config/templates/custom-start.toml",
        "config/templates/service-api.toml",
        "config/templates/event-pipeline.toml",
        ".claude/skills/analyze-logs/templates/base.toml",
        ".claude/skills/analyze-logs/templates/custom-start.toml",
        ".claude/skills/analyze-logs/templates/service-api.toml",
        ".claude/skills/analyze-logs/templates/event-pipeline.toml",
    ];

    for file in files {
        let config = load_config_from_path(Path::new(file));
        assert!(config.is_ok(), "failed to parse template: {}", file);
    }
}

#[test]
fn test_default_config_comes_from_embedded_base_toml() {
    let config = default_config();
    assert_eq!(config.profile_name, "base");
    assert_eq!(config.parser.event_payload_separator, "with payload");
    assert_eq!(
        config.perf.command_start_markers,
        vec!["is called".to_string()]
    );
}

#[test]
fn test_builtin_templates_can_be_loaded_from_names_and_paths() {
    let base = load_builtin_template("base").expect("base template should load");
    assert_eq!(base.profile_name, "base");

    let service =
        load_builtin_template("service-api.toml").expect("service-api template should load");
    assert_eq!(service.profile_name, "service-api");

    let event_path = load_builtin_template("config/templates/event-pipeline.toml")
        .expect("event-pipeline template should load from path-like input");
    assert_eq!(event_path.profile_name, "event-pipeline");

    assert!(load_builtin_template("unknown-template").is_none());
    assert_eq!(
        builtin_template_names(),
        &["base", "custom-start", "service-api", "event-pipeline"]
    );
}
