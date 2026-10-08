use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::tempdir;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .env("LOG_ANALYZER_PRESET", "eyes")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn redaction_covers_text_json_and_files_without_changing_matching() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.jsonl");
    let id = "request-".to_string() + &"x".repeat(160);
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"ERROR","component":"core","componentId":"scope",
        "message":"failure https://example.test/items?%74oken=encoded%26secret&request_id=allowed token=message-secret",
        "payload":{"token":"payload-secret","request_id":id,"nested":{"password":"nested-secret"},"serialized":"{\"api_key\":\"string-secret\"}"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let file = file.to_str().unwrap();
    for format in ["text", "json"] {
        for command in [
            "search", "extract", "process", "trace", "errors", "perf", "info",
        ] {
            let target = dir.path().join(format!("{command}-{format}.out"));
            let mut args = vec![
                "--redact",
                "-F",
                format,
                "-o",
                target.to_str().unwrap(),
                command,
                file,
            ];
            match command {
                "extract" => args.extend(["--field", "token"]),
                "trace" => args.extend(["--id", "allowed"]),
                "search" => args.extend(["--payloads"]),
                "process" => args.extend(["--no-sanitize"]),
                "info" => args.extend(["--payloads"]),
                _ => (),
            }
            let output = run(&args);
            assert!(
                output.status.success(),
                "{command}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            for secret in [
                "encoded%26secret",
                "message-secret",
                "payload-secret",
                "nested-secret",
                "string-secret",
            ] {
                assert!(
                    !stdout.contains(secret),
                    "{command} {format} leaked {secret}: {stdout}"
                );
            }
            if format == "json" || command == "process" {
                let report: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(report["redaction"]["applied"], true);
                if command == "process" {
                    assert!(stdout.contains(&id), "long ID lost: {stdout}");
                }
            } else {
                assert!(stdout.starts_with("[REDACTED OUTPUT]"));
            }
            if target.exists() {
                let saved = fs::read_to_string(&target).unwrap();
                assert!(saved.contains("REDACTED") || saved.contains("redaction"));
                assert!(!saved.contains("payload-secret"));
            }
            if command == "search" {
                assert!(stdout.contains("example.test/items"));
                assert!(stdout.contains("allowed"));
            }
        }
    }
    let raw = run(&["-F", "json", "search", file, "--payloads"]);
    assert!(String::from_utf8_lossy(&raw.stdout).contains("payload-secret"));
}

#[test]
fn stable_identifier_masking_matches_text_json_and_saved_reports() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ids.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"https://example.test/?trace_id=trace%2Dsame&token=secret", "payload":{"trace_id":"trace-same","items":[{"trace_id":"trace-same"},{"trace_id":"other"}]}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let target = dir.path().join("out.json");
    let output = run(&[
        "--redact",
        "--mask-id",
        "trace_id",
        "-F",
        "json",
        "-o",
        target.to_str().unwrap(),
        "search",
        file.to_str().unwrap(),
        "--payloads",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let saved = fs::read_to_string(target).unwrap();
    assert_eq!(stdout, saved);
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        report["search"]["entries"][0]["payload"]["trace_id"],
        "[MASKED_ID:1]"
    );
    assert_eq!(
        report["search"]["entries"][0]["payload"]["items"][0]["trace_id"],
        "[MASKED_ID:1]"
    );
    assert_eq!(
        report["search"]["entries"][0]["payload"]["items"][1]["trace_id"],
        "[MASKED_ID:2]"
    );
    assert!(!stdout.contains("trace-same") && !stdout.contains("trace%2Dsame"));
}

#[test]
fn comparison_redacts_generic_diff_values_and_saved_text() {
    let dir = tempdir().unwrap();
    let left = dir.path().join("left.log");
    let right = dir.path().join("right.log");
    for (file, secret) in [(&left, "left-secret"), (&right, "right-secret")] {
        fs::write(
            file,
            format!("core | 2026-01-01T00:00:00.000Z [INFO ] body {{\"token\":\"{secret}\"}}\n"),
        )
        .unwrap();
    }
    for format in ["text", "json"] {
        let target = dir.path().join(format!("{format}.out"));
        let output = run(&[
            "--redact",
            "-F",
            format,
            "-o",
            target.to_str().unwrap(),
            "diff",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--full",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for text in [
            String::from_utf8(output.stdout).unwrap(),
            fs::read_to_string(target).unwrap(),
        ] {
            assert!(
                !text.contains("left-secret") && !text.contains("right-secret"),
                "{text}"
            );
            assert!(text.contains("REDACTED"));
        }
    }
}

#[test]
fn redaction_handles_camel_case_auth_and_utf8_percent_fragments() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("fragments.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core",
        "message":"Authorization: Bearer auth-secret https://example.test/?token=%😀 settings.token=dot-secret",
        "payload":{"accessToken":"camel-secret","clientSecret":"client-secret"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    for format in ["text", "json"] {
        let output = run(&[
            "--redact",
            "-F",
            format,
            "search",
            file.to_str().unwrap(),
            "--payloads",
        ]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        for secret in ["auth-secret", "dot-secret", "camel-secret", "client-secret"] {
            assert!(!stdout.contains(secret), "{stdout}");
        }
    }
}

#[test]
fn redaction_keeps_filters_on_original_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("filter.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"token=filter-secret", "payload":{"trace_id":"selected-id"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let output = run(&[
        "--redact",
        "-F",
        "json",
        "search",
        file.to_str().unwrap(),
        "--filter",
        "t:filter-secret",
    ]);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["search"]["matches"], 1);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("filter-secret"));
    let output = run(&[
        "--redact",
        "--mask-id",
        "trace_id",
        "-F",
        "json",
        "trace",
        file.to_str().unwrap(),
        "--id",
        "selected-id",
    ]);
    assert!(output.status.success());
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("selected-id"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn masking_precedes_process_truncation_and_masks_text_trace_selector() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ids.jsonl");
    let id = "long-".to_string() + &"x".repeat(250);
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":format!("https://example.test/?trace_id={id}"), "payload":{"trace_id":id}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let output = run(&[
        "--redact",
        "--mask-id",
        "trace_id",
        "process",
        file.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("long-"));
    assert!(!text.contains("[MASKED_ID:2]"), "{report}");
    let output = run(&[
        "--redact",
        "--mask-id",
        "trace_id",
        "trace",
        file.to_str().unwrap(),
        "--id",
        &id,
    ]);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("long-"));
}

#[test]
fn known_ids_are_masked_in_bare_text_and_source_components() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("components.log");
    fs::write(&file, "core (scope-123) | 2026-01-01T00:00:00.000Z [INFO ] Request \"run\" [0--request-123] will be sent with body {\"trace_id\":\"trace-123\"}\ncore (scope-123) | 2026-01-01T00:00:01.000Z [INFO ] received answer for 0--request-123 and trace-123\n").unwrap();
    let target = dir.path().join("out.txt");
    let output = run(&[
        "--redact",
        "--mask-id",
        "component_id",
        "--mask-id",
        "request_id",
        "--mask-id",
        "trace_id",
        "-o",
        target.to_str().unwrap(),
        "trace",
        file.to_str().unwrap(),
        "--id",
        "0--request-123",
    ]);
    assert!(output.status.success());
    for text in [
        String::from_utf8(output.stdout).unwrap(),
        fs::read_to_string(target).unwrap(),
    ] {
        for id in ["scope-123", "0--request-123", "trace-123"] {
            assert!(!text.contains(id), "{text}");
        }
        assert!(text.contains("[MASKED_ID:"));
    }
}

#[test]
fn object_identifier_values_still_obey_compaction_limits() {
    let items = (0..30)
        .map(|_| json!({"nested":{"deeper":{"text":"x".repeat(500)}}}))
        .collect::<Vec<_>>();
    let compact =
        log_analyzer::llm_processor::compact_json_value(&json!({"request_id":items}), 3, 0);
    assert_eq!(compact["request_id"].as_array().unwrap().len(), 11);
    assert!(compact.to_string().contains("TRUNCATED"));
    assert!(compact.to_string().len() < 1000);
}

#[test]
fn numeric_ids_in_prose_and_unmatched_selectors_are_masked() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("numeric.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"received answer for 12345", "payload":{"request_id":"12345"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    for format in ["text", "json"] {
        for id in ["12345", "missing-id"] {
            let output = run(&[
                "--redact",
                "--mask-id",
                "request_id",
                "-F",
                format,
                "trace",
                file.to_str().unwrap(),
                "--id",
                id,
            ]);
            assert!(output.status.success());
            assert!(
                !String::from_utf8_lossy(&output.stdout).contains(id),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

#[test]
fn bounded_errors_budget_includes_redaction_and_expanded_masks() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bounded.log");
    fs::write(
        &file,
        format!(
            "core (scope) | 2026-01-01T00:00:00.000Z [ERROR ] failure {}\n",
            "token=x ".repeat(400)
        ),
    )
    .unwrap();
    for budget in [0, 300, 1000, 1800, 2500] {
        let target = dir.path().join("out.txt");
        let output = run(&[
            "--redact",
            "errors",
            file.to_str().unwrap(),
            "--bounded",
            "--max-output-chars",
            &budget.to_string(),
            "-o",
            target.to_str().unwrap(),
        ]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text, fs::read_to_string(target).unwrap());
        assert!(text.starts_with("[REDACTED OUTPUT]"));
        assert!(
            text.chars().count() <= budget || text.contains("Mandatory metadata exceeds budget"),
            "budget {budget}: {} chars\n{text}",
            text.chars().count()
        );
        assert!(!text.contains("token=x"));
    }
}

#[test]
fn bounded_json_samples_do_not_expand_after_truncation() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.log");
    fs::write(
        &file,
        "core (scope) | 2026-01-01T00:00:00.000Z [ERROR ] token=x\n",
    )
    .unwrap();
    let output = run(&[
        "--redact",
        "-F",
        "json",
        "errors",
        file.to_str().unwrap(),
        "--bounded",
        "--max-sample-chars",
        "10",
    ]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let sample = report["errors"]["clusters"][0]["sample_message"]
        .as_str()
        .unwrap();
    assert!(sample.chars().count() <= 10, "{sample}");
    assert!(!sample.contains("token=x"));
    assert_eq!(report["redaction"]["applied"], true);
}

#[test]
fn authorization_redacts_digest_aws_and_unknown_schemes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("authorization.jsonl");
    let rows = [
        "Authorization: AWS4-HMAC-SHA256 Credential=aws-secret, SignedHeaders=host, Signature=signature-secret",
        "Authorization: Digest username=\"digest-secret\", response=\"response-secret\"",
        "Authorization: CustomScheme opaque-secret second-secret",
        "Authorization: CustomScheme first-secret\n continuation-secret\nrequest_id=allowed",
    ].iter().map(|message| json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core","message":message}).to_string()).collect::<Vec<_>>().join("\n");
    fs::write(&file, rows).unwrap();
    for format in ["text", "json"] {
        let output = run(&["--redact", "-F", format, "search", file.to_str().unwrap()]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        for secret in [
            "aws-secret",
            "signature-secret",
            "digest-secret",
            "response-secret",
            "opaque-secret",
            "second-secret",
            "first-secret",
            "continuation-secret",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
        assert!(text.contains("allowed"), "{text}");
    }
}

#[test]
fn compaction_preserves_identifier_strings_across_name_styles() {
    let id = "correlation-".to_string() + &"x".repeat(250);
    for key in [
        "requestId",
        "traceId",
        "REQUEST_ID",
        "trace-id",
        "requestid",
        "custom_Id",
    ] {
        let compact = log_analyzer::llm_processor::compact_json_value(&json!({key:id}), 3, 0);
        assert_eq!(compact[key], id);
        let compact =
            log_analyzer::llm_processor::compact_json_value(&json!({key: vec!["x";30]}), 3, 0);
        assert_eq!(compact[key].as_array().unwrap().len(), 11);
    }
    let compact =
        log_analyzer::llm_processor::compact_json_value(&json!({"grid":"x".repeat(250)}), 3, 0);
    assert!(compact["grid"].as_str().unwrap().len() <= 100);
}

#[test]
fn sensitive_assignments_redact_whole_embedded_structures_and_cookie_headers() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("structures.jsonl");
    let rows = [
        "Authorization: {\"proof\":\"auth-object-secret\"}",
        "token=[{\"proof\":\"token-array-secret\"}]",
        "Cookie: session=cookie-secret; other=second-cookie-secret",
        "Set-Cookie: name=set-cookie-secret; Path=/; Secure",
    ]
    .iter()
    .map(|message| {
        json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core","message":message})
            .to_string()
    })
    .collect::<Vec<_>>()
    .join("\n");
    fs::write(&file, rows).unwrap();
    for format in ["text", "json"] {
        let output = run(&["--redact", "-F", format, "search", file.to_str().unwrap()]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        for secret in [
            "auth-object-secret",
            "token-array-secret",
            "cookie-secret",
            "second-cookie-secret",
            "set-cookie-secret",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
    }
}

#[test]
fn literal_dotted_sensitive_keys_are_redacted() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("dotted.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"values", "payload":{"api.key":"api-secret","private.key":"private-secret","request.id":"allowed"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    for format in ["text", "json"] {
        let output = run(&[
            "--redact",
            "-F",
            format,
            "search",
            file.to_str().unwrap(),
            "--payloads",
        ]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            !text.contains("api-secret") && !text.contains("private-secret"),
            "{text}"
        );
        assert!(text.contains("allowed"));
    }
}

#[test]
fn redaction_preserves_compact_compare_and_llm_output() {
    let dir = tempdir().unwrap();
    let left = dir.path().join("left.log");
    let right = dir.path().join("right.log");
    for (file, secret) in [(&left, "left-secret"), (&right, "right-secret")] {
        fs::write(
            file,
            format!("core | 2026-01-01T00:00:00.000Z [INFO ] body {{\"token\":\"{secret}\"}}\n"),
        )
        .unwrap();
    }
    for options in [
        vec!["compare", "-j"],
        vec!["compare", "-c", "-F", "json"],
        vec!["llm-diff"],
    ] {
        let target = dir.path().join("out.json");
        let mut args = vec!["--redact", "-o", target.to_str().unwrap()];
        args.extend(options);
        args.extend([left.to_str().unwrap(), right.to_str().unwrap()]);
        let output = run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert_eq!(text, fs::read_to_string(target).unwrap());
        let report: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(report["redaction"]["applied"], true);
        assert!(!text.contains("left-secret") && !text.contains("right-secret"));
    }
}

#[test]
fn dotted_extract_and_comparison_values_keep_sensitive_context() {
    let dir = tempdir().unwrap();
    let left = dir.path().join("left.jsonl");
    let right = dir.path().join("right.jsonl");
    for (file, secret) in [(&left, "left-secret"), (&right, "right-secret")] {
        let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"fields", "api.key":secret, "payload":{"api.key":secret}});
        fs::write(file, format!("{row}\n")).unwrap();
    }
    for format in ["text", "json"] {
        for command in ["extract", "diff"] {
            let mut args = vec!["--redact", "-F", format, command, left.to_str().unwrap()];
            if command == "extract" {
                args.extend(["--field", "api.key"]);
            } else {
                args.push(right.to_str().unwrap());
            }
            let output = run(&args);
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            if format == "json" {
                let report: Value = serde_json::from_str(&text).unwrap();
                if command == "extract" {
                    assert_eq!(report["extract"]["groups"][0]["value"], "[REDACTED]");
                } else {
                    assert_eq!(report["summary"]["differences_count"], 1);
                }
            }
            assert!(
                !text.contains("left-secret") && !text.contains("right-secret"),
                "{command} {format}: {text}"
            );
        }
    }
}

#[test]
fn redacted_text_match_counts_have_the_marker() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("count.jsonl");
    fs::write(
        &file,
        json!({"ts":"2026-01-01T00:00:00Z","message":"one"}).to_string(),
    )
    .unwrap();
    let target = dir.path().join("count.txt");
    let output = run(&[
        "--redact",
        "-o",
        target.to_str().unwrap(),
        "search",
        file.to_str().unwrap(),
        "--count-by",
        "matches",
    ]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[REDACTED OUTPUT]\n1\n"
    );
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "[REDACTED OUTPUT]\n1\n"
    );
}

#[test]
fn nested_sensitive_paths_and_masked_extracts_keep_context() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("nested.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"values", "payload":{"trace_id":"selected-id","parent":{"api":{"key":"nested-api-secret"},"private.key":"nested-private-secret"}}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let output = run(&[
        "--redact",
        "-F",
        "json",
        "search",
        file.to_str().unwrap(),
        "--payloads",
    ]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains("nested-api-secret") && !text.contains("nested-private-secret"),
        "{text}"
    );
    let output = run(&[
        "--redact",
        "--mask-id",
        "trace_id",
        "-F",
        "json",
        "extract",
        file.to_str().unwrap(),
        "--field",
        "trace_id",
    ]);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["extract"]["groups"][0]["value"], "[MASKED_ID:1]");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("[MASKED_ID:2]"));
    let row = json!({"ts":"2026-01-01T00:00:00Z","message":"one","payload":{"request_id":"1"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let output = run(&[
        "--redact",
        "--mask-id",
        "request_id",
        "search",
        file.to_str().unwrap(),
        "--count-by",
        "matches",
    ]);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[REDACTED OUTPUT]\n1\n"
    );
}

#[test]
fn complete_unquoted_scalars_and_raw_placeholder_like_ids_are_redacted() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("scalars.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"https://example.test/?token=query-secret&request_id=allowed token=[prefix]bracket-secret token=[1]array-suffix-secret token=prefix,comma-secret token=prefix&amp-secret token=[REDACTED]suffix-secret token={\"a\":1}object-suffix-secret", "payload":{"request_id":"[MASKED_ID:1]-customer-42"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    for format in ["text", "json"] {
        let output = run(&[
            "--redact",
            "--mask-id",
            "request_id",
            "-F",
            format,
            "search",
            file.to_str().unwrap(),
            "--payloads",
        ]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        for secret in [
            "bracket-secret",
            "array-suffix-secret",
            "object-suffix-secret",
            "comma-secret",
            "amp-secret",
            "suffix-secret",
            "query-secret",
            "customer-42",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
        assert!(text.contains("example.test"));
    }
}

#[test]
fn overlapping_known_ids_use_the_complete_stable_mask() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("overlap.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":"received answer for 123.456 and 123", "payload":{"request_id":"123","trace_id":"123.456"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let output = run(&[
        "--redact",
        "--mask-id",
        "request_id",
        "--mask-id",
        "trace_id",
        "-F",
        "json",
        "search",
        file.to_str().unwrap(),
        "--payloads",
    ]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entry = &report["search"]["entries"][0];
    assert_eq!(
        entry["message"],
        "received answer for [MASKED_ID:2] and [MASKED_ID:1]"
    );
    assert_eq!(entry["payload"]["trace_id"], "[MASKED_ID:2]");
}

#[test]
fn expanding_secret_redaction_keeps_unicode_process_messages_valid() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unicode.jsonl");
    let message = format!("token=x {}🙂tail", "a".repeat(179));
    let row =
        json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core","message":message});
    fs::write(&file, format!("{row}\n")).unwrap();
    let output = run(&["--redact", "process", file.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.to_string().contains("[REDACTED]"));
    assert!(!report.to_string().contains("token=x"));
}

#[test]
fn numeric_masks_preserve_physical_lines_counts_and_timestamps() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("numbers.jsonl");
    let row = json!({"ts":"2026-01-01T00:00:01Z","level":"INFO","component":"core","componentId":"1", "message":"answer for 1 and 01", "payload":{"request_id":"1","trace_id":"01"}});
    fs::write(&file, format!("{row}\n")).unwrap();
    for format in ["text", "json"] {
        let output = run(&[
            "--redact",
            "--mask-id",
            "request_id",
            "--mask-id",
            "trace_id",
            "--mask-id",
            "component_id",
            "-F",
            format,
            "search",
            file.to_str().unwrap(),
        ]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("2026-01-01T00:00:01.000Z"), "{text}");
        assert!(!text.contains("answer for 1"), "{text}");
        if format == "text" {
            assert!(text.contains(">     1:"), "{text}");
        } else {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["search"]["entries"][0]["source_line_number"], 1);
        }
    }
}

#[test]
fn many_identifiers_keep_all_entries_and_stable_masks() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("many.jsonl");
    let rows: String = (0..2000).map(|index| {
        let id = format!("request-{index:05}");
        format!("{}\n", json!({"ts":"2026-01-01T00:00:00Z","level":"INFO","component":"core", "message":format!("answer for {id}"), "payload":{"request_id":id}}))
    }).collect();
    fs::write(&file, rows).unwrap();
    let output = run(&[
        "--redact",
        "--mask-id",
        "request_id",
        "-F",
        "json",
        "search",
        file.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["search"]["entries"].as_array().unwrap().len(), 2000);
    assert!(!text.contains("request-00000"));
    assert!(!text.contains("request-01999"));
    assert_eq!(
        report["search"]["entries"][1999]["message"],
        "answer for [MASKED_ID:2000]"
    );
}

#[test]
fn expanded_unicode_payloads_and_comparison_values_never_panic() {
    let dir = tempdir().unwrap();
    let left = dir.path().join("left.log");
    let right = dir.path().join("right.log");
    for (file, suffix) in [(&left, "a"), (&right, "b")] {
        let value = format!("token=x {}🙂{suffix}", "a".repeat(79));
        let short_value = format!("token=x {}🙂{suffix}", "a".repeat(31));
        let payload = json!({"body":value,"short_body":short_value});
        fs::write(
            file,
            format!("core | 2026-01-01T00:00:00.000Z [INFO] body {payload}\n"),
        )
        .unwrap();
    }
    let output = run(&["--redact", "process", left.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report.to_string().contains("[REDACTED]"));
    for command in ["compare", "diff"] {
        let output = run(&[
            "--redact",
            "-F",
            "text",
            command,
            left.to_str().unwrap(),
            right.to_str().unwrap(),
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("short_body"), "{text}");
        assert!(!text.contains("token=x"));
    }
}

#[test]
fn redacted_performance_contexts_truncate_unicode_safely() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("orphan.log");
    // Use the 100-byte orphan-only display path with an emoji across byte 97.
    let prefix =
        "core (demo) | 2026-01-01T00:00:00Z [INFO] Request \"a\" [0--id] will be sent token=x ";
    let expanded_prefix_len = prefix.len() + "[REDACTED]".len() - 1;
    assert!(expanded_prefix_len < 97);
    fs::write(
        &file,
        format!("{prefix}{}🙂tail\n", "a".repeat(96 - expanded_prefix_len)),
    )
    .unwrap();
    let output = run(&[
        "--redact",
        "-F",
        "text",
        "perf",
        file.to_str().unwrap(),
        "--orphans-only",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Context:"), "{text}");
    assert!(text.contains("[REDACTED]"), "{text}");
    assert!(!text.contains("token=x"));
}

#[test]
fn numeric_masks_preserve_nested_error_and_performance_timestamps() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("time.log");
    fs::write(&file, "core (demo) | 2026-01-01T00:00:01.000Z [ERROR] Request \"a\" [0--id] will be sent request_id=01 failure for 01\n").unwrap();
    let output = run(&[
        "--redact",
        "--mask-id",
        "request_id",
        "-F",
        "json",
        "errors",
        file.to_str().unwrap(),
        "--sessions",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = report.to_string();
    let timestamp =
        report["errors"]["clusters"][0]["affected_sessions"][0]["first_error_timestamp"]
            .as_str()
            .unwrap();
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&chrono::Utc)
            .to_rfc3339(),
        "2026-01-01T00:00:01+00:00"
    );
    assert!(!text.contains("failure for 01"), "{text}");
    let output = run(&[
        "--redact",
        "--mask-id",
        "request_id",
        "-F",
        "text",
        "perf",
        file.to_str().unwrap(),
        "--orphans-only",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let expected = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:01Z")
        .unwrap()
        .with_timezone(&chrono::Local)
        .to_rfc3339();
    assert!(text.contains(&expected), "{text}");
}
