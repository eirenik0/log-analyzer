pub use crate::event_rules::CompiledEventRules;
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
const EMBEDDED_PROFILE_EYES: &str = include_str!("../config/profiles/eyes.toml");
const EMBEDDED_TEMPLATE_CUSTOM_START: &str = include_str!("../config/templates/custom-start.toml");
const EMBEDDED_TEMPLATE_SERVICE_API: &str = include_str!("../config/templates/service-api.toml");
const EMBEDDED_TEMPLATE_EVENT_PIPELINE: &str =
    include_str!("../config/templates/event-pipeline.toml");
const BUILTIN_TEMPLATE_NAMES: &[&str] = &[
    "base",
    "eyes",
    "custom-start",
    "service-api",
    "event-pipeline",
];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid event-rule configuration in '{path}': {reason}")]
    EventRules { path: String, reason: String },
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
    #[error("Unknown built-in preset '{name}'. Available built-ins: {available}")]
    UnknownBuiltin { name: String, available: String },
    #[error("Invalid `extends` in '{path}': {reason}")]
    Extends { path: String, reason: String },
}

/// Longest `extends` chain accepted, to keep resolution bounded.
const MAX_EXTENDS_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzerConfig {
    /// Free-form label for the loaded profile.
    pub profile_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_rules: Option<CompiledEventRules>,
    /// Deprecated command-only compatibility wrapper; legacy request/events remain available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_rules: Option<CompiledEventRules>,
    pub parser: ParserRules,
    pub perf: PerfRules,
    pub timeline: crate::timeline::TimelineRules,
    pub normalization: Option<crate::normalize::NormalizationRules>,
    pub profile: ProfileRules,
    #[serde(skip_serializing_if = "SessionsRules::is_empty")]
    pub sessions: SessionsRules,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            profile_name: "base".to_string(),
            event_rules: None,
            command_rules: None,
            parser: ParserRules::default(),
            perf: PerfRules::default(),
            timeline: crate::timeline::TimelineRules::default(),
            normalization: None,
            profile: ProfileRules::default(),
            sessions: SessionsRules::default(),
        }
    }
}

impl AnalyzerConfig {
    /// Global rules are preferred; command_rules remains a compatibility wrapper.
    pub fn event_classifier(&self) -> Option<&CompiledEventRules> {
        self.event_rules.as_ref().or(self.command_rules.as_ref())
    }

    pub fn command_classifier(&self) -> Option<&CompiledEventRules> {
        self.command_rules.as_ref().or_else(|| {
            self.event_rules.as_ref().filter(|rules| {
                rules
                    .schema()
                    .rules
                    .iter()
                    .all(|rule| rule.mapping.kind == OperationKind::Command)
            })
        })
    }

    /// Required after assembling a configuration programmatically; file loaders call this.
    pub fn validate_event_rules(&self) -> Result<(), String> {
        if self.event_classifier().is_some()
            && (self.perf.correlation_scope_fields.len() > crate::event_rules::MAX_SCOPE_FIELDS
                || self
                    .perf
                    .correlation_scope_fields
                    .iter()
                    .any(|s| s.len() > crate::event_rules::MAX_VALUE_BYTES))
        {
            return Err(
                "explicit correlation supports at most 16 scope field names of at most 4096 bytes"
                    .into(),
            );
        }
        if self.event_classifier().is_some()
            && (self.parser.command_payload_markers.len() > 16
                || self
                    .parser
                    .command_payload_markers
                    .iter()
                    .any(|marker| marker.len() > crate::event_rules::MAX_VALUE_BYTES))
        {
            return Err("explicit command decoding supports at most 16 command_payload_markers of at most 4096 bytes each".into());
        }
        if self.event_rules.is_some()
            && (self.parser.request_payload_markers.len() > 16
                || self
                    .parser
                    .request_payload_markers
                    .iter()
                    .any(|s| s.len() > crate::event_rules::MAX_VALUE_BYTES)
                || self.parser.event_payload_separator.len() > crate::event_rules::MAX_VALUE_BYTES)
        {
            return Err("explicit event decoding supports at most 16 request_payload_markers and payload markers of at most 4096 bytes each".into());
        }
        if let Some(rules) = &self.command_rules {
            if self.event_rules.is_some() {
                return Err("choose command_rules for staged command integration or event_rules for the global contract; both cannot coexist".into());
            }
            if rules
                .schema()
                .rules
                .iter()
                .any(|rule| rule.mapping.kind != OperationKind::Command)
            {
                return Err("command_rules accepts only kind = command; request/event integration is separate".into());
            }
            if !self.parser.command_prefix.is_empty()
                || !self.parser.command_start_marker.is_empty()
                || self
                    .perf
                    .command_start_markers
                    .iter()
                    .any(|s| !s.is_empty())
                || self
                    .perf
                    .command_completion_markers
                    .iter()
                    .any(|s| !s.is_empty())
            {
                return Err("command_rules cannot coexist with legacy command markers; remove parser.command_prefix, parser.command_start_marker, perf.command_start_markers and perf.command_completion_markers, or omit command_rules to retain legacy semantics".into());
            }
        }
        if self.event_rules.is_some()
            && (!self.parser.command_prefix.is_empty()
                || !self.parser.command_start_marker.is_empty()
                || !self.parser.request_prefix.is_empty()
                || self.parser.event_emit_markers.iter().any(|s| !s.is_empty())
                || self
                    .parser
                    .event_receive_markers
                    .iter()
                    .any(|s| !s.is_empty())
                || self
                    .parser
                    .request_send_markers
                    .iter()
                    .any(|s| !s.is_empty())
                || self
                    .parser
                    .request_receive_markers
                    .iter()
                    .any(|s| !s.is_empty())
                || self
                    .perf
                    .command_start_markers
                    .iter()
                    .any(|s| !s.is_empty())
                || self
                    .perf
                    .command_completion_markers
                    .iter()
                    .any(|s| !s.is_empty()))
        {
            return Err("explicit event_rules cannot coexist with legacy lifecycle markers; remove the parser event/command/request identity and phase markers and perf command phase markers, or omit event_rules".into());
        }
        Ok(())
    }

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

/// Version 1 has no implicit defaults for lifecycle semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRuleConfig {
    pub version: u32,
    pub rules: Vec<EventRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRule {
    pub id: String,
    pub adapter: Adapter,
    pub mapping: EventMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Adapter {
    Text {
        pattern: String,
        /// Validate a participating capture as a complete, bounded container suffix.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        complete_payload_capture: Option<String>,
    },
    Structured {
        conditions: Vec<FieldCondition>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldCondition {
    pub field: String,
    pub equals: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventMapping {
    pub kind: OperationKind,
    pub name: ValueMapping,
    pub phase: Option<ValueMapping>,
    pub outcome: Option<ValueMapping>,
    pub correlation_id: Option<ValueMapping>,
    pub direction: Option<ValueMapping>,
    pub endpoint: Option<ValueMapping>,
    #[serde(default)]
    pub scope: Vec<ValueMapping>,
    /// Version 2: `false` marks a start that has no end record by design.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_expected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValueMapping {
    Literal {
        value: String,
    },
    Field {
        field: String,
    },
    /// Explicit ordered alternatives; absence is allowed only for correlation identity.
    FirstField {
        fields: Vec<String>,
    },
    Capture {
        capture: String,
        #[serde(default)]
        decode: CaptureDecode,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureDecode {
    #[default]
    Raw,
    JsonString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Command,
    Request,
    Event,
}

impl OperationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Request => "Request",
            Self::Event => "Event",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
    #[default]
    Auto,
    #[serde(alias = "current", alias = "default")]
    Classic,
    RustTracing,
    Syslog,
    JsonLines,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ParserRules {
    pub format: LogFormat,
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
    pub module_depth: usize,
    pub module_strip_prefix: String,
}

impl Default for ParserRules {
    fn default() -> Self {
        Self {
            format: LogFormat::Auto,
            event_emit_markers: Vec::new(),
            event_receive_markers: Vec::new(),
            event_payload_separator: "payload".to_string(),
            command_prefix: String::new(),
            command_start_marker: String::new(),
            command_payload_markers: Vec::new(),
            request_prefix: String::new(),
            request_send_markers: Vec::new(),
            request_receive_markers: Vec::new(),
            request_payload_markers: Vec::new(),
            request_endpoint_marker: String::new(),
            json_indicators: Vec::new(),
            module_depth: 2,
            module_strip_prefix: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerfRules {
    pub command_start_markers: Vec<String>,
    pub command_completion_markers: Vec<String>,
    pub event_correlation_keys: Vec<String>,
    /// Composite scope fields; component_id scopes related split files by default.
    pub correlation_scope_fields: Vec<String>,
}

impl Default for PerfRules {
    fn default() -> Self {
        Self {
            command_start_markers: Vec::new(),
            command_completion_markers: Vec::new(),
            correlation_scope_fields: vec!["component_id".to_string()],
            event_correlation_keys: vec![
                "trace_id".to_string(),
                "traceId".to_string(),
                "request_id".to_string(),
                "requestId".to_string(),
                "id".to_string(),
                "key".to_string(),
            ],
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

pub fn load_config(
    path: Option<&Path>,
    preset: Option<&str>,
) -> Result<AnalyzerConfig, ConfigError> {
    if let Some(path) = path {
        load_config_from_path(path)
    } else if let Some(preset) = preset {
        load_builtin_template(preset).ok_or_else(|| ConfigError::UnknownBuiltin {
            name: preset.to_string(),
            available: builtin_template_names().join(", "),
        })
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

    parse_config_toml_in(&raw, &path_display, path.parent())
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
    let (source_path, raw) = builtin_source(&template_key)?;
    parse_config_toml(raw, source_path).ok()
}

/// Embedded source for a built-in name: `(display path, TOML text)`.
fn builtin_source(key: &str) -> Option<(&'static str, &'static str)> {
    Some(match key {
        "base" => ("embedded:config/profiles/base.toml", EMBEDDED_PROFILE_BASE),
        "eyes" => ("embedded:config/profiles/eyes.toml", EMBEDDED_PROFILE_EYES),
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
    })
}

fn parse_config_toml(raw: &str, path_display: &str) -> Result<AnalyzerConfig, ConfigError> {
    parse_config_toml_in(raw, path_display, None)
}

/// Parses a profile, resolving a top-level `extends` first.
///
/// `base_dir` is the directory of the profile file, used for relative parents.
fn parse_config_toml_in(
    raw: &str,
    path_display: &str,
    base_dir: Option<&Path>,
) -> Result<AnalyzerConfig, ConfigError> {
    // Same id form as parents (canonical path), so a cycle back to the root is found at once.
    let root_id = Path::new(path_display)
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path_display.to_string());
    let mut chain = vec![root_id];
    let value = resolve_extends(raw, path_display, base_dir, &mut chain)?;
    let config = value
        .try_into::<AnalyzerConfig>()
        .map_err(|source| ConfigError::Parse {
            path: path_display.to_string(),
            source,
        })?;
    config
        .validate_event_rules()
        .map_err(|reason| ConfigError::EventRules {
            path: path_display.to_string(),
            reason,
        })?;
    Ok(config)
}

/// Returns the profile as a TOML value with `extends` merged in and removed.
///
/// `extends` names a built-in (`base`, `eyes`, ...) or a TOML file path, relative to the
/// extending file. Tables merge key by key and the child wins; arrays and scalars are
/// replaced whole.
fn resolve_extends(
    raw: &str,
    path_display: &str,
    base_dir: Option<&Path>,
    chain: &mut Vec<String>,
) -> Result<toml::Value, ConfigError> {
    let mut value = toml::from_str::<toml::Value>(raw).map_err(|source| ConfigError::Parse {
        path: path_display.to_string(),
        source,
    })?;
    let extends_error = |reason: String| ConfigError::Extends {
        path: path_display.to_string(),
        reason,
    };
    let Some(table) = value.as_table_mut() else {
        return Ok(value);
    };
    let Some(parent_ref) = table.remove("extends") else {
        return Ok(value);
    };
    let parent_ref = parent_ref
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| extends_error("expected a non-empty string".to_string()))?
        .to_string();
    if chain.len() >= MAX_EXTENDS_DEPTH {
        return Err(extends_error(format!(
            "chain is longer than {MAX_EXTENDS_DEPTH} profiles"
        )));
    }

    let builtin = builtin_source(&parent_ref.to_ascii_lowercase());
    let (parent_id, parent_raw, parent_dir) = if let Some((id, text)) = builtin {
        (id.to_string(), text.to_string(), None)
    } else {
        let candidate = Path::new(&parent_ref);
        let path = match base_dir {
            Some(dir) if candidate.is_relative() => dir.join(candidate),
            _ => candidate.to_path_buf(),
        };
        let text = fs::read_to_string(&path).map_err(|err| {
            extends_error(format!(
                "'{parent_ref}' is not a built-in ({}) and cannot be read as a file: {err}",
                BUILTIN_TEMPLATE_NAMES.join(", ")
            ))
        })?;
        let id = path.canonicalize().unwrap_or_else(|_| path.clone());
        (
            id.display().to_string(),
            text,
            path.parent().map(Path::to_path_buf),
        )
    };
    if chain.contains(&parent_id) {
        return Err(extends_error(format!(
            "cycle: {} -> {parent_id}",
            chain.join(" -> ")
        )));
    }

    chain.push(parent_id.clone());
    let mut merged = resolve_extends(&parent_raw, &parent_id, parent_dir.as_deref(), chain)?;
    chain.pop();
    merge_toml(&mut merged, value);
    Ok(merged)
}

fn merge_toml(parent: &mut toml::Value, child: toml::Value) {
    match (parent, child) {
        (toml::Value::Table(parent), toml::Value::Table(child)) => {
            for (key, child_value) in child {
                match parent.get_mut(&key) {
                    Some(parent_value) => merge_toml(parent_value, child_value),
                    None => {
                        parent.insert(key, child_value);
                    }
                }
            }
        }
        (parent, child) => *parent = child,
    }
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
            LogEntryKind::Request { request, .. }
                if !known_requests.is_empty()
                    && !known_requests.contains(&request.to_lowercase()) =>
            {
                insights.unknown_requests.insert(request.clone());
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

    let (can_create, can_complete) = match &entry.classification {
        Some(crate::event_rules::ClassifiedRecord::Event {
            semantics,
            legacy: false,
            ..
        }) => (
            semantics.phase == Some(Phase::Start),
            // A start with no expected end is the whole operation, so it can complete too.
            (semantics.phase == Some(Phase::End) && semantics.outcome != Some(Outcome::Failure))
                || (semantics.phase == Some(Phase::Start) && !semantics.end_expected),
        ),
        Some(
            crate::event_rules::ClassifiedRecord::Conflict { .. }
            | crate::event_rules::ClassifiedRecord::Invalid { .. },
        ) => (false, false),
        _ => (true, true), // Preserve legacy hint semantics and caller-constructed records.
    };

    for matched in &matched_segments {
        let is_direct_session_target = matched.path_index == target_path_index;
        let is_create = can_create
            && is_direct_session_target
            && sessions.levels[matched.level_index]
                .config
                .create_command
                .as_deref()
                == Some(command.as_str());
        let is_complete = can_complete
            && sessions.levels[matched.level_index]
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
    use std::collections::HashMap;

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
            structured_fields: HashMap::new(),
            module_path: None,
            source_file: None,
            envelope_payload: None,
            classification: None,
            source_timestamp: None,
            timestamp_year_inferred: false,
            source_row_path: None,
            normalized_record: None,
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
            structured_fields: HashMap::new(),
            module_path: None,
            source_file: None,
            envelope_payload: None,
            classification: None,
            source_timestamp: None,
            timestamp_year_inferred: false,
            source_row_path: None,
            normalized_record: None,
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
    fn builtin_eyes_preset_loads_expected_session_levels() {
        let cfg = load_builtin_template("eyes").expect("eyes preset loads");

        assert_eq!(cfg.profile_name, "eyes");

        let levels = cfg.effective_session_levels();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0].name, "runner");
        assert_eq!(levels[0].segment_prefix, "manager-");
        assert_eq!(levels[1].name, "test");
        assert_eq!(levels[1].segment_prefix, "eyes-");
        assert_eq!(levels[2].name, "environment");
        assert_eq!(levels[2].segment_prefix, "environment-");
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
