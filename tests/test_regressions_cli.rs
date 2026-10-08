use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_log-analyzer")
}

fn command() -> Command {
    let mut cmd = Command::new(bin());
    cmd.env("LOG_ANALYZER_PRESET", "eyes");
    cmd
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

    let output = command()
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

    let output = command()
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

    let output = command()
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

    let output = command()
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

    let output = command()
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

    let output = command()
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
        "svc (demo) | 2026-01-01T00:00:01.000Z [INFO ] Request \"foo\" [0--id1] finished successfully with body {\"statusCode\":200}\n",
    );
    write_file(
        &file_start,
        "svc (demo) | 2026-01-01T00:00:00.000Z [INFO ] Request \"foo\" [0--id1] will be sent with body {\"statusCode\":100}\n",
    );

    let output = command()
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

    let output = command()
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

    let output = command()
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

    let output = command()
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

    let output = command()
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

    let output = command()
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
fn test_generate_config_merges_multiple_logs_for_inference() {
    let dir = tempdir().expect("temp dir");
    let file1 = dir.path().join("part1.log");
    let file2 = dir.path().join("part2.log");

    write_file(
        &file1,
        concat!(
            "core (manager-ufg-1/eyes-ufg-1) | 2026-01-01T00:00:00.000Z [INFO ] Command \"openEyes\" is called with settings {\"test\":1}\n",
            "core (manager-ufg-1/eyes-ufg-1) | 2026-01-01T00:00:00.100Z [INFO ] Generic message\n",
        ),
    );
    write_file(
        &file2,
        "network (manager-ufg-2/eyes-ufg-2) | 2026-01-01T00:00:01.000Z [INFO ] Request \"render\" [0--id1] will be sent with body {\"x\":1}\n",
    );

    let output = command()
        .args([
            "generate-config",
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
        stdout.contains("# Sources (2):"),
        "expected multi-source header, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("openEyes"),
        "expected command inferred from first file, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("render"),
        "expected request inferred from second file, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("manager-") && stdout.contains("eyes-"),
        "expected session prefixes inferred across files, got:\n{}",
        stdout
    );
}

#[test]
fn test_generate_config_detects_rust_tracing_format() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("flux.log");

    write_file(
        &file,
        concat!(
            "2026-03-10T08:15:30.123Z INFO fluxomni_server::integrations::srs::callback: callback received trace_id=abc123 actor_kind=switch restream_name=stream-a\n",
            "2026-03-10T08:15:31.123Z ERROR fluxomni_server::integrations::srs::callback: callback failed trace_id=abc123 exit_code=251\n",
        ),
    );

    let output = command()
        .args(["generate-config", file.to_str().expect("utf8 path")])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("format = \"rust-tracing\""),
        "expected generated parser format to match detected rust tracing logs, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("module_depth = 2"),
        "expected generated module depth inference, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("module_strip_prefix = \"fluxomni_\""),
        "expected generated module prefix inference, got:\n{}",
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

    let output = command()
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

    let output = command()
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

    let output = command()
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

#[test]
fn test_extract_aggregates_payload_field_values() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("extract.log");

    write_file(
        &file,
        concat!(
            "core | 2026-01-01T00:00:00.000Z [INFO ] Request \"makeManager\" [0--id1] will be sent with body {\"concurrency\":100,\"env\":\"prod\"}\n",
            "core | 2026-01-01T00:00:01.000Z [INFO ] Request \"makeManager\" [0--id2] will be sent with body {\"concurrency\":100,\"env\":\"prod\"}\n",
            "core | 2026-01-01T00:00:02.000Z [INFO ] Request \"makeManager\" [0--id3] will be sent with body {\"concurrency\":50,\"env\":\"staging\"}\n",
            "core | 2026-01-01T00:00:03.000Z [INFO ] Request \"otherCall\" [0--id4] will be sent with body {\"concurrency\":999}\n",
        ),
    );

    let output = command()
        .args([
            "extract",
            file.to_str().expect("utf8 path"),
            "-f",
            "t:makeManager",
            "--field",
            "concurrency",
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
        stdout.contains("concurrency=100 (2 occurrences)")
            && stdout.contains("concurrency=50 (1 occurrence)"),
        "expected grouped extracted values, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("999"),
        "expected non-matching entry to be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_search_and_extract_support_rust_tracing_structured_fields() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("flux.log");

    write_file(
        &file,
        concat!(
            "2026-03-10T08:15:30.123Z INFO fluxomni_server::integrations::srs::callback: callback received trace_id=abc123 actor_kind=switch restream_name=stream-a\n",
            "2026-03-10T08:15:31.123Z INFO fluxomni_server::integrations::srs::callback: callback received trace_id=abc456 actor_kind=switch restream_name=stream-a\n",
            "2026-03-10T08:15:32.123Z INFO fluxomni_server::integrations::srs::callback: callback received trace_id=def999 actor_kind=relay restream_name=stream-b\n",
        ),
    );

    let search_output = command()
        .args([
            "search",
            file.to_str().expect("utf8 path"),
            "-f",
            "actor_kind:switch",
            "--payloads",
        ])
        .output()
        .expect("search should run");

    assert!(
        search_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&search_output.stderr)
    );

    let search_stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(
        search_stdout.contains("SEARCH matched 2 entries"),
        "expected structured-field filter to match two entries, got:\n{}",
        search_stdout
    );
    assert!(
        search_stdout.contains("\"restream_name\":\"stream-a\""),
        "expected structured fields to be shown with --payloads, got:\n{}",
        search_stdout
    );
    assert!(
        !search_stdout.contains("stream-b"),
        "expected non-matching structured field values to be excluded, got:\n{}",
        search_stdout
    );

    let extract_output = command()
        .args([
            "extract",
            file.to_str().expect("utf8 path"),
            "-f",
            "actor_kind:switch",
            "--field",
            "restream_name",
        ])
        .output()
        .expect("extract should run");

    assert!(
        extract_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&extract_output.stderr)
    );

    let extract_stdout = String::from_utf8_lossy(&extract_output.stdout);
    assert!(
        extract_stdout.contains("restream_name=\"stream-a\" (2 occurrences)"),
        "expected structured field extraction to aggregate values, got:\n{}",
        extract_stdout
    );
}

#[test]
fn test_trace_by_id_matches_rust_tracing_trace_fields() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("trace.log");

    write_file(
        &file,
        concat!(
            "2026-03-10T08:15:30.123Z INFO fluxomni_server::scheduler::ingress: stream accepted trace_id=fabb5aa4 span_id=span-1 actor_kind=switch\n",
            "2026-03-10T08:15:30.523Z INFO fluxomni_server::scheduler::ingress: stream queued trace_id=fabb5aa4 span_id=span-1 actor_kind=switch\n",
            "2026-03-10T08:15:31.523Z ERROR fluxomni_server::scheduler::ingress: stream failed trace_id=fabb5aa4 span_id=span-1 exit_code=251\n",
            "2026-03-10T08:15:32.523Z INFO fluxomni_server::scheduler::ingress: other trace trace_id=zzzz9999 span_id=span-2 actor_kind=switch\n",
        ),
    );

    let output = command()
        .args([
            "trace",
            file.to_str().expect("utf8 path"),
            "--id",
            "fabb5aa4",
        ])
        .output()
        .expect("trace should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Matched 3 entries"),
        "expected trace_id-based matching to find three entries, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("+   400ms") && stdout.contains("+  1000ms"),
        "expected chronological timing deltas, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("zzzz9999"),
        "expected unrelated trace_id entries to be excluded, got:\n{}",
        stdout
    );
}

#[test]
fn test_errors_defaults_to_error_only_and_normalizes_cluster_pattern() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("errors.log");

    write_file(
        &file,
        concat!(
            "core (manager-1/eyes-1/check-1) | 2026-01-01T00:00:00.000Z [INFO ] Request \"check\" [0--rid-1] will be sent with body {\"x\":1}\n",
            "core (manager-1/eyes-1/check-1) | 2026-01-01T00:00:01.000Z [ERROR] Render with id \"5bfcc412-1fd6-4f8d-a6d5-246f90f3e7ab\" failed due to an error - internal failure\n",
            "core (manager-1/eyes-1/check-1) | 2026-01-01T00:00:02.000Z [INFO ] Request \"check\" [0--rid-1] finished successfully with body {\"statusCode\":200}\n",
            "core (manager-2/eyes-2/check-2) | 2026-01-01T00:00:03.000Z [WARN ] Warning - Invalid keys in check settings (will be ignored)\n",
        ),
    );

    let output = command()
        .args(["errors", file.to_str().expect("utf8 path")])
        .output()
        .expect("command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ERRORS: 1 entries (1 patterns) across 1 file"),
        "expected error-only header, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Render with id \"...\" failed due to an error - internal failure"),
        "expected normalized error cluster pattern, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("Warning - Invalid keys in check settings"),
        "expected WARN entries to be excluded by default, got:\n{}",
        stdout
    );
}

#[test]
fn test_errors_warn_and_sessions_show_completed_and_orphaned_sessions() {
    let dir = tempdir().expect("temp dir");
    let file = dir.path().join("errors_sessions.log");

    write_file(
        &file,
        concat!(
            "core (manager-1/eyes-1/check-1) | 2026-01-01T00:00:00.000Z [INFO ] Request \"check\" [0--req-complete] will be sent with body {\"x\":1}\n",
            "core (manager-1/eyes-1/check-1) | 2026-01-01T00:00:01.000Z [ERROR] Render with id \"5bfcc412-1fd6-4f8d-a6d5-246f90f3e7ab\" failed due to an error - internal failure\n",
            "core (manager-1/eyes-1/check-1) | 2026-01-01T00:00:02.000Z [INFO ] Request \"check\" [0--req-complete] finished successfully with body {\"statusCode\":200}\n",
            "core (manager-2/eyes-2/check-2) | 2026-01-01T00:00:03.000Z [INFO ] Request \"check\" [0--req-orphan] will be sent with body {\"x\":1}\n",
            "core (manager-2/eyes-2/check-2) | 2026-01-01T00:00:04.000Z [ERROR] Render with id \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\" failed due to an error - internal failure\n",
            "core (manager-2/eyes-2/check-2) | 2026-01-01T00:00:05.000Z [WARN ] Warning - Invalid keys in check settings (will be ignored)\n",
        ),
    );

    let output = command()
        .args([
            "errors",
            file.to_str().expect("utf8 path"),
            "--warn",
            "--sessions",
            "--top-n",
            "0",
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
        stdout.contains("ERRORS/WARNS: 3 entries (2 patterns) across 1 file"),
        "expected combined error/warn header, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Warning - Invalid keys in check settings (will be ignored)"),
        "expected WARN cluster with --warn, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("manager-1/eyes-1/check-1") && stdout.contains("completed"),
        "expected completed session status, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("manager-2/eyes-2/check-2") && stdout.contains("orphaned"),
        "expected orphaned session status, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Longest blocking error:"),
        "expected impact summary blocking line, got:\n{}",
        stdout
    );
}

#[test]
fn test_analysis_coverage_distinguishes_parsing_from_selection() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("coverage.log");
    let out = dir.path().join("coverage.json");
    let cases = [
        ("", "empty_input", 0, 0, 0),
        ("\n  \n", "empty_input", 0, 0, 0),
        (
            "unsupported format\n    at frame1\n    at frame2\n",
            "unparsed_input",
            0,
            0,
            1,
        ),
        (
            "worker | 2026-10-07T10:00:00.000Z [INFO ] started\n",
            "parsed",
            1,
            1,
            0,
        ),
    ];
    for (content, status, parsed, matched, rejected) in cases {
        write_file(&file, content);
        for subcommand in ["info", "errors", "perf"] {
            let result = command()
                .args([
                    "-F",
                    "json",
                    "-o",
                    out.to_str().unwrap(),
                    subcommand,
                    file.to_str().unwrap(),
                ])
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(if status == "unparsed_input" { 1 } else { 0 })
            );
            let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(value["coverage"]["status"], status);
            assert_eq!(value["coverage"]["parsed_entries"], parsed);
            assert_eq!(value["coverage"]["filter_matches"], matched);
            let coverage = &value["coverage"]["files"][0];
            assert_eq!(coverage["input_bytes"], content.len());
            assert_eq!(coverage["parsed_entries"], parsed);
            assert_eq!(coverage["rejected_candidates"], rejected);
            assert_eq!(coverage["selected_parser"], "classic");
            assert_eq!(coverage["configured_parser"], "auto");
            assert_eq!(coverage["profile"], "eyes");
            assert_eq!(fs::read(&out).unwrap(), result.stdout);
            if subcommand == "errors" && status == "parsed" {
                assert_eq!(value["errors"]["summary"]["total_entries"], 0);
            }
        }
    }
    write_file(&file, "worker | 2026-10-07T10:00:00.000Z [INFO ] started\n");
    for subcommand in ["info", "errors", "perf"] {
        let result = command()
            .args([
                "-F",
                "json",
                "-f",
                "c:absent",
                subcommand,
                file.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(result.status.success());
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["coverage"]["status"], "zero_filter_matches");
        assert_eq!(value["coverage"]["parsed_entries"], 1);
        assert_eq!(value["coverage"]["filter_matches"], 0);
    }
}

#[test]
fn test_nonempty_unparsed_input_text_fails_even_in_a_mixed_file_set() {
    let dir = tempdir().unwrap();
    let bad = dir.path().join("bad.log");
    let good = dir.path().join("good.log");
    write_file(&bad, "an unsupported record\n");
    write_file(&good, "worker | 2026-10-07T10:00:00.000Z [INFO ] started\n");
    for subcommand in ["info", "errors", "perf"] {
        let result = command()
            .args([subcommand, good.to_str().unwrap(), bad.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("Status: unparsed_input"));
        assert!(text.contains("input=22 bytes, parsed=0 entries, rejected=1 candidates"));
        assert!(!text.contains("completed successfully"));
        assert!(String::from_utf8_lossy(&result.stderr).contains("no recognized log entries"));
    }
}

#[test]
fn test_browser_console_fixture_reports_real_error_with_full_coverage() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("console.log");
    write_file(
        &file,
        concat!(
            "background.js:123 worker | 2026-10-07T10:00:00.000Z [INFO ] started\n",
            "background.js:124 worker | 2026-10-07T10:00:01.000Z [ERROR] example failure\n",
        ),
    );
    for subcommand in ["info", "errors", "perf"] {
        let result = command()
            .args(["-F", "json", subcommand, file.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(result.status.success());
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["coverage"]["parsed_entries"], 2);
        assert_eq!(value["coverage"]["files"][0]["rejected_candidates"], 0);
        if subcommand == "errors" {
            assert_eq!(value["errors"]["summary"]["error_count"], 1);
            assert_eq!(
                value["errors"]["clusters"][0]["sample_message"],
                "example failure"
            );
        }
    }
}

fn long_stack_fixture() -> String {
    let mut content = String::new();
    for (index, name) in ["alpha", "beta", "gamma"].iter().enumerate() {
        content.push_str(&format!("worker (session-{name}) | 2026-10-07T10:00:0{index}.000Z [ERROR] Failed task {name} 🦀\n"));
        for frame in 0..80 {
            content.push_str(&format!(
                "    at syntheticFunction{frame} (/example/app.js:{}:1)\n",
                frame + 1
            ));
        }
    }
    content
}

#[test]
fn test_bounded_errors_preserves_coverage_and_all_cluster_overviews_before_samples() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("stacks.log");
    let out = dir.path().join("report.txt");
    write_file(&file, &long_stack_fixture());
    let result = command()
        .args([
            "--color",
            "never",
            "-o",
            out.to_str().unwrap(),
            "errors",
            file.to_str().unwrap(),
            "--sessions",
            "--bounded",
            "--top-n",
            "3",
            "--max-output-chars",
            "2400",
            "--max-stack-frames",
            "2",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&out).unwrap(), result.stdout);
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.chars().count() <= 2400, "{}", text.chars().count());
    assert!(text.contains("3 entries (3 patterns) across 1 file"));
    assert!(text.contains("parsed=3 entries, rejected=0 candidates"));
    assert!(text.contains("Affected sessions: 3"));
    assert!(text.contains("Omitted: 0 clusters;"));
    assert!(text.contains("234 stack-frame lines"));
    let impact = text.find("Impact summary").unwrap();
    let sample = text.find("Sample #1").unwrap();
    for heading in ["#1", "#2", "#3"] {
        let index = text.find(heading).unwrap();
        assert!(impact < index && index < sample);
    }
    assert_eq!(
        text.lines()
            .filter(|line| line.trim_start().starts_with("at "))
            .count(),
        6
    );
    let first_50 = text.lines().take(50).collect::<Vec<_>>().join("\n");
    assert!(first_50.contains("#3"));
    assert!(first_50.contains("Impact summary"));
}

#[test]
fn test_errors_budget_tiny_and_zero_limits_retain_metadata_and_omissions() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("stacks.log");
    write_file(&file, &long_stack_fixture());
    for budget in ["0", "1", "900", "1200", "2000"] {
        let result = command()
            .args([
                "errors",
                file.to_str().unwrap(),
                "--sessions",
                "--max-output-chars",
                budget,
                "--max-sample-chars",
                "0",
                "--max-stack-frames",
                "0",
            ])
            .output()
            .unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("Total errors: 3"));
        assert!(text.contains("Affected sessions: 3"));
        assert!(text.contains("Parse coverage"));
        assert!(text.contains("240 stack-frame lines"));
        assert!(!text.contains("Sample #"));
        if budget.parse::<usize>().unwrap() < 900 {
            assert!(text.contains("Mandatory metadata exceeds budget"));
            assert!(text.contains("Omitted: 3 clusters;"));
        } else {
            assert!(text.chars().count() <= budget.parse::<usize>().unwrap());
        }
    }
}

#[test]
fn test_errors_json_bounded_details_preserve_machine_readable_totals() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("stacks.log");
    write_file(&file, &long_stack_fixture());
    let result = command()
        .args([
            "-F",
            "json",
            "errors",
            file.to_str().unwrap(),
            "--bounded",
            "--top-n",
            "2",
            "--max-sample-chars",
            "600",
            "--max-stack-frames",
            "2",
            "--max-output-chars",
            "1",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let errors = &value["errors"];
    assert_eq!(errors["summary"]["error_count"], 3);
    assert_eq!(errors["summary"]["unique_patterns"], 3);
    assert_eq!(errors["summary"]["affected_sessions_count"], 3);
    assert_eq!(errors["clusters_total"], 3);
    assert_eq!(errors["clusters_displayed"], 2);
    assert_eq!(errors["omitted"]["clusters"], 1);
    assert_eq!(errors["omitted"]["stack_frames"], 236);
    assert_eq!(errors["options"]["output_budget_applies_to"], "text");
    let mut visible_chars = 0;
    for cluster in errors["clusters"].as_array().unwrap() {
        let sample = cluster["sample_message"].as_str().unwrap();
        assert!(sample.chars().count() <= 600);
        visible_chars += sample.chars().count();
        assert_eq!(
            sample
                .lines()
                .filter(|line| line.trim_start().starts_with("at "))
                .count(),
            2
        );
    }
    let entries = log_analyzer::parse_log_file(&file).unwrap();
    let total_chars: usize = entries
        .iter()
        .map(|entry| entry.message.chars().count())
        .sum();
    assert_eq!(
        errors["omitted"]["sample_chars"],
        total_chars - visible_chars
    );

    let complete = command()
        .args([
            "-F",
            "json",
            "errors",
            file.to_str().unwrap(),
            "--complete",
            "--top-n",
            "0",
        ])
        .output()
        .unwrap();
    assert!(complete.status.success());
    let complete: serde_json::Value = serde_json::from_slice(&complete.stdout).unwrap();
    for field in [
        "file_count",
        "total_entries",
        "error_count",
        "warn_count",
        "unique_patterns",
        "affected_sessions_count",
    ] {
        assert_eq!(
            complete["errors"]["summary"][field],
            errors["summary"][field]
        );
    }
    assert_eq!(
        complete["errors"]["summary"]["longest_blocking"]["duration_ms"],
        errors["summary"]["longest_blocking"]["duration_ms"]
    );
    assert_eq!(complete["errors"]["clusters_displayed"], 3);
    assert_eq!(complete["errors"]["omitted"]["sample_chars"], 0);
    assert_eq!(complete["errors"]["omitted"]["stack_frames"], 0);
    assert!(
        complete["errors"]["clusters"][0]["sample_message"]
            .as_str()
            .unwrap()
            .contains("syntheticFunction79")
    );
}

#[test]
fn test_error_limits_handle_unicode_and_complete_flag_conflicts() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unicode.log");
    write_file(&file, "worker | 2026-10-07T10:00:00Z [ERROR] 🦀🦀🦀ééé\n");
    let result = command()
        .args([
            "-F",
            "json",
            "errors",
            file.to_str().unwrap(),
            "--max-sample-chars",
            "4",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["errors"]["clusters"][0]["sample_message"], "🦀🦀🦀é");
    assert_eq!(value["errors"]["omitted"]["sample_chars"], 2);
    for flags in [
        vec!["--bounded"],
        vec!["--max-output-chars", "1000"],
        vec!["--max-sample-chars", "4"],
    ] {
        let result = command()
            .args(["errors", file.to_str().unwrap(), "--complete"])
            .args(flags)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
    }
}

#[test]
fn process_sorts_before_limit_and_reports_timestamp_bounds() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("order.jsonl");
    write_file(
        &file,
        concat!(
            "{\"timestamp\":\"2026-01-01T00:00:02Z\",\"level\":\"INFO\",\"component\":\"alpha\",\"message\":\"later\"}\n",
            "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"level\":\"ERROR\",\"component\":\"zeta\",\"message\":\"earlier\"}\n",
            "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"level\":\"ERROR\",\"component\":\"zeta\",\"message\":\"tied\"}\n",
        ),
    );
    for (sort, first) in [
        ("time", "earlier"),
        ("component", "later"),
        ("level", "earlier"),
        ("type", "earlier"),
    ] {
        let output = command()
            .args([
                "process",
                file.to_str().unwrap(),
                "--sort-by",
                sort,
                "--limit",
                "1",
            ])
            .env("TZ", "UTC")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["logs"].as_array().unwrap().len(), 1);
        assert_eq!(report["logs"][0]["msg"], first, "sort: {sort}");
        assert_eq!(report["metadata"]["total_entries"], 3);
        assert_eq!(report["metadata"]["filtered_entries"], 1);
    }
    let output = command()
        .args([
            "process",
            file.to_str().unwrap(),
            "--sort-by",
            "component",
            "--limit",
            "0",
        ])
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["metadata"]["time_range"]["start"],
        "2026-01-01T00:00:01.000Z"
    );
    assert_eq!(
        report["metadata"]["time_range"]["end"],
        "2026-01-01T00:00:02.000Z"
    );
    assert_eq!(report["logs"][1]["msg"], "earlier");
    assert_eq!(report["logs"][2]["msg"], "tied");
    let output = command()
        .args(["process", file.to_str().unwrap(), "--sort-by", "diff-count"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value"));
}

#[test]
fn process_type_sort_orders_kinds_and_filters_before_limiting() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("types.log");
    write_file(
        &file,
        concat!(
            "svc | 2026-01-01T00:00:01.000Z [INFO] worker alive\n",
            "svc | 2026-01-01T00:00:02.000Z [INFO] Request \"fetch\" [0--demo] will be sent with body {\"x\":1}\n",
            "svc | 2026-01-01T00:00:03.000Z [INFO] Command \"run\" is called with settings {\"x\":1}\n",
        ),
    );
    let output = command()
        .args([
            "process",
            file.to_str().unwrap(),
            "--sort-by",
            "type",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["logs"][0]["typ"], "C:run");
    let output = command()
        .args([
            "--filter",
            "text:fetch",
            "process",
            file.to_str().unwrap(),
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["metadata"]["total_entries"], 1);
    assert!(report["logs"][0]["msg"].as_str().unwrap().contains("fetch"));
}

#[test]
fn process_timestamps_preserve_instants_across_timezones_dst_and_midnight() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("clock.jsonl");
    let instants = [
        ("2026-01-01T00:00:02Z", "2026-01-01T00:00:02.000Z"),
        ("2026-01-01T00:30:00+02:00", "2025-12-31T22:30:00.000Z"),
        ("2026-03-29T01:59:59.999+01:00", "2026-03-29T00:59:59.999Z"),
        ("2026-03-29T03:00:00+02:00", "2026-03-29T01:00:00.000Z"),
        ("2026-10-25T02:30:00+02:00", "2026-10-25T00:30:00.000Z"),
        ("2026-10-25T02:30:00+01:00", "2026-10-25T01:30:00.000Z"),
    ];
    write_file(
        &file,
        &instants
            .iter()
            .map(|(input, _)| {
                serde_json::json!({"timestamp": input, "message": "sample"}).to_string() + "\n"
            })
            .collect::<String>(),
    );
    let mut expected = instants.iter().map(|(_, utc)| *utc).collect::<Vec<_>>();
    expected.sort();
    for timezone in ["UTC", "Europe/Bratislava", "America/New_York"] {
        let output = command()
            .args(["process", file.to_str().unwrap(), "--limit", "0"])
            .env("TZ", timezone)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let actual = report["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["ts"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "timezone: {timezone}");
        assert_eq!(
            report["metadata"]["time_range"]["start"],
            *expected.first().unwrap()
        );
        assert_eq!(
            report["metadata"]["time_range"]["end"],
            *expected.last().unwrap()
        );
    }
}

#[test]
fn perf_json_and_text_apply_selection_with_full_totals() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("requests.log");
    write_file(
        &file,
        concat!(
            "core (demo) | 2026-01-01T00:00:00.000Z [INFO] Request \"slow\" [0--first] sent\n",
            "core (demo) | 2026-01-01T00:00:02.000Z [INFO] Request \"slow\" [0--first] completed\n",
            "core (demo) | 2026-01-01T00:00:03.000Z [INFO] Request \"fast\" [0--second] sent\n",
            "core (demo) | 2026-01-01T00:00:03.010Z [INFO] Request \"fast\" [0--second] completed\n",
            "core (demo) | 2026-01-01T00:00:04.000Z [INFO] Request \"fast\" [0--third] sent\n",
            "core (demo) | 2026-01-01T00:00:04.010Z [INFO] Request \"fast\" [0--third] completed\n",
            "core (demo) | 2026-01-01T00:00:05.000Z [INFO] Request \"pending\" [0--fourth] sent\n",
            "core (demo) | 2026-01-01T00:00:06.000Z [INFO] Request \"pending2\" [0--fifth] sent\n",
        ),
    );
    for (sort, selected_name) in [("duration", "slow"), ("count", "fast"), ("name", "fast")] {
        let output = command()
            .args([
                "--preset",
                "service-api",
                "-j",
                "perf",
                file.to_str().unwrap(),
                "--top-n",
                "1",
                "--sort-by",
                sort,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["operations"][0]["name"], selected_name);
        assert_eq!(report["stats"][0]["name"], selected_name);
        for section in ["operations", "stats", "orphans", "threshold_violations"] {
            assert_eq!(report[section].as_array().unwrap().len(), 1, "{section}");
        }
        assert_eq!(report["totals"]["operations"], 3);
        assert_eq!(report["totals"]["stats"], 2);
        assert_eq!(report["totals"]["orphans"], 2);
        assert_eq!(report["omitted"]["operations"], 2);
        assert_eq!(report["omitted"]["stats"], 1);
        assert_eq!(report["omitted"]["orphans"], 1);
        let output = command()
            .args([
                "--preset",
                "service-api",
                "perf",
                file.to_str().unwrap(),
                "--top-n",
                "1",
                "--sort-by",
                sort,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("Full totals: 3 completed operations, 2 statistics groups, 2 orphans, 1 threshold violations"));
        assert!(text.contains("Omitted: 2 completed operations, 1 statistics groups, 1 orphans, 0 threshold violations"));
        assert!(text.contains(&format!("1. [Request] {selected_name} -")));
        assert!(!text.contains("pending2"));
    }
    let output = command()
        .args([
            "--preset",
            "service-api",
            "-j",
            "perf",
            file.to_str().unwrap(),
            "--orphans-only",
            "--top-n",
            "1",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["operations"].as_array().unwrap().is_empty());
    assert!(report["stats"].as_array().unwrap().is_empty());
    assert!(
        report["threshold_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(report["orphans"].as_array().unwrap().len(), 1);
    assert_eq!(report["totals"]["operations"], 3);
    assert_eq!(report["omitted"]["operations"], 3);
    let output = command()
        .args([
            "--preset",
            "service-api",
            "-j",
            "perf",
            file.to_str().unwrap(),
            "--top-n",
            "0",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operations"].as_array().unwrap().len(), 3);
    for section in ["operations", "stats", "orphans", "threshold_violations"] {
        assert_eq!(report["omitted"][section], 0);
    }
    for option in [["--sort-by", "name"], ["--threshold-ms", "1000"]] {
        let output = command()
            .args(["perf", file.to_str().unwrap(), "--orphans-only"])
            .args(option)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    }
}

#[test]
fn perf_scopes_reused_ids_and_preserves_ambiguous_events_with_sources() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("collision.log");
    write_file(
        &file,
        concat!(
            "core (context-a) | 2026-01-01T00:00:00.000Z [INFO] Request \"fetch\" [0--same] sent\n",
            "core (context-b) | 2026-01-01T00:00:01.000Z [INFO] Request \"fetch\" [0--same] sent\n",
            "core (context-a) | 2026-01-01T00:00:02.000Z [INFO] Request \"fetch\" [0--same] completed\n",
            "core (context-b) | 2026-01-01T00:00:04.000Z [INFO] Request \"fetch\" [0--same] completed\n",
        ),
    );
    let output = command()
        .args([
            "--preset",
            "service-api",
            "-j",
            "perf",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let ops = report["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0]["duration_ms"], 3000);
    assert_eq!(ops[1]["duration_ms"], 2000);
    assert_eq!(ops[1]["scope"], serde_json::json!(["context-a"]));
    assert_eq!(ops[1]["start_source"]["line"], 1);
    assert_eq!(ops[1]["end_source"]["line"], 3);
    assert_eq!(ops[1]["start_source"]["file"], file.to_str().unwrap());
    assert!(report["unmatched_events"].as_array().unwrap().is_empty());
    let unscoped = fs::read_to_string(&file)
        .unwrap()
        .replace(" (context-a)", "")
        .replace(" (context-b)", "");
    write_file(
        &file,
        &(unscoped
            + "core | 2026-01-01T00:00:05.000Z [INFO] Request \"other\" [0--end] completed\n"),
    );
    let config = dir.path().join("unscoped.toml");
    let mut profile = log_analyzer::config::load_builtin_template("service-api").unwrap();
    profile.perf.correlation_scope_fields.clear();
    write_file(&config, &toml::to_string(&profile).unwrap());
    let output = command()
        .env_remove("LOG_ANALYZER_PRESET")
        .args([
            "--config",
            config.to_str().unwrap(),
            "-j",
            "perf",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["operations"].as_array().unwrap().is_empty());
    assert!(report["stats"].as_array().unwrap().is_empty());
    assert_eq!(report["orphans"].as_array().unwrap().len(), 2);
    assert_eq!(report["ambiguous_groups"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["ambiguous_groups"][0]["events"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let unmatched = report["unmatched_events"].as_array().unwrap();
    assert_eq!(unmatched.len(), 5);
    assert_eq!(unmatched[0]["reason"], "overlapping_starts");
    assert_eq!(unmatched[4]["reason"], "missing_start");
    let output = command()
        .env_remove("LOG_ANALYZER_PRESET")
        .args([
            "--config",
            config.to_str().unwrap(),
            "perf",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 ambiguous groups, 5 unmatched events")
    );
}

#[test]
fn perf_custom_composite_scope_pairs_across_files_and_reports_missing_fields() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("scope.toml");
    let mut profile = log_analyzer::config::load_builtin_template("service-api").unwrap();
    profile.perf.correlation_scope_fields = vec!["component_id".to_string(), "tenant".to_string()];
    write_file(&config, &toml::to_string(&profile).unwrap());
    let start = dir.path().join("start.jsonl");
    let end = dir.path().join("end.jsonl");
    write_file(
        &start,
        concat!(
            "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"session_id\":\"demo\",\"tenant\":\"a\",\"message\":\"Request \\\"fetch\\\" [0--same] sent\"}\n",
            "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"session_id\":\"demo\",\"tenant\":\"b\",\"message\":\"Request \\\"fetch\\\" [0--same] sent\"}\n",
        ),
    );
    write_file(
        &end,
        concat!(
            "{\"timestamp\":\"2026-01-01T00:00:02Z\",\"session_id\":\"demo\",\"tenant\":\"a\",\"message\":\"Request \\\"fetch\\\" [0--same] completed\"}\n",
            "{\"timestamp\":\"2026-01-01T00:00:04Z\",\"session_id\":\"demo\",\"tenant\":\"b\",\"message\":\"Request \\\"fetch\\\" [0--same] completed\"}\n",
            "{\"timestamp\":\"2026-01-01T00:00:05Z\",\"session_id\":\"demo\",\"message\":\"Request \\\"fetch\\\" [0--missing] sent\"}\n",
        ),
    );
    let output = command()
        .env_remove("LOG_ANALYZER_PRESET")
        .args([
            "--config",
            config.to_str().unwrap(),
            "-j",
            "perf",
            end.to_str().unwrap(),
            start.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operations"].as_array().unwrap().len(), 2);
    assert_eq!(report["operations"][0]["duration_ms"], 3000);
    assert_eq!(report["operations"][1]["duration_ms"], 2000);
    assert_eq!(
        report["operations"][1]["start_source"]["file"],
        start.to_str().unwrap()
    );
    assert_eq!(
        report["operations"][1]["end_source"]["file"],
        end.to_str().unwrap()
    );
    assert_eq!(
        report["unmatched_events"][0]["reason"],
        "missing_scope_field"
    );
}

#[test]
fn perf_rejects_missing_default_scope_even_for_unique_pairs() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unscoped.jsonl");
    write_file(
        &file,
        concat!(
            "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":\"Request \\\"fetch\\\" [0--same] sent\"}\n",
            "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":\"Request \\\"fetch\\\" [0--same] completed\"}\n",
        ),
    );
    let output = command()
        .args([
            "--preset",
            "service-api",
            "-j",
            "perf",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["operations"].as_array().unwrap().is_empty());
    assert_eq!(report["unmatched_events"].as_array().unwrap().len(), 2);
    assert!(
        report["unmatched_events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["reason"] == "missing_scope_field")
    );
}

#[test]
fn perf_uses_json_envelope_scope_alongside_embedded_payload() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("scope.toml");
    let mut profile = log_analyzer::config::load_builtin_template("service-api").unwrap();
    profile.perf.correlation_scope_fields = vec!["component_id".into(), "tenant".into()];
    write_file(&config, &toml::to_string(&profile).unwrap());
    let file = dir.path().join("envelope.jsonl");
    for envelope_field in ["payload", "fields"] {
        let rows = [
            serde_json::json!({"timestamp":"2026-01-01T00:00:00Z", "session_id":"demo", "message":"Request \"fetch\" [0--same] sent with body {\"attempt\":1}", envelope_field: {"tenant":"a"}}),
            serde_json::json!({"timestamp":"2026-01-01T00:00:01Z", "session_id":"demo", "message":"Request \"fetch\" [0--same] completed", envelope_field: {"tenant":"a"}}),
        ];
        write_file(
            &file,
            &rows
                .iter()
                .map(|row| row.to_string() + "\n")
                .collect::<String>(),
        );
        let output = command()
            .env_remove("LOG_ANALYZER_PRESET")
            .args([
                "--config",
                config.to_str().unwrap(),
                "-j",
                "perf",
                file.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report["operations"].as_array().unwrap().len(),
            1,
            "{envelope_field}"
        );
        assert_eq!(report["operations"][0]["duration_ms"], 1000);
        assert_eq!(
            report["operations"][0]["scope"],
            serde_json::json!(["demo", "a"])
        );
        assert!(report["unmatched_events"].as_array().unwrap().is_empty());
    }
}

#[test]
fn configured_timelines_are_available_in_perf_and_trace_text_and_json() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("timeline.log");
    let config = dir.path().join("timeline.toml");
    write_file(
        &file,
        "core (demo) | 2026-01-01T00:00:00Z [INFO] fetch begin id=a\ncore (demo) | 2026-01-01T00:00:02Z [INFO] fetch response id=a\n",
    );
    write_file(
        &config,
        r#"
[[timeline.events]]
name = "begin"
pattern = 'fetch begin id=(?P<id>\w+)'
correlation_fields = ["component_id", "id"]
[[timeline.events]]
name = "response"
pattern = 'fetch response id=(?P<id>\w+)'
correlation_fields = ["component_id", "id"]
[[timeline.pairs]]
name = "fetch"
start_event = "begin"
end_event = "response"
timing = "measured"
"#,
    );
    for subcommand in ["perf", "trace"] {
        for json in [true, false] {
            let mut cmd = command();
            cmd.env_remove("LOG_ANALYZER_PRESET")
                .args(["--config", config.to_str().unwrap()]);
            if json {
                cmd.arg("-j");
            }
            cmd.args([subcommand, file.to_str().unwrap()]);
            if subcommand == "trace" {
                cmd.args(["--session", "demo"]);
            }
            let output = cmd.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            if json {
                let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                let timeline = if subcommand == "trace" {
                    &value["trace"]["event_timeline"]
                } else {
                    &value["event_timeline"]
                };
                assert_eq!(timeline["intervals"][0]["measured_duration_ms"], 2000);
                assert_eq!(
                    timeline["events"][0]["source"]["file"],
                    file.to_str().unwrap()
                );
                assert_eq!(timeline["sample_counts"]["response"], 1);
            } else {
                assert!(
                    String::from_utf8_lossy(&output.stdout)
                        .contains("EVENT TIMELINE: boundaries_available")
                );
            }
        }
    }
}

#[test]
fn normalization_reports_skipped_rows_and_schema_preview_without_guessing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("wrapped.jsonl");
    let config = dir.path().join("normalize.toml");
    write_file(
        &file,
        "[\"2026-01-01T00:00:01+05:30\",{\"info\":{\"event\":\"heartbeat\",\"processId\":\"demo\"}}]\n[null,{\"info\":{\"event\":\"heartbeat\"}}]\n",
    );
    write_file(
        &config,
        "[normalization.fields]\ntimestamp='/0'\nmessage='/1/info/event'\npayload='/1/info'\n",
    );
    let output = command()
        .env_remove("LOG_ANALYZER_PRESET")
        .args([
            "--config",
            config.to_str().unwrap(),
            "-j",
            "info",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["coverage"]["parsed_entries"], 1);
    assert_eq!(
        value["coverage"]["files"][0]["normalization_diagnostics"][0]["reason"],
        "null_field"
    );
    assert_eq!(
        value["coverage"]["files"][0]["normalization_diagnostics"][0]["line"],
        2
    );
    let output = command()
        .args(["schema", file.to_str().unwrap(), "--samples", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["schema_preview"]["samples"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        value["schema_preview"]["samples"][0]["paths"]["/0"],
        "string"
    );
    assert_eq!(value["schema_preview"]["json_strings_decoded"], false);
    let output = command()
        .env_remove("LOG_ANALYZER_PRESET")
        .args([
            "--config",
            config.to_str().unwrap(),
            "process",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("null_field"));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["logs"][0]["source_line"], 1);
    assert_eq!(value["logs"][0]["source_row_path"], "");
}

#[test]
fn search_preserves_expanded_row_paths_in_both_output_formats() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("rows.jsonl");
    let config = dir.path().join("rows.toml");
    write_file(
        &file,
        "{\"rows\":[{\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":\"first\"},{\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":\"needle\"}]}",
    );
    write_file(
        &config,
        "[normalization]\nroot_path='/rows'\nexpand_rows=true\n",
    );
    for json in [true, false] {
        let mut cmd = command();
        cmd.env_remove("LOG_ANALYZER_PRESET")
            .args(["--config", config.to_str().unwrap()]);
        if json {
            cmd.arg("-j");
        }
        let output = cmd
            .args(["search", file.to_str().unwrap(), "--filter", "text:needle"])
            .output()
            .unwrap();
        assert!(output.status.success());
        if json {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["search"]["entries"][0]["source_line_number"], 1);
            assert_eq!(value["search"]["entries"][0]["source_row_path"], "/rows/1");
        } else {
            assert!(String::from_utf8_lossy(&output.stdout).contains("row: /rows/1"));
        }
    }
}
