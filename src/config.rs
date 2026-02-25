use crate::parser::{LogEntry, LogEntryKind};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use thiserror::Error;

const EMBEDDED_PROFILE_BASE: &str = include_str!("../config/profiles/base.toml");
const EMBEDDED_TEMPLATE_CUSTOM_START: &str = include_str!("../config/templates/custom-start.toml");
const EMBEDDED_TEMPLATE_SERVICE_API: &str = include_str!("../config/templates/service-api.toml");
const EMBEDDED_TEMPLATE_EVENT_PIPELINE: &str =
    include_str!("../config/templates/event-pipeline.toml");
const BUILTIN_TEMPLATE_NAMES: &[&str] = &["base", "custom-start", "service-api", "event-pipeline"];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse config file '{path}': {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzerConfig {
    /// Free-form label for the loaded profile.
    pub profile_name: String,
    pub parser: ParserRules,
    pub perf: PerfRules,
    pub profile: ProfileRules,
    #[serde(skip_serializing_if = "SessionsRules::is_empty")]
    pub sessions: SessionsRules,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            profile_name: "base".to_string(),
            parser: ParserRules::default(),
            perf: PerfRules::default(),
            profile: ProfileRules::default(),
            sessions: SessionsRules::default(),
        }
    }
}

impl AnalyzerConfig {
    pub fn has_profile_hints(&self) -> bool {
        !self.profile.known_components.is_empty()
            || !self.profile.known_commands.is_empty()
            || !self.profile.known_requests.is_empty()
            || !self.effective_session_levels().is_empty()
    }

    pub fn effective_session_levels(&self) -> Vec<SessionLevelConfig> {
        self.sessions.levels.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ParserRules {
    pub event_emit_markers: Vec<String>,
    pub event_receive_markers: Vec<String>,
    pub event_payload_separator: String,
    pub command_prefix: String,
    pub command_start_marker: String,
    pub command_payload_markers: Vec<String>,
    pub request_prefix: String,
    pub request_send_markers: Vec<String>,
    pub request_receive_markers: Vec<String>,
    pub request_payload_markers: Vec<String>,
    pub request_endpoint_marker: String,
    pub json_indicators: Vec<String>,
}

impl Default for ParserRules {
    fn default() -> Self {
        Self {
            event_emit_markers: vec!["Emit event of type".to_string()],
            event_receive_markers: vec!["Received event of type".to_string()],
            event_payload_separator: "with payload".to_string(),
            command_prefix: "Command \"".to_string(),
            command_start_marker: "\" is called".to_string(),
            command_payload_markers: vec!["with settings".to_string()],
            request_prefix: "Request \"".to_string(),
            request_send_markers: vec!["will be sent".to_string()],
            request_receive_markers: vec![
                "finished successfully".to_string(),
                "respond with".to_string(),
                "that was sent".to_string(),
                "is going to retried".to_string(),
            ],
            request_payload_markers: vec!["with body".to_string()],
            request_endpoint_marker: "address \"[".to_string(),
            json_indicators: vec![
                "with settings {".to_string(),
                "with body [".to_string(),
                "with body {".to_string(),
                "with body".to_string(),
                "with body ".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerfRules {
    pub command_start_markers: Vec<String>,
    pub command_completion_markers: Vec<String>,
    pub event_correlation_keys: Vec<String>,
}

impl Default for PerfRules {
    fn default() -> Self {
        Self {
            command_start_markers: vec!["is called".to_string()],
            command_completion_markers: vec![
                "finished".to_string(),
                "returned".to_string(),
                "completed".to_string(),
            ],
            event_correlation_keys: vec!["key".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProfileRules {
    pub known_components: Vec<String>,
    pub known_commands: Vec<String>,
    pub known_requests: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionsRules {
    pub levels: Vec<SessionLevelConfig>,
}

impl SessionsRules {
    fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLevelConfig {
    pub name: String,
    pub segment_prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub complete_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_fields: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileInsights {
    pub unknown_components: BTreeSet<String>,
    pub unknown_commands: BTreeSet<String>,
    pub unknown_requests: BTreeSet<String>,
    pub sessions: SessionInsights,
}

#[derive(Debug, Clone, Default)]
pub struct SessionInsights {
    pub levels: Vec<SessionLevelInsights>,
}

impl SessionInsights {
    pub fn from_configs(configs: Vec<SessionLevelConfig>) -> Self {
        Self {
            levels: configs
                .into_iter()
                .map(|config| SessionLevelInsights {
                    config,
                    sessions: BTreeMap::new(),
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.levels.iter().all(|level| level.sessions.is_empty())
    }

    pub fn level_session_ids(&self, level_index: usize) -> BTreeSet<String> {
        self.levels
            .get(level_index)
            .map(|l| l.sessions.keys().cloned().collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct SessionLevelInsights {
    pub config: SessionLevelConfig,
    pub sessions: BTreeMap<String, SessionInfo>,
}

impl SessionLevelInsights {
    pub fn completed_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|session| session.completed_via.is_some())
            .count()
    }

    pub fn incomplete_count(&self) -> usize {
        self.sessions.len().saturating_sub(self.completed_count())
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
    pub created_via: Option<String>,
    pub completed_via: Option<String>,
    pub parent: Option<String>,
    pub children: BTreeSet<String>,
    pub operation_counts: BTreeMap<String, usize>,
    pub entry_count: usize,
    pub summary_fields: BTreeMap<String, Value>,
}

impl SessionInfo {
    fn new(id: String, timestamp: DateTime<Local>) -> Self {
        Self {
            id,
            first_seen: timestamp,
            last_seen: timestamp,
            created_via: None,
            completed_via: None,
            parent: None,
            children: BTreeSet::new(),
            operation_counts: BTreeMap::new(),
            entry_count: 0,
            summary_fields: BTreeMap::new(),
        }
    }
}

pub fn contains_any_marker(text: &str, markers: &[String]) -> bool {
    markers
        .iter()
        .any(|marker| !marker.is_empty() && text.contains(marker))
}

pub fn load_config(path: Option<&Path>) -> Result<AnalyzerConfig, ConfigError> {
    if let Some(path) = path {
        load_config_from_path(path)
    } else {
        Ok(default_config().clone())
    }
}

pub fn load_config_from_path(path: &Path) -> Result<AnalyzerConfig, ConfigError> {
    let path_display = path.display().to_string();
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path_display.clone(),
        source,
    })?;

    parse_config_toml(&raw, &path_display)
}

pub fn default_config() -> &'static AnalyzerConfig {
    static DEFAULT_CONFIG: LazyLock<AnalyzerConfig> = LazyLock::new(|| {
        parse_config_toml(EMBEDDED_PROFILE_BASE, "embedded:config/profiles/base.toml")
            .unwrap_or_else(|err| panic!("Invalid embedded base config: {err}"))
    });
    &DEFAULT_CONFIG
}

pub fn builtin_template_names() -> &'static [&'static str] {
    BUILTIN_TEMPLATE_NAMES
}

pub fn load_builtin_template(name: &str) -> Option<AnalyzerConfig> {
    let template_key = normalized_template_key(name)?;
    let (source_path, raw) = match template_key.as_str() {
        "base" => ("embedded:config/profiles/base.toml", EMBEDDED_PROFILE_BASE),
        "custom-start" => (
            "embedded:config/templates/custom-start.toml",
            EMBEDDED_TEMPLATE_CUSTOM_START,
        ),
        "service-api" => (
            "embedded:config/templates/service-api.toml",
            EMBEDDED_TEMPLATE_SERVICE_API,
        ),
        "event-pipeline" => (
            "embedded:config/templates/event-pipeline.toml",
            EMBEDDED_TEMPLATE_EVENT_PIPELINE,
        ),
        _ => return None,
    };

    parse_config_toml(raw, source_path).ok()
}

fn parse_config_toml(raw: &str, path_display: &str) -> Result<AnalyzerConfig, ConfigError> {
    toml::from_str::<AnalyzerConfig>(raw).map_err(|source| ConfigError::Parse {
        path: path_display.to_string(),
        source,
    })
}

fn normalized_template_key(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let file_name = Path::new(trimmed)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(trimmed);
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or(file_name);

    Some(stem.to_ascii_lowercase())
}

pub fn analyze_profile(logs: &[LogEntry], cfg: &AnalyzerConfig) -> ProfileInsights {
    let mut insights = ProfileInsights {
        sessions: SessionInsights::from_configs(cfg.effective_session_levels()),
        ..ProfileInsights::default()
    };

    let known_components: HashSet<String> = cfg
        .profile
        .known_components
        .iter()
        .map(|v| v.to_lowercase())
        .collect();
    let known_commands: HashSet<String> = cfg
        .profile
        .known_commands
        .iter()
        .map(|v| v.to_lowercase())
        .collect();
    let known_requests: HashSet<String> = cfg
        .profile
        .known_requests
        .iter()
        .map(|v| v.to_lowercase())
        .collect();

    for entry in logs {
        if !known_components.is_empty()
            && !known_components.contains(&entry.component.to_lowercase())
        {
            insights.unknown_components.insert(entry.component.clone());
        }

        analyze_session_path(entry, &mut insights.sessions);

        match &entry.kind {
            LogEntryKind::Command { command, .. } => {
                if !known_commands.is_empty() && !known_commands.contains(&command.to_lowercase()) {
                    insights.unknown_commands.insert(command.clone());
                }
            }
            LogEntryKind::Request { request, .. } => {
                if !known_requests.is_empty() && !known_requests.contains(&request.to_lowercase()) {
                    insights.unknown_requests.insert(request.clone());
                }
            }
            _ => {}
        }
    }

    insights
}

#[derive(Debug, Clone)]
struct MatchedSessionSegment {
    path_index: usize,
    level_index: usize,
    session_id: String,
}

fn analyze_session_path(entry: &LogEntry, sessions: &mut SessionInsights) {
    if sessions.levels.is_empty() || entry.component_id.is_empty() {
        return;
    }

    let path_segments: Vec<&str> = entry
        .component_id
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if path_segments.is_empty() {
        return;
    }

    let mut matched_segments = Vec::new();
    for (path_index, segment) in path_segments.iter().enumerate() {
        let Some(level_index) = find_matching_session_level(segment, &sessions.levels) else {
            continue;
        };

        let level = &mut sessions.levels[level_index];
        let session = level
            .sessions
            .entry((*segment).to_string())
            .or_insert_with(|| SessionInfo::new((*segment).to_string(), entry.timestamp));

        if entry.timestamp < session.first_seen {
            session.first_seen = entry.timestamp;
        }
        if entry.timestamp > session.last_seen {
            session.last_seen = entry.timestamp;
        }
        session.entry_count += 1;

        matched_segments.push(MatchedSessionSegment {
            path_index,
            level_index,
            session_id: (*segment).to_string(),
        });
    }

    for pair in matched_segments.windows(2) {
        let parent = &pair[0];
        let child = &pair[1];

        if let Some(child_session) = sessions.levels[child.level_index]
            .sessions
            .get_mut(&child.session_id)
            && child_session.parent.as_ref() != Some(&parent.session_id)
        {
            child_session.parent = Some(parent.session_id.clone());
        }

        if let Some(parent_session) = sessions.levels[parent.level_index]
            .sessions
            .get_mut(&parent.session_id)
        {
            parent_session.children.insert(child.session_id.clone());
        }
    }

    for (path_index, segment) in path_segments.iter().enumerate() {
        if matched_segments.iter().any(|m| m.path_index == path_index) {
            continue;
        }

        let Some(parent_match) = matched_segments
            .iter()
            .rev()
            .find(|m| m.path_index < path_index)
        else {
            continue;
        };

        let op_type = strip_instance_suffix(segment);
        if op_type.is_empty() {
            continue;
        }

        if let Some(parent_session) = sessions.levels[parent_match.level_index]
            .sessions
            .get_mut(&parent_match.session_id)
        {
            *parent_session
                .operation_counts
                .entry(op_type.to_string())
                .or_insert(0) += 1;
        }
    }

    let LogEntryKind::Command { command, settings } = &entry.kind else {
        return;
    };

    let Some(target_path_index) = matched_segments.last().map(|m| m.path_index) else {
        return;
    };

    for matched in &matched_segments {
        let is_direct_session_target = matched.path_index == target_path_index;
        let is_create = is_direct_session_target
            && sessions.levels[matched.level_index]
                .config
                .create_command
                .as_deref()
                == Some(command.as_str());
        let is_complete = sessions.levels[matched.level_index]
            .config
            .complete_commands
            .iter()
            .any(|candidate| candidate == command);

        if !is_create && !is_complete {
            continue;
        }

        let create_summary_fields = if is_create {
            sessions.levels[matched.level_index]
                .config
                .summary_fields
                .clone()
        } else {
            Vec::new()
        };

        if let Some(session) = sessions.levels[matched.level_index]
            .sessions
            .get_mut(&matched.session_id)
        {
            if is_create {
                session.created_via = Some(command.clone());
                extract_summary_fields(session, settings.as_ref(), &create_summary_fields);
            }
            if is_complete {
                session.completed_via = Some(command.clone());
            }
        }
    }
}

fn find_matching_session_level(segment: &str, levels: &[SessionLevelInsights]) -> Option<usize> {
    let mut best_match: Option<(usize, usize)> = None;
    for (index, level) in levels.iter().enumerate() {
        let prefix = level.config.segment_prefix.as_str();
        if prefix.is_empty() || !segment.starts_with(prefix) {
            continue;
        }

        let prefix_len = prefix.len();
        match best_match {
            Some((_, best_len)) if best_len >= prefix_len => {}
            _ => best_match = Some((index, prefix_len)),
        }
    }

    best_match.map(|(index, _)| index)
}

fn strip_instance_suffix(segment: &str) -> &str {
    segment
        .rsplit_once('-')
        .map(|(base, _)| base)
        .unwrap_or(segment)
}

fn extract_summary_fields(
    session: &mut SessionInfo,
    settings: Option<&Value>,
    summary_fields: &[String],
) {
    let Some(settings) = settings else {
        return;
    };

    for field_path in summary_fields {
        if field_path.is_empty() {
            continue;
        }

        if let Some(value) = value_at_path(settings, field_path) {
            session
                .summary_fields
                .insert(field_path.clone(), value.clone());
        }
    }
}

fn value_at_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        if segment.is_empty() {
            return None;
        }

        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }

    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use serde_json::json;

    fn ts(rfc3339: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("valid timestamp")
            .with_timezone(&Local)
    }

    fn command_entry(
        component_id: &str,
        timestamp: &str,
        command: &str,
        settings: Option<Value>,
    ) -> LogEntry {
        LogEntry {
            component: "core".to_string(),
            component_id: component_id.to_string(),
            timestamp: ts(timestamp),
            level: "INFO".to_string(),
            message: format!("Command \"{command}\" is called"),
            raw_logline: String::new(),
            kind: LogEntryKind::Command {
                command: command.to_string(),
                settings,
            },
            source_line_number: 1,
        }
    }

    fn generic_entry(component_id: &str, timestamp: &str) -> LogEntry {
        LogEntry {
            component: "core".to_string(),
            component_id: component_id.to_string(),
            timestamp: ts(timestamp),
            level: "INFO".to_string(),
            message: "generic".to_string(),
            raw_logline: String::new(),
            kind: LogEntryKind::Generic { payload: None },
            source_line_number: 1,
        }
    }

    #[test]
    fn effective_session_levels_returns_configured_levels() {
        let cfg = AnalyzerConfig {
            sessions: SessionsRules {
                levels: vec![
                    SessionLevelConfig {
                        name: "level-1".to_string(),
                        segment_prefix: "manager-".to_string(),
                        create_command: None,
                        complete_commands: Vec::new(),
                        summary_fields: Vec::new(),
                    },
                    SessionLevelConfig {
                        name: "level-2".to_string(),
                        segment_prefix: "eyes-".to_string(),
                        create_command: None,
                        complete_commands: Vec::new(),
                        summary_fields: Vec::new(),
                    },
                ],
            },
            ..AnalyzerConfig::default()
        };

        let levels = cfg.effective_session_levels();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].name, "level-1");
        assert_eq!(levels[0].segment_prefix, "manager-");
        assert_eq!(levels[1].name, "level-2");
        assert_eq!(levels[1].segment_prefix, "eyes-");
    }

    #[test]
    fn parse_new_session_levels_config() {
        let raw = r#"
profile_name = "test"

[parser]
[perf]
[profile]

[[sessions.levels]]
name = "runner"
segment_prefix = "manager-"
create_command = "makeManager"
complete_commands = ["getResults", "closeBatch"]
summary_fields = ["concurrency", "batch.id"]
"#;

        let cfg = parse_config_toml(raw, "test.toml").expect("config parses");
        let levels = cfg.effective_session_levels();
        assert_eq!(levels.len(), 1);
        assert_eq!(levels[0].name, "runner");
        assert_eq!(levels[0].segment_prefix, "manager-");
        assert_eq!(levels[0].create_command.as_deref(), Some("makeManager"));
        assert_eq!(
            levels[0].complete_commands,
            vec!["getResults", "closeBatch"]
        );
        assert_eq!(levels[0].summary_fields, vec!["concurrency", "batch.id"]);
    }

    #[test]
    fn analyze_profile_builds_session_tree_and_lifecycle() {
        let cfg = AnalyzerConfig {
            sessions: SessionsRules {
                levels: vec![
                    SessionLevelConfig {
                        name: "runner".to_string(),
                        segment_prefix: "manager-".to_string(),
                        create_command: Some("makeManager".to_string()),
                        complete_commands: vec!["closeBatch".to_string()],
                        summary_fields: vec!["concurrency".to_string(), "batch.id".to_string()],
                    },
                    SessionLevelConfig {
                        name: "test".to_string(),
                        segment_prefix: "eyes-".to_string(),
                        create_command: Some("openEyes".to_string()),
                        complete_commands: vec!["close".to_string(), "abort".to_string()],
                        summary_fields: Vec::new(),
                    },
                ],
            },
            ..AnalyzerConfig::default()
        };

        let logs = vec![
            command_entry(
                "manager-1/makeManager-abc",
                "2026-01-01T00:00:00Z",
                "makeManager",
                Some(json!({"concurrency": 100, "batch": {"id": "batch-1"}})),
            ),
            command_entry(
                "manager-1/eyes-1/openEyes-rw2",
                "2026-01-01T00:00:01Z",
                "openEyes",
                None,
            ),
            generic_entry("manager-1/eyes-1/check-ufg-jdx", "2026-01-01T00:00:02Z"),
            command_entry(
                "manager-1/eyes-1/close-rw2",
                "2026-01-01T00:00:03Z",
                "close",
                None,
            ),
            command_entry(
                "manager-1/closeBatch-rw2",
                "2026-01-01T00:00:04Z",
                "closeBatch",
                None,
            ),
        ];

        let insights = analyze_profile(&logs, &cfg);

        let runner_level = &insights.sessions.levels[0];
        let runner = runner_level
            .sessions
            .get("manager-1")
            .expect("runner session");
        assert_eq!(runner.created_via.as_deref(), Some("makeManager"));
        assert_eq!(runner.completed_via.as_deref(), Some("closeBatch"));
        assert_eq!(runner.summary_fields.get("concurrency"), Some(&json!(100)));
        assert_eq!(
            runner.summary_fields.get("batch.id"),
            Some(&json!("batch-1"))
        );
        assert!(runner.children.contains("eyes-1"));
        assert_eq!(runner.operation_counts.get("makeManager"), Some(&1));
        assert_eq!(runner.operation_counts.get("closeBatch"), Some(&1));

        let test_level = &insights.sessions.levels[1];
        let test = test_level.sessions.get("eyes-1").expect("test session");
        assert_eq!(test.parent.as_deref(), Some("manager-1"));
        assert_eq!(test.created_via.as_deref(), Some("openEyes"));
        assert_eq!(test.completed_via.as_deref(), Some("close"));
        assert_eq!(test.operation_counts.get("openEyes"), Some(&1));
        assert_eq!(test.operation_counts.get("check-ufg"), Some(&1));
        assert_eq!(insights.sessions.level_session_ids(0).len(), 1);
        assert_eq!(insights.sessions.level_session_ids(1).len(), 1);
    }
}
