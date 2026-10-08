use log_analyzer::config::{self, ConfigError};
use std::fs;
use tempfile::tempdir;

#[test]
fn child_overrides_parent_and_inherits_the_rest() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("team.toml");
    fs::write(
        &path,
        r#"
extends = "base"
profile_name = "team"

[parser]
module_depth = 3

[profile]
known_components = ["api"]
"#,
    )
    .unwrap();

    let cfg = config::load_config_from_path(&path).unwrap();
    let base = config::load_builtin_template("base").unwrap();
    assert_eq!(cfg.profile_name, "team");
    assert_eq!(cfg.parser.module_depth, 3);
    assert_eq!(cfg.profile.known_components, ["api"]);
    assert_eq!(cfg.parser.format, base.parser.format);
    assert_eq!(
        cfg.perf.event_correlation_keys,
        base.perf.event_correlation_keys
    );
}

#[test]
fn omitted_profile_name_is_inherited() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("p.toml");
    fs::write(&path, "extends = \"eyes\"\n").unwrap();
    let cfg = config::load_config_from_path(&path).unwrap();
    let eyes = config::load_builtin_template("eyes").unwrap();
    assert_eq!(cfg.profile_name, "eyes");
    assert_eq!(cfg.profile.known_components, eyes.profile.known_components);
}

#[test]
fn arrays_are_replaced_and_tables_are_merged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("p.toml");
    fs::write(
        &path,
        r#"
extends = "eyes"
[profile]
known_components = ["only-this"]
"#,
    )
    .unwrap();
    let cfg = config::load_config_from_path(&path).unwrap();
    let eyes = config::load_builtin_template("eyes").unwrap();
    assert_eq!(cfg.profile.known_components, ["only-this"]);
    assert_eq!(cfg.profile.known_commands, eyes.profile.known_commands);
    assert_eq!(
        cfg.parser.request_endpoint_marker,
        eyes.parser.request_endpoint_marker
    );
}

#[test]
fn extends_a_file_relative_to_the_extending_file() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(
        dir.path().join("mid.toml"),
        "extends = \"base\"\nprofile_name = \"mid\"\n[perf]\nevent_correlation_keys = [\"k\"]\n",
    )
    .unwrap();
    fs::write(
        nested.join("leaf.toml"),
        "extends = \"../mid.toml\"\n[profile]\nknown_requests = [\"r\"]\n",
    )
    .unwrap();
    let cfg = config::load_config_from_path(&nested.join("leaf.toml")).unwrap();
    assert_eq!(cfg.profile_name, "mid");
    assert_eq!(cfg.perf.event_correlation_keys, ["k"]);
    assert_eq!(cfg.profile.known_requests, ["r"]);
}

#[test]
fn unknown_parent_cycle_and_bad_value_are_reported() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.toml");
    let b = dir.path().join("b.toml");
    fs::write(&a, "extends = \"b.toml\"\n").unwrap();
    fs::write(&b, "extends = \"a.toml\"\n").unwrap();
    let err = config::load_config_from_path(&a).unwrap_err();
    assert!(
        matches!(&err, ConfigError::Extends { reason, .. } if reason.contains("cycle")),
        "{err}"
    );

    fs::write(&a, "extends = \"nope\"\n").unwrap();
    let err = config::load_config_from_path(&a).unwrap_err();
    assert!(
        matches!(&err, ConfigError::Extends { reason, .. } if reason.contains("not a built-in")),
        "{err}"
    );

    fs::write(&a, "extends = 3\n").unwrap();
    let err = config::load_config_from_path(&a).unwrap_err();
    assert!(
        matches!(&err, ConfigError::Extends { reason, .. } if reason.contains("non-empty string")),
        "{err}"
    );
}

#[test]
fn invalid_child_values_still_fail_with_the_child_path() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("p.toml");
    fs::write(
        &path,
        "extends = \"base\"\n[parser]\nmodule_depth = \"deep\"\n",
    )
    .unwrap();
    let err = config::load_config_from_path(&path).unwrap_err();
    assert!(
        matches!(&err, ConfigError::Parse { path: p, .. } if p.ends_with("p.toml")),
        "{err}"
    );
}

#[test]
fn child_adds_event_rules_on_top_of_base() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("p.toml");
    fs::write(
        &path,
        r#"
extends = "base"
profile_name = "svc"

[event_rules]
version = 2

[[event_rules.rules]]
id = "call-start"
adapter = { type = "text", pattern = "Call (?P<n>\"\\w+\") begins" }

[event_rules.rules.mapping]
kind = "request"
name = { from = "capture", capture = "n", decode = "json_string" }
phase = { from = "literal", value = "start" }
correlation_id = { from = "literal", value = "a" }
"#,
    )
    .unwrap();
    let cfg = config::load_config_from_path(&path).unwrap();
    assert!(cfg.event_rules.is_some());
    assert_eq!(cfg.profile_name, "svc");
}

#[test]
fn self_extension_is_a_cycle() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("self.toml");
    fs::write(&path, "extends = \"self.toml\"\n").unwrap();
    let err = config::load_config_from_path(&path).unwrap_err();
    assert!(
        matches!(&err, ConfigError::Extends { reason, .. } if reason.contains("cycle")),
        "{err}"
    );
}
