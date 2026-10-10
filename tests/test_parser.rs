// Assume the parser is implemented in parser.rs with a function:
//   fn parse_log_entry(line: &str) -> Result<LogRecord, ParseError>
// and a LogRecord struct with fields such as component, timestamp, level, message, etc.

use chrono::{DateTime, Local};
use log_analyzer::config::{AnalyzerConfig, load_builtin_template};
use log_analyzer::parser::{
    LogEntryKind, RequestDirection, parse_log_entry, parse_log_entry_with_config,
    parse_log_file_with_config,
};
use serde_json::json;
use std::fs;
use tempfile::tempdir;

fn eyes_config() -> AnalyzerConfig {
    load_builtin_template("eyes").expect("eyes preset should load")
}

// Test for a core-universal initialization log entry.
#[test]
fn test_parse_core_universal_initialization() {
    let log_line = r#"core-universal | 2025-04-03T21:35:06.108Z [INFO ] Core universal is going to be initialized with options {
  debug: false,
  shutdownMode: 'stdin',
  idleTimeout: 900000,
  printStdout: false,
  defaultEnvironment: undefined,
  _: [ 'universal' ],
  singleton: false,
  'shutdown-mode': 'stdin',
  shutdown: 'stdin',
  port: 21077,
  fork: false,
  'port-resolution-mode': 'next',
  'port-resolution': 'next',
  portResolution: 'next',
  portResolutionMode: 'next',
  'idle-timeout': 900000,
  'mask-log': false,
  '$0': '../core_universal/apts/core_universal/bin/core'
}"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse core-universal initialization log");

    assert_eq!(record.component, "core-universal");
    assert_eq!(
        record.timestamp,
        "2025-04-03T21:35:06.108Z"
            .parse::<DateTime<Local>>()
            .unwrap()
    );
    assert_eq!(record.level, "INFO");
    assert_eq!(
        record.payload(),
        Some(&serde_json::json!({
          "debug": false,
          "shutdownMode": "stdin",
          "idleTimeout": 900000,
          "printStdout": false,
          "defaultEnvironment": null,
          "_": [ "universal" ],
          "singleton": false,
          "shutdown-mode": "stdin",
          "shutdown": "stdin",
          "port": 21077,
          "fork": false,
          "port-resolution-mode": "next",
          "port-resolution": "next",
          "portResolution": "next",
          "portResolutionMode": "next",
          "idle-timeout": 900000,
          "mask-log": false,
          "$0": "../core_universal/apts/core_universal/bin/core"
        }
                ))
    );
}

// Test for a socket log emitting an event.
#[test]
fn test_parse_socket_emit_event() {
    let log_line = r#"socket | 2025-04-03T21:35:06.157Z [INFO ] Emit event of type "Logger.log" with payload {
    "level": "info",
    "message": "Logs saved in: /Users/eiro/sdk/logs"
}"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse socket emit event log");

    // Assert component and level.
    assert_eq!(record.component, "socket");
    assert_eq!(record.level, "INFO");
    // Optionally check that the event type and payload are parsed from the message.
    // For example, if record.event_type is available:
    // assert_eq!(record.event_type, "Logger.log");
}

// Test for a socket log receiving an event.
#[test]
fn test_parse_socket_received_event() {
    let log_line = r#"socket | 2025-04-03T21:35:06.163Z [INFO ] Received event of type {"name":"Core.makeCore"} with payload {
    "agentId": "eyes.sdk.python/6.1.0",
    "cwd": "/Users/eiro/sdk/logs/python/tests",
    "environment": {
        "versions": {
            "appium-python-client": "3.2.1",
            "eyes-common": "6.1.0",
            "eyes-images": "6.1.0",
            "eyes-playwright": "6.1.0",
            "eyes-robotframework": "6.1.0",
            "eyes-selenium": "6.1.0",
            "robotframework": "7.2.2",
            "robotframework-appiumlibrary": "2.1.0",
            "robotframework-seleniumlibrary": "6.7.0",
            "selenium": "4.16.0",
            "python": "3.12.3"
        },
        "sdk": {
            "lang": "python",
            "name": "eyes-selenium",
            "currentVersion": "6.1.0"
        }
    },
    "spec": "webdriver"
}"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse received event log");

    // Validate basic fields.
    assert_eq!(record.component, "socket");
    assert_eq!(record.level, "INFO");
    // Additional assertions should validate the JSON event type and payload if your parser extracts them.
}

// Test for a driver log related to switching context.
#[test]
fn test_parse_driver_switch_context() {
    let log_line = r#"driver (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx) | 2025-04-03T21:35:14.042Z [INFO ] Switching to a child context with depth: 0"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse driver context switch log");

    // Assert that the component and message contain expected keywords.
    assert_eq!(record.component, "driver");
    assert_eq!(
        record.component_id,
        "manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx"
    );
    assert!(record.message.contains("Switching to a child context"));
}

// Test for a core-ufg log taking a DOM snapshot.
#[test]
fn test_parse_dom_snapshot_log() {
    let log_line = r#"core-ufg (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx) | 2025-04-03T21:35:15.301Z [INFO ] Taking dom snapshot for viewport size [object Object]"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse DOM snapshot log");

    // Validate that the log message indicates a DOM snapshot.
    assert_eq!(record.component, "core-ufg");
    assert_eq!(
        record.component_id,
        "manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx"
    );
    assert!(
        record
            .message
            .contains("Taking dom snapshot for viewport size")
    );
}

// Test for a core-requests log for the "openEyes" request.
#[test]
fn test_parse_open_eyes_request() {
    let log_line = r#"core-requests (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/eyes-base-htm/core-request-bdg) | 2025-04-03T21:35:29.392Z [INFO ] Request "openEyes" [0--e6f57eb8-a8a0-4d1f-985b-9de36025ce90] will be sent to the address "[POST]https://eyesapi.apts.com/api/sessions/running" with body {"startInfo":{ ... }}"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse openEyes request log");

    // Assert that the log has been parsed with correct component and request information.
    assert_eq!(record.component, "core-requests");
    assert_eq!(
        record.component_id,
        "manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/eyes-base-htm/core-request-bdg"
    );
    // If your parser extracts the request name:
    // assert_eq!(record.request_name, "openEyes");
}

// Test for a ufg-requests log for the "startRenders" event.
#[test]
fn test_parse_start_renders() {
    let log_line = r#"ufg-requests (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/render-t7j/start-render-request-cly) | 2025-04-03T21:35:32.628Z [INFO ] Request "startRenders" finished successfully with body [
  {
    jobId: '54db7691-c742-49e5-a4dd-a2db1f4377b9',
    renderId: 'c1cc643b-b811-49f2-a4d6-e0b252fb6924',
    status: 'rendering',
    needMoreResources: undefined,
    needMoreDom: undefined
  }
]"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse startRenders log");

    // Check that the component is correct and the message mentions startRenders.
    assert_eq!(record.component, "ufg-requests");
    assert_eq!(
        record.component_id,
        "manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/render-t7j/start-render-request-cly"
    );
    match record.kind {
        LogEntryKind::Request {
            request,
            payload,
            direction,
            ..
        } => {
            assert_eq!(request, "startRenders");
            assert_eq!(direction, RequestDirection::Receive);
            assert_eq!(
                payload,
                Some(json!( [
                  {
                    "jobId": "54db7691-c742-49e5-a4dd-a2db1f4377b9",
                    "renderId": "c1cc643b-b811-49f2-a4d6-e0b252fb6924",
                    "status": "rendering",
                    "needMoreResources": null,
                    "needMoreDom": null
                  }
                ]))
            )
        }
        _ => panic!("Wrong kind of log entry"),
    }
}
// Test for a ufg-requests log for the "getActualEnvironments" event.
#[test]
fn test_parse_with_request() {
    let log_line = r#"ufg-requests (manager-ufg-hoh/eyes-ufg-aif/check-ufg-ebh/environment-lrd/get-actual-environment-4bu/get-actual-environments-g55 & manager-ufg-hoh/eyes-ufg-aif/check-ufg-ebh/environment-g6p/get-actual-environment-fpc/get-actual-environments-g55) | 2025-04-03T21:08:12.795Z [INFO ] Request "getActualEnvironments" [0--1af9f42c-67ff-48c9-b1f8-09ee02017cdb] will be sent to the address "[POST]https://ufg-wus.apts.com/job-info" with body [{"agentId":"eyes-universal/4.33.0/eyes.visualgrid.ruby/6.6.1 [eyes.selenium.visualgrid.ruby/6.6.1]","webhook":"","stitchingService":"","platform":{"name":"linux","type":"web"},"browser":{"name":"chrome"},"renderInfo":{"width":400,"height":800,"target":"viewport"}},{"agentId":"eyes-universal/4.33.0/eyes.visualgrid.ruby/6.6.1 [eyes.selenium.visualgrid.ruby/6.6.1]","webhook":"","stitchingService":"","platform":{"name":"linux","type":"web"},"browser":{"name":"chrome"},"renderInfo":{"width":1000,"height":800,"target":"viewport"}}]"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse getActualEnvironments log");

    // Check that the component is correct and the message mentions getActualEnvironments.
    assert_eq!(record.component, "ufg-requests");

    match record.kind {
        LogEntryKind::Request {
            request,
            request_id,
            payload,
            direction,
            ..
        } => {
            assert_eq!(request, "getActualEnvironments");
            assert_eq!(
                request_id,
                Some("0--1af9f42c-67ff-48c9-b1f8-09ee02017cdb".to_string()),
            );
            assert_eq!(
                payload,
                Some(
                    json!([{"agentId":"eyes-universal/4.33.0/eyes.visualgrid.ruby/6.6.1 [eyes.selenium.visualgrid.ruby/6.6.1]","webhook":"","stitchingService":"","platform":{"name":"linux","type":"web"},"browser":{"name":"chrome"},"renderInfo":{"width":400,"height":800,"target":"viewport"}},{"agentId":"eyes-universal/4.33.0/eyes.visualgrid.ruby/6.6.1 [eyes.selenium.visualgrid.ruby/6.6.1]","webhook":"","stitchingService":"","platform":{"name":"linux","type":"web"},"browser":{"name":"chrome"},"renderInfo":{"width":1000,"height":800,"target":"viewport"}}])
                )
            );
            assert_eq!(direction, RequestDirection::Send)
        }
        _ => panic!("Wrong kind of log entry"),
    }
}
// Test for a ufg-requests log for the "getActualEnvironments" event.
#[test]
fn test_parse_with_request2() {
    let log_line = r#"core-requests (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/eyes-base-htm/core-request-bdg) | 2025-04-03T21:35:29.392Z [INFO ] Request "openEyes" [0--e6f57eb8-a8a0-4d1f-985b-9de36025ce90] will be sent to the address "[POST]https://eyesapi.apts.com/api/sessions/running" with body {"startInfo":{"agentId":"eyes-universal/4.35.0/eyes.selenium.visualgrid.python/6.1.0","agentSessionId":"CheckWindowWithReloadLayoutBreakpoints--6894fe00-2c2b-4f39-b9b8-a309bc6b2359","agentRunId":"CheckWindowWithReloadLayoutBreakpoints--6894fe00-2c2b-4f39-b9b8-a309bc6b2359","appIdOrName":"Applitools Eyes SDK","scenarioIdOrName":"CheckWindowWithReloadLayoutBreakpoints","properties":[{"name":"browserVersion","value":"135.0.7049.52"}],"batchInfo":{"id":"6e8afcf5-bc7a-406a-9104-728d710183d5","name":"Py3.12|Sel4.15.2 Generated tests","startedAt":"2025-04-03T21:35:04Z"},"egSessionId":"f03c5a9b-dbad-4d04-8c65-d1abf3300f7a","environment":{"ufgJobType":"web","inferred":"useragent:Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/135.0.0.0 Safari/537.36","deviceInfo":"Desktop","displaySize":{"width":400,"height":800},"0.sg1fmhj9ufh":"got you!"},"branchName":"master","parentBranchName":"master","compareWithParentBranch":false,"ignoreBaseline":false,"latestCommitInfo":{"sha":"32dba3b1ba58911956b430911eeb7624e51cad66","timestamp":"2025-04-03T21:37:57+02:00"},"processId":"056b3f40-e104-4df2-b3df-5baefcbc35b9"}}"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse openEyes log");

    // Check that the component is correct and the message mentions getActualEnvironments.
    assert_eq!(record.component, "core-requests");

    match record.kind {
        LogEntryKind::Request {
            request,
            request_id,
            payload,
            direction,
            ..
        } => {
            assert_eq!(request, "openEyes");
            assert_eq!(
                request_id,
                Some("0--e6f57eb8-a8a0-4d1f-985b-9de36025ce90".to_string())
            );
            assert_eq!(
                payload,
                Some(
                    json!({"startInfo":{"agentId":"eyes-universal/4.35.0/eyes.selenium.visualgrid.python/6.1.0","agentSessionId":"CheckWindowWithReloadLayoutBreakpoints--6894fe00-2c2b-4f39-b9b8-a309bc6b2359","agentRunId":"CheckWindowWithReloadLayoutBreakpoints--6894fe00-2c2b-4f39-b9b8-a309bc6b2359","appIdOrName":"Applitools Eyes SDK","scenarioIdOrName":"CheckWindowWithReloadLayoutBreakpoints","properties":[{"name":"browserVersion","value":"135.0.7049.52"}],"batchInfo":{"id":"6e8afcf5-bc7a-406a-9104-728d710183d5","name":"Py3.12|Sel4.15.2 Generated tests","startedAt":"2025-04-03T21:35:04Z"},"egSessionId":"f03c5a9b-dbad-4d04-8c65-d1abf3300f7a","environment":{"ufgJobType":"web","inferred":"useragent:Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/135.0.0.0 Safari/537.36","deviceInfo":"Desktop","displaySize":{"width":400,"height":800},"0.sg1fmhj9ufh":"got you!"},"branchName":"master","parentBranchName":"master","compareWithParentBranch":false,"ignoreBaseline":false,"latestCommitInfo":{"sha":"32dba3b1ba58911956b430911eeb7624e51cad66","timestamp":"2025-04-03T21:37:57+02:00"},"processId":"056b3f40-e104-4df2-b3df-5baefcbc35b9"}})
                )
            );
            assert_eq!(direction, RequestDirection::Send)
        }
        _ => panic!("Wrong kind of log entry"),
    }
}

// Test for a ufg-requests log for the "getActualEnvironments" event.
#[test]
fn test_parse_command_with_settings() {
    let log_line = r#"core-base (manager-ufg-hoh/eyes-ufg-aif/close-p78/check-ufg-ebh/environment-lrd/eyes-base-e8f/close-base-5wk) | 2025-04-03T21:08:25.197Z [INFO ] Command "close" is called with settings {
  updateBaselineIfNew: false,
  testMetadata: undefined,
  environments: undefined
}"#;
    let record = parse_log_entry_with_config(log_line, 1, &eyes_config())
        .expect("Failed to parse openEyes log");

    assert_eq!(record.component, "core-base");

    match record.kind {
        LogEntryKind::Command { command, .. } => {
            assert_eq!(command, "close".to_string());
        }
        _ => panic!("Wrong kind of log entry"),
    }
}

#[test]
fn test_parse_rust_tracing_log_with_structured_fields() {
    let log_line = concat!(
        "2026-03-10T08:15:30.123Z INFO ",
        "fluxomni_server::integrations::srs::callback: received callback ",
        "trace_id=abc123 span_id=def456 actor_kind=switch restream_name=demo-stream",
    );

    let record = parse_log_entry(log_line, 1).expect("Failed to parse rust tracing log");

    assert_eq!(record.component, "srs::callback");
    assert_eq!(
        record.module_path.as_deref(),
        Some("fluxomni_server::integrations::srs::callback")
    );
    assert_eq!(record.message, "received callback");
    assert_eq!(record.structured_field("trace_id"), Some("abc123"));
    assert_eq!(
        record.structured_field("restream_name"),
        Some("demo-stream")
    );
    assert!(matches!(record.kind, LogEntryKind::Generic { .. }));
}

#[test]
fn test_parse_rust_tracing_file_with_multiline_entries() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("flux.log");
    fs::write(
        &file,
        concat!(
            "2026-03-10T08:15:30.123Z INFO fluxomni_server::ffmpeg::runner: launching ffmpeg command trace_id=abc123\n",
            "ffmpeg -i input.ts -c:v copy output.ts\n",
            "2026-03-10T08:15:31.123Z ERROR fluxomni_server::ffmpeg::runner: ffmpeg failed trace_id=abc123 exit_code=251\n",
        ),
    )
    .expect("write log file");

    let logs = parse_log_file_with_config(&file, &eyes_config())
        .expect("Failed to parse rust tracing file");

    assert_eq!(logs.len(), 2);
    assert!(logs[0].message.contains("launching ffmpeg command"));
    assert!(
        logs[0]
            .message
            .contains("ffmpeg -i input.ts -c:v copy output.ts")
    );
    assert_eq!(logs[1].structured_field("exit_code"), Some("251"));
}

#[test]
fn test_long_tracing_prose_with_and_without_structured_suffix() {
    let message = "synthetic café 界🙂 message ".repeat(2000);
    let prefix = "2026-01-01T00:00:00Z INFO app::worker: ";
    let plain = parse_log_entry(&format!("{prefix}{message}"), 1).unwrap();
    assert_eq!(plain.message, message.trim_end());
    assert!(plain.structured_fields.is_empty());

    let with_fields = parse_log_entry(
        &format!("{prefix}{message}trace_id \t=abc123 nested={{\"ok\":true}}"),
        2,
    )
    .unwrap();
    assert_eq!(with_fields.message, message.trim_end());
    assert_eq!(with_fields.structured_field("trace_id"), Some("abc123"));
    assert_eq!(
        with_fields.structured_field("nested"),
        Some(r#"{"ok":true}"#)
    );
}

#[test]
fn test_tracing_field_boundaries_keep_existing_whitespace_and_prose_rules() {
    for (input, message, field) in [
        ("hello trace_id=abc", "hello", Some("abc")),
        ("hello trace_id\u{2003}=abc", "hello", Some("abc")),
        ("hello trace id=abc", "hello trace", None),
        ("hello trace_id= abc", "hello trace_id= abc", None),
        (
            "hello trace_id=abc trailing prose",
            "hello trace_id=abc trailing prose",
            None,
        ),
        ("hello trace_id=\"café 界🙂\"", "hello", Some("café 界🙂")),
    ] {
        let entry = parse_log_entry(
            &format!("2026-01-01T00:00:00Z INFO app::worker: {input}"),
            1,
        )
        .unwrap();
        assert_eq!(entry.message, message, "{input}");
        assert_eq!(entry.structured_field("trace_id"), field, "{input}");
    }
}

#[test]
fn test_parse_syslog_line() {
    let log_line =
        "2026-03-10T08:15:30Z host-a stream-manager[4221]: ERROR failed to restart pipeline";
    let record = parse_log_entry(log_line, 1).expect("Failed to parse syslog line");

    assert_eq!(record.component, "stream-manager");
    assert_eq!(record.component_id, "4221");
    assert_eq!(record.level, "ERROR");
    assert_eq!(record.structured_field("host"), Some("host-a"));
    assert_eq!(record.message, "ERROR failed to restart pipeline");
}

#[test]
fn test_parse_json_line() {
    let log_line = r#"{"timestamp":"2026-03-10T08:15:30Z","level":"warn","target":"fluxomni_server::workers::restream","message":"worker stalled","trace_id":"abc123","payload":{"attempt":2}}"#;
    let record = parse_log_entry(log_line, 1).expect("Failed to parse JSON log line");

    assert_eq!(record.component, "workers::restream");
    assert_eq!(
        record.module_path.as_deref(),
        Some("fluxomni_server::workers::restream")
    );
    assert_eq!(record.level, "WARN");
    assert_eq!(record.structured_field("trace_id"), Some("abc123"));
    assert_eq!(record.payload(), Some(&json!({"attempt": 2})));
}

#[test]
fn test_default_base_profile_keeps_specialized_eyes_message_generic() {
    let log_line = r#"svc | 2026-01-01T00:00:00.000Z [INFO ] Request "foo" [0--id1] will be sent with body {"x":1}"#;
    let record = parse_log_entry(log_line, 1).expect("Failed to parse generic base line");

    assert!(matches!(record.kind, LogEntryKind::Generic { .. }));
    assert_eq!(record.payload(), Some(&json!({ "x": 1 })));
}

#[test]
fn test_parse_coverage_counts_candidates_without_counting_continuations() {
    use log_analyzer::parser::{ParseError, parse_log_file_report};
    let dir = tempdir().unwrap();
    let file = dir.path().join("coverage.log");
    let content = concat!(
        "worker | 2026-10-07T10:00:00.000Z [INFO ] started\n",
        "{\n  \"payload\": 1\n}\n",
        "    at frame1\n",
        "worker | 2026-99-07T10:00:00.000Z [ERROR] invalid timestamp\n",
        "    at frame2\n    at frame3\n",
    );
    fs::write(&file, content).unwrap();
    let report = parse_log_file_report(&file, &AnalyzerConfig::default()).unwrap();
    assert_eq!(report.coverage.input_bytes, content.len() as u64);
    assert_eq!(report.coverage.parsed_entries, 1);
    assert_eq!(report.coverage.rejected_candidates, 1);
    assert_eq!(report.entries[0].source_line_number, 1);
    assert!(report.entries[0].raw_logline.contains("frame1"));
    fs::write(&file, "unsupported record\n    at frame\n").unwrap();
    assert!(matches!(
        parse_log_file_with_config(&file, &AnalyzerConfig::default()),
        Err(ParseError::NoRecognizedEntries(_))
    ));
}

#[test]
fn test_json_lines_coverage_includes_malformed_and_missing_timestamp_candidates() {
    use log_analyzer::config::LogFormat;
    use log_analyzer::parser::parse_log_file_report;
    let dir = tempdir().unwrap();
    let file = dir.path().join("coverage.jsonl");
    fs::write(
        &file,
        concat!(
            "{\"timestamp\":\"2026-10-07T10:00:00Z\",\"message\":\"ok\"}\n",
            "{broken json\n",
            "{\"message\":\"missing timestamp\"}\n",
            "\n",
        ),
    )
    .unwrap();
    let mut config = AnalyzerConfig::default();
    config.parser.format = LogFormat::JsonLines;
    let report = parse_log_file_report(&file, &config).unwrap();
    assert_eq!(report.coverage.parsed_entries, 1);
    assert_eq!(report.coverage.rejected_candidates, 2);
    assert_eq!(report.coverage.nonempty_lines, 3);
}

#[test]
fn test_browser_console_classic_entries_preserve_source_and_raw_text() {
    use log_analyzer::config::LogFormat;
    use log_analyzer::parser::parse_log_file_report;
    let dir = tempdir().unwrap();
    let file = dir.path().join("console.log");
    let content = concat!(
        "\n",
        "background.js:123 worker | 2026-10-07T10:00:00.000Z [INFO ] started\n",
        "background.js:124 worker | 2026-10-07T10:00:01.000Z [ERROR] example failure\n",
    );
    fs::write(&file, content).unwrap();
    for format in [LogFormat::Auto, LogFormat::Classic] {
        let mut config = AnalyzerConfig::default();
        config.parser.format = format;
        let report = parse_log_file_report(&file, &config).unwrap();
        assert_eq!(report.coverage.selected_parser, LogFormat::Classic);
        assert_eq!(report.coverage.parsed_entries, 2);
        assert_eq!(report.coverage.rejected_candidates, 0);
        for (index, entry) in report.entries.iter().enumerate() {
            assert_eq!(entry.component, "worker");
            assert_eq!(entry.component_id, "");
            assert_eq!(entry.source_line_number, index + 2);
            assert_eq!(entry.timestamp.timestamp(), 1791367200 + index as i64);
            assert_eq!(
                entry.structured_field("console_source"),
                Some(if index == 0 {
                    "background.js:123"
                } else {
                    "background.js:124"
                })
            );
            assert_eq!(entry.raw_logline, content.lines().nth(index + 1).unwrap());
        }
        assert_eq!(report.entries[0].level, "INFO");
        assert_eq!(report.entries[0].message, "started");
        assert_eq!(report.entries[1].level, "ERROR");
        assert_eq!(report.entries[1].message, "example failure");
    }
}

#[test]
fn test_browser_console_multiline_payloads_and_continuations() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("console.log");
    let content = concat!(
        "https://example.test/assets/background.js:123:8 worker (session-1) | 2026-10-07T10:00:00Z [INFO ] payload {\n",
        "https://example.test/assets/background.js:123:8   \"answer\": 42\n",
        "https://example.test/assets/background.js:123:8 }\n",
        "./assets/background.js:124:2 worker | 2026-10-07T10:00:01Z [ERROR] failed\n",
        "./assets/background.js:124:2     at frame1\n",
        "arbitrary continuation background.js:125 worker | 2026-10-07T10:00:02Z [ERROR] embedded text\n",
        "background.js:126 ordinary continuation text\n",
        "quoted worker | 2026-10-07T10:00:02Z [ERROR] continuation example\n",
        "worker | 2026-10-07T10:00:03Z [INFO ] ordinary log\n",
    );
    fs::write(&file, content).unwrap();
    let entries = parse_log_file_with_config(&file, &AnalyzerConfig::default()).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].payload(), Some(&json!({"answer": 42})));
    assert_eq!(entries[0].component_id, "session-1");
    assert!(
        entries[0]
            .raw_logline
            .contains("background.js:123:8   \"answer\"")
    );
    assert_eq!(entries[1].source_line_number, 4);
    assert!(entries[1].message.contains("at frame1"));
    assert!(entries[1].message.contains("embedded text"));
    assert!(entries[1].message.contains("ordinary continuation text"));
    assert!(entries[1].message.contains("continuation example"));
    assert_eq!(entries[2].source_line_number, 9);
    assert_eq!(entries[2].message, "ordinary log");
    assert_eq!(entries[2].structured_field("console_source"), None);

    // Single-entry callers retain the existing permissive component parsing.
    let ordinary = parse_log_entry("worker.api | 2026-10-07T10:00:00Z [INFO ] started", 1).unwrap();
    assert_eq!(ordinary.component, "worker.api");
    assert_eq!(ordinary.message, "started");

    let direct = parse_log_entry(content.lines().next().unwrap(), 17).unwrap();
    assert_eq!(direct.source_line_number, 17);
    assert_eq!(
        direct.structured_field("console_source"),
        Some("https://example.test/assets/background.js:123:8")
    );
}

#[test]
fn classic_slash_components_and_controls_preserve_record_identity() {
    use log_analyzer::config::LogFormat;
    use log_analyzer::parser::parse_log_file_report;
    let dir = tempdir().unwrap();
    let file = dir.path().join("slash.log");
    for component in ["worker/io", "worker-io", "服务/输入"] {
        for prefix in ["", "bundle.js:12:3 "] {
            let content = format!(
                "worker (run-1) | 2026-01-01T00:00:00.000Z [INFO ] started\n{prefix}{component} (run-1) | 2026-01-01T00:00:01.000+02:00 [ERROR] processing item\nworker (run-1) | 2026-01-01T00:00:02.000Z [INFO ] finished\n"
            );
            fs::write(&file, &content).unwrap();
            for format in [LogFormat::Auto, LogFormat::Classic] {
                let mut config = AnalyzerConfig::default();
                config.parser.format = format;
                let report = parse_log_file_report(&file, &config).unwrap();
                assert_eq!(report.coverage.selected_parser, LogFormat::Classic);
                assert_eq!(report.coverage.parsed_entries, 3, "{component} {prefix}");
                assert_eq!(report.coverage.rejected_candidates, 0);
                let middle = &report.entries[1];
                assert_eq!(middle.source_line_number, 2);
                assert_eq!(middle.component, component);
                assert_eq!(middle.component_id, "run-1");
                assert_eq!(middle.level, "ERROR");
                assert_eq!(middle.message, "processing item");
                assert_eq!(
                    middle.source_timestamp.unwrap().offset().local_minus_utc(),
                    7200
                );
                assert_eq!(middle.raw_logline, content.lines().nth(1).unwrap());
                assert!(!report.entries[0].raw_logline.contains("processing item"));
            }
        }
    }
}

#[test]
fn classic_punctuated_candidates_reject_without_swallowing_multiline_records() {
    use log_analyzer::parser::parse_log_file_report;
    let dir = tempdir().unwrap();
    let file = dir.path().join("candidates.log");
    let content = concat!(
        "worker/io (run-1) | 2026-01-01T00:00:00.000Z [INFO ] payload {\n",
        "  \"path\": \"worker@io | ordinary continuation 🦀\"\n",
        "}\n",
        "    at worker@io (traceback)\n",
        "worker/io (run-1) | 2026-99-01T00:00:01.000Z [ERROR] invalid timestamp\n",
        "    at rejected frame\n",
        "worker@io (run-1) | 2026-01-01T00:00:02.000Z [ERROR] unsupported component\n",
        "bundle.js:12 worker@io (run-1) | 2026-01-01T00:00:03.000Z [ERROR] unsupported prefixed component\n",
        "worker (run-1) | 2026-01-01T00:00:04.000Z [INFO ] finished\n",
    );
    fs::write(&file, content).unwrap();
    let report = parse_log_file_report(&file, &AnalyzerConfig::default()).unwrap();
    assert_eq!(report.coverage.parsed_entries, 2);
    assert_eq!(report.coverage.rejected_candidates, 3);
    assert_eq!(report.entries[0].source_line_number, 1);
    assert_eq!(report.entries[1].source_line_number, 9);
    assert_eq!(
        report.entries[0].payload(),
        Some(&json!({"path":"worker@io | ordinary continuation 🦀"}))
    );
    assert!(report.entries[0].raw_logline.contains("at worker@io"));
    assert!(!report.entries[0].raw_logline.contains("invalid timestamp"));
    assert_eq!(report.entries[1].message, "finished");
}
