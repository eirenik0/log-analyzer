use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_log-analyzer")
}

fn write_file(path: &Path, content: &str) {
    fs::write(path, content).expect("failed to write test file");
}

#[test]
fn test_json_format_written_to_output_file_is_json() {
    let dir = tempdir().expect("temp dir");
    let file1 = dir.path().join("a.log");
    let file2 = dir.path().join("b.log");
    let out = dir.path().join("out.json");

    write_file(
        &file1,
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":1}\n",
    );
    write_file(
        &file2,
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":2}\n",
    );

    let output = Command::new(bin())
        .args([
            "-F",
            "json",
            "-o",
            out.to_str().expect("utf8 path"),
            "diff",
            file1.to_str().expect("utf8 path"),
            file2.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let file_content = fs::read_to_string(&out).expect("output file should exist");
    assert!(
        file_content.trim_start().starts_with('{'),
        "expected JSON content in output file, got:\n{}",
        file_content
    );
}

#[test]
fn test_full_diff_prints_full_json_payloads() {
    let dir = tempdir().expect("temp dir");
    let file1 = dir.path().join("a.log");
    let file2 = dir.path().join("b.log");

    write_file(
        &file1,
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"a\":1,\"b\":2}\n",
    );
    write_file(
        &file2,
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"a\":3,\"b\":4}\n",
    );

    let output = Command::new(bin())
        .args([
            "diff",
            "--full",
            file1.to_str().expect("utf8 path"),
            file2.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"a\"") && stdout.contains("\"b\""),
        "expected full JSON payload fields in --full output, got:\n{}",
        stdout
    );
}

#[test]
fn test_info_json_schema_uses_request_occurrence_counts() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("requests.log");

    let content = concat!(
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":1}\n",
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id2] will be sent with body {\"x\":2}\n",
    );
    write_file(&file, content);

    let output = Command::new(bin())
        .args(["info", "--json-schema", file.to_str().expect("utf8 path")])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("foo (2 occurrences):"),
        "expected request schema section to report request count, got:\n{}",
        stdout
    );
}

#[test]
fn test_process_honors_output_file_flag() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("input.log");
    let out = dir.path().join("process.json");

    write_file(
        &file,
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":1}\n",
    );

    let output = Command::new(bin())
        .args([
            "-o",
            out.to_str().expect("utf8 path"),
            "process",
            file.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        out.exists(),
        "expected process output file to be created when -o is provided"
    );
}

#[test]
fn test_perf_text_honors_output_file_flag() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("input.log");
    let out = dir.path().join("perf.txt");

    // Request send + receive with same request id to produce one timed operation.
    let content = concat!(
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"statusCode\":100}\n",
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id1] finished successfully with body {\"statusCode\":200}\n",
    );
    write_file(&file, content);

    let output = Command::new(bin())
        .args([
            "-o",
            out.to_str().expect("utf8 path"),
            "perf",
            file.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.exists(),
        "expected perf text output file to be created when -o is provided"
    );

    let file_content = fs::read_to_string(&out).expect("output file should be readable");
    assert!(
        file_content.contains("PERFORMANCE ANALYSIS SUMMARY"),
        "expected text perf report in output file, got:\n{}",
        file_content
    );
}

#[test]
fn test_compare_shows_unpaired_annotation_in_unique_output() {
    let dir = tempdir().expect("temp dir");
    let file1 = dir.path().join("a.log");
    let file2 = dir.path().join("b.log");

    let a_content = concat!(
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":1}\n",
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id2] will be sent with body {\"x\":2}\n",
        "svc | 2026-01-01T00:00:02.000Z [INFO ] Request \"foo\" [0--id3] will be sent with body {\"x\":3}\n",
    );
    let b_content = concat!(
        "svc | 2026-01-01T00:00:03.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":10}\n",
        "svc | 2026-01-01T00:00:04.000Z [INFO ] Request \"foo\" [0--id2] will be sent with body {\"x\":20}\n",
    );
    write_file(&file1, a_content);
    write_file(&file2, b_content);

    let output = Command::new(bin())
        .args([
            "-v",
            "compare",
            file1.to_str().expect("utf8 path"),
            file2.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unpaired occurrence"),
        "expected unpaired entries to be visible in unique output, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Send `foo`"),
        "expected unique output to preserve request details, got:\n{}",
        stdout
    );
}
