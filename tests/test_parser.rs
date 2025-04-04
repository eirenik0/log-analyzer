// Assume the parser is implemented in parser.rs with a function:
//   fn parse_log_entry(line: &str) -> Result<LogRecord, ParseError>
// and a LogRecord struct with fields such as component, timestamp, level, message, etc.

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Local};
    use log_analyzer::parser::{LogEntryKind, RequestDirection, parse_log_entry};
    use serde_json::json;

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
  '$0': '../core_universal/applitools/core_universal/bin/core'
}"#;
        let record =
            parse_log_entry(log_line).expect("Failed to parse core-universal initialization log");

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
              "$0": "../core_universal/applitools/core_universal/bin/core"
            }
                    ))
        );
    }

    // Test for a socket log emitting an event.
    #[test]
    fn test_parse_socket_emit_event() {
        let log_line = r#"socket | 2025-04-03T21:35:06.157Z [INFO ] Emit event of type "Logger.log" with payload {
    "level": "info",
    "message": "Logs saved in: /Users/eirenik0/Projects/APPLITOOLS/eyes.sdk/logs"
}"#;
        let record = parse_log_entry(log_line).expect("Failed to parse socket emit event log");

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
    "cwd": "/Users/eirenik0/Projects/APPLITOOLS/eyes.sdk/python/tests",
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
        let record = parse_log_entry(log_line).expect("Failed to parse received event log");

        // Validate basic fields.
        assert_eq!(record.component, "socket");
        assert_eq!(record.level, "INFO");
        // Additional assertions should validate the JSON event type and payload if your parser extracts them.
    }

    // Test for a driver log related to switching context.
    #[test]
    fn test_parse_driver_switch_context() {
        let log_line = r#"driver (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx) | 2025-04-03T21:35:14.042Z [INFO ] Switching to a child context with depth: 0"#;
        let record = parse_log_entry(log_line).expect("Failed to parse driver context switch log");

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
        let record = parse_log_entry(log_line).expect("Failed to parse DOM snapshot log");

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
        let log_line = r#"core-requests (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/eyes-base-htm/core-request-bdg) | 2025-04-03T21:35:29.392Z [INFO ] Request "openEyes" [0--e6f57eb8-a8a0-4d1f-985b-9de36025ce90] will be sent to the address "[POST]https://eyesapi.applitools.com/api/sessions/running" with body {"startInfo":{ ... }}"#;
        let record = parse_log_entry(log_line).expect("Failed to parse openEyes request log");

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
        let record = parse_log_entry(log_line).expect("Failed to parse startRenders log");

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
        let log_line = r#"ufg-requests (manager-ufg-hoh/eyes-ufg-aif/check-ufg-ebh/environment-lrd/get-actual-environment-4bu/get-actual-environments-g55 & manager-ufg-hoh/eyes-ufg-aif/check-ufg-ebh/environment-g6p/get-actual-environment-fpc/get-actual-environments-g55) | 2025-04-03T21:08:12.795Z [INFO ] Request "getActualEnvironments" [0--1af9f42c-67ff-48c9-b1f8-09ee02017cdb] will be sent to the address "[POST]https://ufg-wus.applitools.com/job-info" with body [{"agentId":"eyes-universal/4.33.0/eyes.visualgrid.ruby/6.6.1 [eyes.selenium.visualgrid.ruby/6.6.1]","webhook":"","stitchingService":"","platform":{"name":"linux","type":"web"},"browser":{"name":"chrome"},"renderInfo":{"width":400,"height":800,"target":"viewport"}},{"agentId":"eyes-universal/4.33.0/eyes.visualgrid.ruby/6.6.1 [eyes.selenium.visualgrid.ruby/6.6.1]","webhook":"","stitchingService":"","platform":{"name":"linux","type":"web"},"browser":{"name":"chrome"},"renderInfo":{"width":1000,"height":800,"target":"viewport"}}]"#;
        let record = parse_log_entry(log_line).expect("Failed to parse getActualEnvironments log");

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
        let log_line = r#"core-requests (manager-ufg-43w/eyes-ufg-oer/check-ufg-jdx/environment-oja/eyes-base-htm/core-request-bdg) | 2025-04-03T21:35:29.392Z [INFO ] Request "openEyes" [0--e6f57eb8-a8a0-4d1f-985b-9de36025ce90] will be sent to the address "[POST]https://eyesapi.applitools.com/api/sessions/running" with body {"startInfo":{"agentId":"eyes-universal/4.35.0/eyes.selenium.visualgrid.python/6.1.0","agentSessionId":"CheckWindowWithReloadLayoutBreakpoints--6894fe00-2c2b-4f39-b9b8-a309bc6b2359","agentRunId":"CheckWindowWithReloadLayoutBreakpoints--6894fe00-2c2b-4f39-b9b8-a309bc6b2359","appIdOrName":"Applitools Eyes SDK","scenarioIdOrName":"CheckWindowWithReloadLayoutBreakpoints","properties":[{"name":"browserVersion","value":"135.0.7049.52"}],"batchInfo":{"id":"6e8afcf5-bc7a-406a-9104-728d710183d5","name":"Py3.12|Sel4.15.2 Generated tests","startedAt":"2025-04-03T21:35:04Z"},"egSessionId":"f03c5a9b-dbad-4d04-8c65-d1abf3300f7a","environment":{"ufgJobType":"web","inferred":"useragent:Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/135.0.0.0 Safari/537.36","deviceInfo":"Desktop","displaySize":{"width":400,"height":800},"0.sg1fmhj9ufh":"got you!"},"branchName":"master","parentBranchName":"master","compareWithParentBranch":false,"ignoreBaseline":false,"latestCommitInfo":{"sha":"32dba3b1ba58911956b430911eeb7624e51cad66","timestamp":"2025-04-03T21:37:57+02:00"},"processId":"056b3f40-e104-4df2-b3df-5baefcbc35b9"}}"#;
        let record = parse_log_entry(log_line).expect("Failed to parse openEyes log");

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
        let record = parse_log_entry(log_line).expect("Failed to parse openEyes log");

        assert_eq!(record.component, "core-base");

        match record.kind {
            LogEntryKind::Command { command, .. } => {
                assert_eq!(command, "close".to_string());
            }
            _ => panic!("Wrong kind of log entry"),
        }
    }
}
