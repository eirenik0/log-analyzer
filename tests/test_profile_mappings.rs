use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;
fn run(args: &[&str]) -> (Value, Output) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    command.args(args);
    for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("LOG_ANALYZER")) {
        command.env_remove(key);
    }
    let output = command.output().unwrap();
    if output.stdout.is_empty() && !output.status.success() {
        return (Value::Null, output);
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr)));
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}\n{value}");
    (value, output)
}
fn profile(dir: &Path, name: &str, scope: &str) -> String {
    let path = dir.join(format!("{name}.toml"));
    let field = |key: &str| json!({"from":"field","field":key});
    let rules:Vec<_>=["start","end"].iter().map(|phase|json!({"id":phase,"adapter":{"type":"structured","conditions":[{"field":"phase","equals":phase}]},"mapping":{"kind":"request","name":field("operation"),"phase":{"from":"literal","value":phase},"correlation_id":field("id"),"scope":[field(scope)]}})).collect();
    fs::write(&path,toml::to_string(&json!({"extends":"base","profile_name":name,"parser":{"format":"json-lines"},"event_rules":{"version":2,"rules":rules}})).unwrap()).unwrap();
    path.to_str().unwrap().into()
}
fn fixture(dir: &Path) -> String {
    let path = dir.join("sample with spaces.jsonl");
    let rows:Vec<_>=["start","end"].iter().enumerate().map(|(i,phase)|json!({"ts":format!("2026-01-01T00:00:0{i}+02:00"),"level":"INFO","component":"worker","component_id":"a","message":"generic","phase":phase,"operation":"work","id":"shared","session":"a","tenant":"b"})).collect();
    fs::write(
        &path,
        rows.iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    path.to_str().unwrap().into()
}
fn facts(dir: &Path, scope: &str) -> String {
    let path = dir.join("expected.json");
    let records:Vec<_>=["start","end"].iter().enumerate().map(|(i,phase)|json!({"source":{"input":0,"line":i+1,"row_path":null},"checks":{"/status":"event","/semantics/kind":"request","/semantics/name":"work","/semantics/phase":phase,"/semantics/correlation_id":"shared","/semantics/scope":[scope],"/semantics/end_expected":true}})).collect();
    fs::write(&path,json!({"version":1,"records":records,"pairs":[{"start":{"input":0,"line":1,"row_path":null},"end":{"input":0,"line":2,"row_path":null},"duration_ms":1000}]}).to_string()).unwrap();
    path.to_str().unwrap().into()
}
fn manage(
    root: &Path,
    p: &str,
    action: &str,
    file: &str,
    f: &str,
    extra: &[&str],
) -> (Value, Output) {
    let mut args = vec![
        "--config",
        p,
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        action,
        file,
        "--kind",
        "request",
        "--expected",
        f,
    ];
    args.extend_from_slice(extra);
    run(&args)
}
fn resolve(root: &Path, file: &str, f: &str) -> (Value, Output) {
    run(&[
        "resolve-profile",
        file,
        "--kind",
        "request",
        "--expected",
        f,
        "--project-root",
        root.to_str().unwrap(),
        "--user-mappings",
        root.join("absent-user.json").to_str().unwrap(),
    ])
}
#[test]
fn remember_reuse_move_replace_and_forget_without_private_evidence() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project with spaces");
    fs::create_dir(&root).unwrap();
    let file = fixture(&root);
    let p = profile(&root, "candidate", "session");
    let f = facts(&root, "a");
    let (view, o) = manage(&root, &p, "remember", &file, &f, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let entry = &view["profile_mappings"]["entries"][0];
    let id = entry["entry"]["id"].as_str().unwrap().to_owned();
    let registry = root.join(".log-analyzer/profile-mappings.json");
    let saved = fs::read_to_string(&registry).unwrap();
    assert!(!saved.contains("generic"));
    assert!(!saved.contains("2026-01-01"));
    assert!(!saved.contains("duration_ms"));
    assert!(!saved.contains(root.to_str().unwrap()));
    assert_eq!(entry["entry"]["profile"]["config"], "candidate.toml");
    let (v, o) = resolve(&root, &file, &f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        v["profile_resolution"]["selection_provenance"],
        "project_mapping"
    );
    assert_eq!(
        v["profile_resolution"]["mappings"][0]["revalidation"]["status"],
        "revalidated"
    );
    assert_eq!(
        v["profile_resolution"]["mappings"][0]["status"],
        "evaluated"
    );
    assert_eq!(v["profile_resolution"]["mappings"][0]["eligible"], true);
    assert_eq!(
        v["profile_resolution"]["mappings"][0]["semantic_status"],
        "sufficient_on_assertion_covered_sample"
    );
    // A new project location retains root-relative source and profile matching.
    let moved = dir.path().join("moved project");
    fs::rename(&root, &moved).unwrap();
    let file = moved.join("sample with spaces.jsonl");
    let p = moved.join("candidate.toml");
    let f = moved.join("expected.json");
    let (v, o) = resolve(&moved, file.to_str().unwrap(), f.to_str().unwrap());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        v["profile_resolution"]["selection_provenance"],
        "project_mapping"
    );
    let (_, o) = manage(
        &moved,
        p.to_str().unwrap(),
        "remember",
        file.to_str().unwrap(),
        f.to_str().unwrap(),
        &[],
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("already remembered"));
    let before = fs::read(moved.join(".log-analyzer/profile-mappings.json")).unwrap();
    let (_, o) = manage(
        &moved,
        p.to_str().unwrap(),
        "replace",
        file.to_str().unwrap(),
        f.to_str().unwrap(),
        &["--entry-id", &id, "--if-digest", &"0".repeat(64)],
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        before,
        fs::read(moved.join(".log-analyzer/profile-mappings.json")).unwrap()
    );
    let (_, o) = manage(
        &moved,
        p.to_str().unwrap(),
        "replace",
        file.to_str().unwrap(),
        f.to_str().unwrap(),
        &[
            "--entry-id",
            &id,
            "--if-digest",
            entry["digest"].as_str().unwrap(),
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (view, o) = run(&[
        "profile-mappings",
        "--project-root",
        moved.to_str().unwrap(),
        "inspect",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    fs::remove_file(&file).unwrap();
    fs::remove_file(&p).unwrap();
    let (_, o) = run(&[
        "profile-mappings",
        "--project-root",
        moved.to_str().unwrap(),
        "forget",
        "--entry-id",
        &id,
        "--if-digest",
        view["profile_mappings"]["entries"][0]["digest"]
            .as_str()
            .unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
#[test]
fn lookup_is_read_only_and_invalid_or_ambiguous_mappings_never_override_explicit_choices() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file = fixture(root);
    let p = profile(root, "candidate", "session");
    let f = facts(root, "a");
    let (_, o) = resolve(root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert!(!root.join(".log-analyzer").exists());
    let (_, o) = run(&[
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "inspect",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!root.join(".log-analyzer").exists());
    let (_, o) = manage(root, &p, "remember", &file, &f, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let registry = root.join(".log-analyzer/profile-mappings.json");
    let saved = fs::read(&registry).unwrap();
    let (_, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--project-root",
        root.to_str().unwrap(),
        "--user-mappings",
        root.join("absent-user.json").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(saved, fs::read(&registry).unwrap());
    let (unproved, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--project-root",
        root.to_str().unwrap(),
        "--user-mappings",
        root.join("absent-user.json").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        unproved["profile_resolution"]["mappings"][0]["status"],
        "evaluated"
    );
    assert_eq!(
        unproved["profile_resolution"]["mappings"][0]["eligible"],
        false
    );
    assert_eq!(
        unproved["profile_resolution"]["mappings"][0]["revalidation"]["status"],
        "revalidated"
    );
    let mut v: Value = serde_json::from_slice(&saved).unwrap();
    let duplicate = v["entries"][0].clone();
    v["entries"].as_array_mut().unwrap().push(duplicate);
    fs::write(&registry, v.to_string()).unwrap();
    let (v, o) = resolve(root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["profile_resolution"]["status"], "ambiguous");
    let (v, o) = run(&[
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
        "--project-root",
        root.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(v["profile_resolution"]["selection_provenance"], "explicit");
    fs::write(&registry, &saved).unwrap();
    let config = fs::read_to_string(&p).unwrap();
    fs::write(&p, config.replace("session", "tenant")).unwrap();
    let (v, o) = resolve(root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        v["profile_resolution"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["origin"] == "project_mapping" && c["status"] == "invalid_configuration")
    );
    assert_eq!(saved, fs::read(&registry).unwrap());
    fs::remove_file(&p).unwrap();
    let (_, o) = resolve(root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(saved, fs::read(&registry).unwrap());
    fs::write(&registry, "{malformed\n").unwrap();
    let malformed = fs::read(&registry).unwrap();
    let (_, o) = run(&[
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "inspect",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(malformed, fs::read(&registry).unwrap());
}
#[test]
fn native_at_prefixed_profile_paths_and_user_precedence_survive_round_trips() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file = fixture(root);
    let p = profile(root, "@candidate", "session");
    let f = facts(root, "a");
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .current_dir(root)
        .args([
            "--config",
            "@candidate.toml",
            "profile-mappings",
            "remember",
            &file,
            "--kind",
            "request",
            "--expected",
            &f,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stored: Value = serde_json::from_slice(
        &fs::read(root.join(".log-analyzer/profile-mappings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["entries"][0]["profile"]["config"], "@candidate.toml");
    assert!(stored["entries"][0]["profile"]["preset"].is_null());
    let user = root.join("user registry.json");
    let (_, o) = run(&[
        "--config",
        &p,
        "profile-mappings",
        "--scope",
        "user",
        "--project-root",
        root.to_str().unwrap(),
        "--registry",
        user.to_str().unwrap(),
        "remember",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let args = [
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
        "--project-root",
        root.to_str().unwrap(),
        "--user-mappings",
        user.to_str().unwrap(),
    ];
    let (v, o) = run(&args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        v["profile_resolution"]["selection_provenance"],
        "project_mapping"
    );
    let registry = root.join(".log-analyzer/profile-mappings.json");
    let mut duplicate = stored.clone();
    duplicate["entries"]
        .as_array_mut()
        .unwrap()
        .push(stored["entries"][0].clone());
    fs::write(&registry, duplicate.to_string()).unwrap();
    let (v, o) = run(&args);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["profile_resolution"]["status"], "ambiguous");
    fs::write(&registry, "{malformed\n").unwrap();
    let (v, o) = run(&args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        v["profile_resolution"]["selection_provenance"],
        "user_mapping"
    );
    let other = root.join("different project");
    fs::create_dir(&other).unwrap();
    let (v, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
        "--project-root",
        other.to_str().unwrap(),
        "--user-mappings",
        user.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_ne!(
        v["profile_resolution"]["selection_provenance"],
        "user_mapping"
    );
}
#[cfg(unix)]
#[test]
fn registry_alias_mutations_and_escaped_project_profiles_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    let file = fixture(&root);
    let p = profile(&root, "candidate", "session");
    let f = facts(&root, "a");
    let (_, o) = manage(&root, &p, "remember", &file, &f, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let registry = root.join(".log-analyzer/profile-mappings.json");
    let saved = fs::read(&registry).unwrap();
    let alias = root.join("alias.json");
    symlink(&registry, &alias).unwrap();
    let view: Value = serde_json::from_slice(&saved).unwrap();
    let inspect: Value = run(&[
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "inspect",
    ])
    .0;
    let (_, o) = run(&[
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "--registry",
        alias.to_str().unwrap(),
        "forget",
        "--entry-id",
        view["entries"][0]["id"].as_str().unwrap(),
        "--if-digest",
        inspect["profile_mappings"]["entries"][0]["digest"]
            .as_str()
            .unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("symlinks"));
    assert_eq!(saved, fs::read(&registry).unwrap());
    let outside = dir.path().join("outside.toml");
    fs::rename(&p, &outside).unwrap();
    symlink(&outside, &p).unwrap();
    let (v, o) = resolve(&root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        v["profile_resolution"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["origin"] == "project_mapping" && c["status"] == "invalid_configuration")
    );
}
#[test]
fn saving_requires_current_independent_proof_and_reuse_reports_changed_structure_and_contracts() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file = fixture(root);
    let p = profile(root, "candidate", "session");
    let f = facts(root, "a");
    let full_facts = fs::read(&f).unwrap();
    let mut partial: Value = serde_json::from_slice(&full_facts).unwrap();
    partial["pairs"] = json!([]);
    fs::write(&f, partial.to_string()).unwrap();
    let (_, o) = manage(root, &p, "remember", &file, &f, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!root.join(".log-analyzer").exists());
    fs::write(&f, &full_facts).unwrap();
    let (_, o) = manage(root, &p, "remember", &file, &f, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let registry = root.join(".log-analyzer/profile-mappings.json");
    let saved = fs::read(&registry).unwrap();
    let rows = fs::read_to_string(&file).unwrap();
    fs::write(&file,format!("{rows}2026-01-01 00:00:02,123 ERROR python.module unsupported\nTraceback (most recent call last):\n")).unwrap();
    let (v, o) = resolve(root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        v["profile_resolution"]["mappings"][0]["revalidation"]["status"],
        "invalid"
    );
    assert_eq!(saved, fs::read(&registry).unwrap());
    fs::write(&file, rows).unwrap();
    let mut stale: Value = serde_json::from_slice(&saved).unwrap();
    stale["entries"][0]["event_contract"] = json!(999);
    fs::write(&registry, stale.to_string()).unwrap();
    let (v, o) = resolve(root, &file, &f);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        v["profile_resolution"]["mappings"][0]["reason"],
        "mapping_contract_changed"
    );
    // Lower-priority lookup setup cannot block a valid supplied association.
    let association = root.join("association.json");
    let remembered: Value = serde_json::from_slice(&saved).unwrap();
    fs::write(&association,json!({"version":1,"profile":{"config":"candidate.toml","sha256":remembered["entries"][0]["profile"]["sha256"]},"sources":[{"file":file,"selected_parser":"json-lines"}],"event_contract":2,"structural_contract":1}).to_string()).unwrap();
    let (v, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
        "--association",
        association.to_str().unwrap(),
        "--project-root",
        root.join("missing-root").to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        v["profile_resolution"]["selection_provenance"],
        "association"
    );
    let (_, o) = run(&[
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
        "--project-root",
        root.join("missing-root").to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
#[test]
fn redacted_mapping_reports_keep_contract_types_and_hide_embedded_path_ids() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file = fixture(root);
    let original = profile(root, "candidate", "session");
    let p = root.join("prefixprivate-id.toml");
    fs::rename(original, &p).unwrap();
    let p = p.to_str().unwrap();
    let f = facts(root, "a");
    let rows = fs::read_to_string(&file)
        .unwrap()
        .lines()
        .map(|line| {
            let mut v: Value = serde_json::from_str(line).unwrap();
            v["component_id"] = json!("private-id");
            v["payload"] = json!({"mappings":"private-payload","selected":"private-selected"});
            v.to_string() + "\n"
        })
        .collect::<String>();
    fs::write(&file, rows).unwrap();
    let output = root.join("saved report.json");
    let (v, o) = run(&[
        "--redact",
        "--mask-id",
        "component_id",
        "--mask-id",
        "entries",
        "--output",
        output.to_str().unwrap(),
        "--config",
        p,
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "remember",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(v["profile_mappings"]["entries"].is_array());
    assert!(!String::from_utf8_lossy(&o.stdout).contains("private-id"));
    assert!(!fs::read_to_string(&output).unwrap().contains("private-id"));
    for field in ["mappings", "revalidation", "selected"] {
        let (v, o) = run(&[
            "--redact",
            "--mask-id",
            field,
            "--complete-output",
            "resolve-profile",
            &file,
            "--kind",
            "request",
            "--project-root",
            root.to_str().unwrap(),
            "--user-mappings",
            root.join("absent-user.json").to_str().unwrap(),
        ]);
        assert_eq!(o.status.code(), Some(1));
        assert!(v["profile_resolution"]["selected"].is_null());
        assert!(v["profile_resolution"]["mappings"].is_array());
    }
}
#[test]
fn resolution_v1_schema_remains_valid() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let p = profile(dir.path(), "candidate", "session");
    let (mut v, o) = run(&[
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    v["profile_resolution"]["version"] = json!(1);
    v["profile_resolution"]
        .as_object_mut()
        .unwrap()
        .remove("mappings");
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&v));
}
#[test]
fn management_reports_cannot_overwrite_the_registry_or_lock() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let registry = root.join(".log-analyzer/profile-mappings.json");
    let lock = registry.with_file_name("profile-mappings.json.lock");
    for output in [&registry, &lock] {
        let (_, o) = run(&[
            "--output",
            output.to_str().unwrap(),
            "profile-mappings",
            "--project-root",
            root.to_str().unwrap(),
            "inspect",
        ]);
        assert_eq!(o.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("conflicts"),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!root.join(".log-analyzer").exists());
    }
    let case_alias = root.join(".LOG-ANALYZER/PROFILE-MAPPINGS.JSON");
    let (_, o) = run(&[
        "--output",
        case_alias.to_str().unwrap(),
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "inspect",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!root.join(".log-analyzer").exists());
    assert!(!root.join(".LOG-ANALYZER").exists());
    let file = fixture(root);
    let p = profile(root, "candidate", "session");
    let f = facts(root, "a");
    let (_, o) = run(&[
        "--output",
        case_alias.to_str().unwrap(),
        "--config",
        &p,
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "remember",
        &file,
        "--kind",
        "request",
        "--expected",
        &f,
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(!root.join(".log-analyzer").exists());
    let (_, o) = manage(root, &p, "remember", &file, &f, &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let saved = fs::read(&registry).unwrap();
    for output in [&registry, &lock] {
        let (_, o) = run(&[
            "--output",
            output.to_str().unwrap(),
            "profile-mappings",
            "--project-root",
            root.to_str().unwrap(),
            "inspect",
        ]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(saved, fs::read(&registry).unwrap());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .current_dir(root)
        .args([
            "--output",
            ".log-analyzer/../.log-analyzer/profile-mappings.json",
            "profile-mappings",
            "inspect",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(saved, fs::read(&registry).unwrap());
    let empty = root.join("empty report.json");
    fs::write(&empty, []).unwrap();
    let (_, o) = run(&[
        "--output",
        empty.to_str().unwrap(),
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "inspect",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(saved, fs::read(&registry).unwrap());
    let hard = root.join("hard-report.json");
    fs::hard_link(&registry, &hard).unwrap();
    let (_, o) = run(&[
        "--output",
        hard.to_str().unwrap(),
        "profile-mappings",
        "--project-root",
        root.to_str().unwrap(),
        "inspect",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(saved, fs::read(&registry).unwrap());
    #[cfg(unix)]
    {
        let alias = root.join("alias-report.json");
        std::os::unix::fs::symlink(&registry, &alias).unwrap();
        let (_, o) = run(&[
            "--output",
            alias.to_str().unwrap(),
            "profile-mappings",
            "--project-root",
            root.to_str().unwrap(),
            "inspect",
        ]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(saved, fs::read(&registry).unwrap());
    }
}
