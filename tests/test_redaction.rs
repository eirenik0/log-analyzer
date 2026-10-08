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
