use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}
fn success(args: &[&str]) -> Value {
    let output = run(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn create(path: &Path, storage: &str, extra: &[&str]) -> Value {
    let mut args = vec![
        "--profile",
        "examples/investigations/profile.toml",
        "investigate",
        "examples/investigations/slow.jsonl",
        "--artifact",
        path.to_str().unwrap(),
        "--artifact-storage",
        storage,
        "--complete-output",
    ];
    args.extend_from_slice(extra);
    success(&args)
}
fn retrieve(path: &Path, report: &Value, extra: &[&str]) -> Value {
    let mut args = vec![
        "evidence",
        path.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    success(&args)["artifact_retrieval"].clone()
}

#[test]
fn indexed_collections_preserve_logical_evidence_and_nested_source_references() {
    let temp = tempfile::tempdir().unwrap();
    let json = temp.path().join("legacy.json");
    let db = temp.path().join("indexed.db");
    create(&json, "json", &[]);
    let report = create(&db, "sqlite", &[]);
    assert_eq!(report["artifact"]["status"], "complete");
    log_analyzer::investigation::validate_relations(&report, None).unwrap();
    let retained: Value = serde_json::from_slice(&fs::read(&json).unwrap()).unwrap();
    for key in [
        "findings",
        "records",
        "populations",
        "memberships",
        "sequences",
        "captured_inputs",
    ] {
        let result = retrieve(
            &db,
            &report,
            &["--collection", &format!("/{key}"), "--complete-output"],
        );
        assert_eq!(result["status"], "complete", "{key}");
        assert_eq!(result["items"], retained[key], "{key}");
        assert_eq!(result["parse_passes"], 0);
        assert_eq!(result["correlation_passes"], 0);
    }
    for (index, membership) in retained["memberships"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let result = retrieve(
            &db,
            &report,
            &[
                "--collection",
                &format!("/memberships/{index}/members"),
                "--complete-output",
            ],
        );
        assert_eq!(result["items"], membership["members"]);
    }
    let id = retained["records"][0]["occurrence"]["evidence_ref"]["reference_id"]
        .as_str()
        .unwrap();
    let result = retrieve(&db, &report, &["--collection", "/records", "--id", id]);
    assert_eq!(result["total"], 1);
    assert_eq!(result["items"][0], retained["records"][0]);
    let result = retrieve(&db, &report, &["--id", "no-such-id"]);
    assert_eq!(result["total"], 0);
    assert_eq!(result["status"], "complete");
}

#[test]
fn cursors_bind_to_artifact_and_selector_and_zero_item_pages_remain_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("indexed.db");
    let report = create(&db, "sqlite", &[]);
    let first = retrieve(
        &db,
        &report,
        &["--collection", "/records", "--report-max-items", "2"],
    );
    assert_eq!(first["total"], 6);
    assert_eq!(first["remaining"], 4);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = retrieve(
        &db,
        &report,
        &[
            "--collection",
            "/records",
            "--report-max-items",
            "2",
            "--report-cursor",
            cursor,
        ],
    );
    assert_eq!(second["prior"], 2);
    assert_eq!(second["remaining"], 2);
    assert_ne!(first["items"], second["items"]);
    let bad = run(&[
        "evidence",
        db.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--report-cursor",
        cursor,
    ]);
    assert!(!bad.status.success());
    assert!(bad.stdout.is_empty());
    let empty = retrieve(
        &db,
        &report,
        &["--collection", "/records", "--report-max-items", "0"],
    );
    assert_eq!(empty["status"], "item_limit_zero");
    assert_eq!(empty["remaining"], 6);
}

#[test]
fn integrity_and_explicit_record_cache_and_disk_limits_precede_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("indexed.db");
    let report = create(&db, "sqlite", &[]);
    let base = [
        "evidence",
        db.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
    ];
    for extra in [
        ["--evidence-record-max-bytes", "1"],
        ["--evidence-cache-max-bytes", "4096"],
        ["--artifact-max-bytes", "1"],
    ] {
        let output = run(&[base.as_slice(), extra.as_slice()].concat());
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE items SET value='{}' WHERE path='/findings' AND ordinal=0",
            [],
        )
        .unwrap();
    let output = run(&base);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest differs"));
    let limited = temp.path().join("limited.db");
    let report = create(&limited, "sqlite", &["--artifact-max-bytes", "1"]);
    assert_eq!(report["artifact"]["status"], "unavailable");
    assert!(!limited.exists());
    assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("journal")
    }));
}

#[test]
fn redaction_remains_bound_and_original_payloads_are_absent() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("redacted.db");
    let report = create(&db, "sqlite", &["--redact"]);
    let connection = Connection::open(&db).unwrap();
    let record: String = connection
        .query_row(
            "SELECT value FROM items WHERE path='/records' AND ordinal=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let record: Value = serde_json::from_str(&record).unwrap();
    assert!(record["raw_text"].is_null());
    assert_eq!(record["data_omitted"], true);
    let result = retrieve(&db, &report, &["--redact", "--verify-sources"]);
    assert_eq!(
        result["source_verification"]["inputs"][0]["status"],
        "unavailable"
    );
}

#[test]
fn indexed_page_memory_cap_paginates_without_discarding_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("indexed.db");
    let report = create(&db, "sqlite", &[]);
    let connection = Connection::open(&db).unwrap();
    let largest: i64 = connection
        .query_row(
            "SELECT max(length(CAST(value AS BLOB))) FROM items WHERE path='/records'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let cache = (largest * 32 + 4096).to_string();
    let mut cursor: Option<String> = None;
    let mut seen = Vec::new();
    loop {
        let mut args = vec![
            "--collection",
            "/records",
            "--complete-output",
            "--evidence-cache-max-bytes",
            &cache,
        ];
        if let Some(cursor) = &cursor {
            args.retain(|arg| *arg != "--complete-output");
            args.extend(["--report-cursor", cursor]);
        }
        let result = retrieve(&db, &report, &args);
        let items = result["items"].as_array().unwrap();
        assert!(!items.is_empty());
        seen.extend(items.iter().cloned());
        cursor = result["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
        assert_eq!(result["status"], "page");
    }
    assert_eq!(seen.len(), 6);
    let unique: std::collections::BTreeSet<_> =
        seen.iter().map(|v| v["occurrence"].to_string()).collect();
    assert_eq!(unique.len(), 6);
}

#[test]
fn indexed_chains_match_legacy_compact_views_and_keep_scoped_contrary_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut artifacts = Vec::new();
    for storage in ["json", "sqlite"] {
        let path = temp.path().join(format!("chains.{storage}"));
        let report = success(&[
            "--profile",
            "examples/investigations/chains/profile.toml",
            "investigate",
            "examples/investigations/chains/events.jsonl",
            "--artifact",
            path.to_str().unwrap(),
            "--artifact-storage",
            storage,
            "--complete-output",
        ]);
        artifacts.push((path, report));
    }
    let legacy = retrieve(
        &artifacts[0].0,
        &artifacts[0].1,
        &["--collection", "/chain-bundles", "--complete-output"],
    );
    assert!(legacy["total"].as_u64().unwrap() > 0);
    let indexed = retrieve(
        &artifacts[1].0,
        &artifacts[1].1,
        &["--collection", "/chain-bundles", "--complete-output"],
    );
    assert_eq!(indexed["items"], legacy["items"]);
    let id = legacy["items"][0]["id"].as_str().unwrap();
    let args = ["--collection", "/chain-bundles", "--id", id, "--compact"];
    assert_eq!(
        retrieve(&artifacts[0].0, &artifacts[0].1, &args)["items"],
        retrieve(&artifacts[1].0, &artifacts[1].1, &args)["items"]
    );
}

#[test]
fn changed_and_missing_sources_do_not_change_indexed_facts() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.jsonl");
    let db = temp.path().join("indexed.db");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/investigations/slow.jsonl"),
        &source,
    )
    .unwrap();
    let report = success(&[
        "--profile",
        "examples/investigations/profile.toml",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        db.to_str().unwrap(),
        "--artifact-storage",
        "sqlite",
    ]);
    let original = retrieve(&db, &report, &["--verify-sources"]);
    assert_eq!(
        original["source_verification"]["inputs"][0]["status"],
        "unchanged"
    );
    fs::write(&source, "changed synthetic source\n").unwrap();
    let changed = retrieve(&db, &report, &["--verify-sources"]);
    assert_eq!(
        changed["source_verification"]["inputs"][0]["status"],
        "changed"
    );
    assert_eq!(changed["items"], original["items"]);
    fs::remove_file(&source).unwrap();
    let missing = retrieve(&db, &report, &["--verify-sources"]);
    assert_eq!(
        missing["source_verification"]["inputs"][0]["status"],
        "missing"
    );
    assert_eq!(missing["items"], original["items"]);
}

#[test]
fn unsupported_storage_versions_and_cancelled_staging_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("indexed.db");
    create(&db, "sqlite", &[]);
    let connection = Connection::open(&db).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    let hash = log_analyzer::evidence::digest(&fs::read(&db).unwrap());
    let output = run(&["evidence", db.to_str().unwrap(), "--expected-sha256", &hash]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Unsupported indexed evidence storage version")
    );
    let cancelled = temp.path().join("cancelled.db");
    let cancel = temp.path().join("cancel");
    fs::write(&cancel, "").unwrap();
    let report = create(
        &cancelled,
        "sqlite",
        &["--cancel-file", cancel.to_str().unwrap()],
    );
    assert_eq!(report["processing"]["stop"]["reason"], "cancelled");
    assert_eq!(report["artifact"]["status"], "unavailable");
    assert!(!cancelled.exists());
}

#[test]
fn malformed_index_counts_cannot_panic_or_report_a_clean_empty_result() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("indexed.db");
    create(&db, "sqlite", &[]);
    Connection::open(&db)
        .unwrap()
        .execute("UPDATE collections SET count=0 WHERE path='/records'", [])
        .unwrap();
    let hash = log_analyzer::evidence::digest(&fs::read(&db).unwrap());
    let output = run(&[
        "evidence",
        db.to_str().unwrap(),
        "--expected-sha256",
        &hash,
        "--collection",
        "/records",
    ]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Malformed indexed collection count"));
}
