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
fn test_info_json_schema_aggregates_request_counts_across_multiple_files() {
    let dir = tempdir().expect("temp dir");
    let file1 = dir.path().join("part1.log");
    let file2 = dir.path().join("part2.log");

    write_file(
        &file1,
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"x\":1}\n",
    );
    write_file(
        &file2,
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id2] will be sent with body {\"x\":2}\n",
    );

    let output = Command::new(bin())
        .args([
            "info",
            "--json-schema",
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
        stdout.contains("foo (2 occurrences):"),
        "expected aggregated request count across files, got:\n{}",
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
fn test_perf_orphans_only_resolves_cross_file_pairs_after_timestamp_sort() {
    let dir = tempdir().expect("temp dir");
    let file_finish = dir.path().join("finish.log");
    let file_start = dir.path().join("start.log");

    // Intentionally provide files in reverse chronological order. Without global timestamp sort,
    // the completion would be seen before the start and the request would remain orphaned.
    write_file(
        &file_finish,
        "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id1] finished successfully with body {\"statusCode\":200}\n",
    );
    write_file(
        &file_start,
        "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"statusCode\":100}\n",
    );

    let output = Command::new(bin())
        .args([
            "perf",
            "--orphans-only",
            file_finish.to_str().expect("utf8 path"),
            file_start.to_str().expect("utf8 path"),
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
        stdout.contains("No orphaned operations found!"),
        "expected cross-file request to be paired after timestamp sort, got:\n{}",
        stdout
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

#[test]
fn test_diff_json_includes_unpaired_entries_in_unique_sections() {
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
            "-F",
            "json",
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be JSON");

    let unique_count = parsed["summary"]["unique_to_log1_count"]
        .as_u64()
        .expect("summary.unique_to_log1_count should be numeric");
    assert_eq!(unique_count, 1);

    let unique_entries = parsed["unique_to_log1"]
        .as_array()
        .expect("unique_to_log1 should be an array");
    assert_eq!(
        unique_entries.len(),
        1,
        "diff output should include unpaired entries in unique_to_log1, got:\n{}",
        stdout
    );
}

#[test]
fn test_diff_text_includes_unpaired_entries_in_unique_sections() {
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unpaired occurrence"),
        "expected diff text output to include unpaired unique entries, got:\n{}",
        stdout
    );
}

#[test]
fn test_trace_by_id_sorts_matches_across_files_and_shows_step_timing() {
    let dir = tempdir().expect("temp dir");
    let file_late = dir.path().join("late.log");
    let file_early = dir.path().join("early.log");

    write_file(
        &file_late,
        concat!(
            "svc (manager-ufg-3nl/eyes-ufg-zn8/open-abc) | 2026-01-01T00:00:01.000Z [INFO ] Correlation f227f11e checkpoint reached\n",
            "svc (manager-ufg-3nl/eyes-ufg-zn8/open-abc) | 2026-01-01T00:00:02.000Z [INFO ] Request \"openEyes\" [0--f227f11e-aaaa] finished successfully with body {\"ok\":true}\n",
        ),
    );
    write_file(
        &file_early,
        concat!(
            "svc (manager-ufg-3nl/eyes-ufg-zn8/open-abc) | 2026-01-01T00:00:00.500Z [INFO ] Request \"openEyes\" [0--f227f11e-aaaa] will be sent with body {\"ok\":false}\n",
            "svc (manager-ufg-999/eyes-ufg-zzz/open-def) | 2026-01-01T00:00:03.000Z [INFO ] Request \"openEyes\" [0--other-id] finished successfully with body {\"ok\":true}\n",
        ),
    );

    let output = Command::new(bin())
        .args([
            "trace",
            file_late.to_str().expect("utf8 path"),
            file_early.to_str().expect("utf8 path"),
            "--id",
            "f227f11e",
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
        stdout.contains("TRACE (id) contains \"f227f11e\""),
        "expected trace header, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Matched 3 entries"),
        "expected 3 matched entries, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("+   500ms") && stdout.contains("+  1000ms"),
        "expected step timing deltas in output, got:\n{}",
        stdout
    );

    let idx_0500 = stdout
        .find("2026-01-01T00:00:00.500")
        .expect("missing first timestamp");
    let idx_1000 = stdout
        .find("2026-01-01T00:00:01.000")
        .expect("missing second timestamp");
    let idx_2000 = stdout
        .find("2026-01-01T00:00:02.000")
        .expect("missing third timestamp");
    assert!(
        idx_0500 < idx_1000 && idx_1000 < idx_2000,
        "expected chronological ordering across files, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("other-id"),
        "expected non-matching entries to be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_trace_by_session_filters_using_component_id_hierarchy() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("session.log");

    write_file(
        &file,
        concat!(
            "core (manager-ufg-3nl/eyes-ufg-zn8) | 2026-01-01T00:00:00.000Z [INFO ] Started session flow\n",
            "driver (manager-ufg-3nl/eyes-ufg-zn8/close-zy9) | 2026-01-01T00:00:00.200Z [INFO ] Closing target\n",
            "core (manager-ufg-999/eyes-ufg-abc) | 2026-01-01T00:00:00.300Z [INFO ] Other session noise\n",
        ),
    );

    let output = Command::new(bin())
        .args([
            "trace",
            file.to_str().expect("utf8 path"),
            "--session",
            "manager-ufg-3nl",
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
        stdout.contains("Started session flow") && stdout.contains("Closing target"),
        "expected matching session hierarchy entries, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("Other session noise"),
        "expected non-matching session entries to be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_search_prints_matching_entries_and_payloads() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("search.log");

    write_file(
        &file,
        concat!(
            "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"retryTimeout\" [0--id1] will be sent with body {\"timeout\":1000}\n",
            "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"other\" [0--id2] will be sent with body {\"x\":1}\n",
            "core (manager-1) | 2026-01-01T00:00:02.000Z [WARN ] Request \"retryTimeout\" [0--id3] will be sent with body {\"timeout\":2000}\n",
        ),
    );

    let output = Command::new(bin())
        .args([
            "search",
            file.to_str().expect("utf8 path"),
            "-f",
            "t:retryTimeout",
            "--payloads",
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
        stdout.contains("SEARCH matched 2 entries"),
        "expected match count header, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("payload: {\"timeout\":1000}")
            && stdout.contains("payload: {\"timeout\":2000}"),
        "expected parsed payloads in output, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("Request \"other\""),
        "expected non-matching entry to be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_search_context_shows_neighbor_entries_only() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("context.log");

    write_file(
        &file,
        concat!(
            "svc | 2026-01-01T00:00:00.000Z [INFO ] alpha\n",
            "svc | 2026-01-01T00:00:01.000Z [INFO ] beta\n",
            "svc | 2026-01-01T00:00:02.000Z [INFO ] needle match\n",
            "svc | 2026-01-01T00:00:03.000Z [INFO ] delta\n",
            "svc | 2026-01-01T00:00:04.000Z [INFO ] epsilon\n",
        ),
    );

    let output = Command::new(bin())
        .args([
            "search",
            file.to_str().expect("utf8 path"),
            "-f",
            "t:needle",
            "--context",
            "1",
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
        stdout.contains("beta") && stdout.contains("needle match") && stdout.contains("delta"),
        "expected matching entry with one entry of context, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("alpha") && !stdout.contains("epsilon"),
        "expected outer entries to be excluded when context=1, got:\n{}",
        stdout
    );
}

#[test]
fn test_search_count_by_payload_groups_duplicate_payloads() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("count.log");

    write_file(
        &file,
        concat!(
            "svc | 2026-01-01T00:00:00.000Z [INFO ] Request \"concurrency\" [0--id1] will be sent with body {\"limit\":2}\n",
            "svc | 2026-01-01T00:00:01.000Z [INFO ] Request \"concurrency\" [0--id2] will be sent with body {\"limit\":2}\n",
            "svc | 2026-01-01T00:00:02.000Z [INFO ] Request \"concurrency\" [0--id3] will be sent with body {\"limit\":3}\n",
            "svc | 2026-01-01T00:00:03.000Z [INFO ] Request \"other\" [0--id4] will be sent with body {\"limit\":999}\n",
        ),
    );

    let output = Command::new(bin())
        .args([
            "search",
            file.to_str().expect("utf8 path"),
            "-f",
            "t:concurrency",
            "--count-by",
            "payload",
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
        stdout.contains("SEARCH count by payload (3 entries)"),
        "expected payload count header, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("     2  {\"limit\":2}") && stdout.contains("     1  {\"limit\":3}"),
        "expected grouped payload counts, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("{\"limit\":999}"),
        "expected non-matching payload to be excluded, got:\n{}",
        stdout
    );
}
