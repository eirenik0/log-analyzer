use crate::parser::{LogEntry, LogEntryKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
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
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            profile_name: "base".to_string(),
            parser: ParserRules::default(),
            perf: PerfRules::default(),
            profile: ProfileRules::default(),
        }
    }
}

impl AnalyzerConfig {
    pub fn has_profile_hints(&self) -> bool {
        !self.profile.known_components.is_empty()
            || !self.profile.known_commands.is_empty()
            || !self.profile.known_requests.is_empty()
            || !self.profile.session_prefixes.primary.is_empty()
            || !self.profile.session_prefixes.secondary.is_empty()
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
    pub session_prefixes: SessionPrefixes,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionPrefixes {
    pub primary: String,
    pub secondary: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileInsights {
    pub unknown_components: BTreeSet<String>,
    pub unknown_commands: BTreeSet<String>,
    pub unknown_requests: BTreeSet<String>,
    pub primary_sessions: BTreeSet<String>,
    pub secondary_sessions: BTreeSet<String>,
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
    let mut insights = ProfileInsights::default();

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

        if !entry.component_id.is_empty() {
            for segment in entry.component_id.split('/') {
                if !cfg.profile.session_prefixes.primary.is_empty()
                    && segment.starts_with(&cfg.profile.session_prefixes.primary)
                {
                    insights.primary_sessions.insert(segment.to_string());
                }
                if !cfg.profile.session_prefixes.secondary.is_empty()
                    && segment.starts_with(&cfg.profile.session_prefixes.secondary)
                {
                    insights.secondary_sessions.insert(segment.to_string());
                }
            }
        }

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
