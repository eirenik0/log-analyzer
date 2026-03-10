use crate::config::{AnalyzerConfig, LogFormat, SessionLevelConfig};
use crate::parser::{LogEntry, LogEntryKind};
use std::collections::{BTreeMap, BTreeSet};

pub struct GenerateConfigOptions {
    pub profile_name: String,
}

pub fn generate_config(
    logs: &[LogEntry],
    base: &AnalyzerConfig,
    options: &GenerateConfigOptions,
) -> AnalyzerConfig {
    let mut config = base.clone();
    config.profile_name = options.profile_name.clone();

    let mut components = BTreeSet::new();
    let mut commands = BTreeSet::new();
    let mut requests = BTreeSet::new();
    let mut component_id_segments = BTreeSet::new();
    let mut prefix_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut module_paths = Vec::new();
    let mut module_roots = BTreeSet::new();

    for entry in logs {
        if !entry.component.is_empty() {
            components.insert(entry.component.clone());
        }
        if let Some(module_path) = &entry.module_path {
            module_paths.push(module_path.clone());
            if let Some(root) = module_path.split("::").next()
                && !root.is_empty()
            {
                module_roots.insert(root.to_string());
            }
        }

        if !entry.component_id.is_empty() {
            for (idx, segment) in entry.component_id.split('/').enumerate() {
                if !segment.is_empty() {
                    component_id_segments.insert(segment.to_string());

                    // Session hierarchy is encoded at the start of component_id paths.
                    if idx < 2
                        && let Some(prefix) = session_prefix(segment)
                    {
                        *prefix_counts.entry(prefix).or_default() += 1;
                    }
                }
            }
        }

        match &entry.kind {
            LogEntryKind::Command { command, .. } => {
                if !command.is_empty() {
                    commands.insert(command.clone());
                }
            }
            LogEntryKind::Request { request, .. } => {
                if !request.is_empty() {
                    requests.insert(request.clone());
                }
            }
            _ => {}
        }
    }

    let mut ranked_prefixes: Vec<(String, usize)> = prefix_counts
        .into_iter()
        .filter(|(prefix, count)| {
            *count > 1
                && component_id_segments
                    .iter()
                    .any(|segment| segment.starts_with(prefix))
        })
        .collect();

    // Deterministic ranking: most common first, lexical tiebreak.
    ranked_prefixes.sort_by(|(prefix_a, count_a), (prefix_b, count_b)| {
        count_b.cmp(count_a).then_with(|| prefix_a.cmp(prefix_b))
    });

    config.profile.known_components = components.into_iter().collect();
    config.profile.known_commands = commands.into_iter().collect();
    config.profile.known_requests = requests.into_iter().collect();
    let detected_prefixes: Vec<String> = ranked_prefixes
        .iter()
        .map(|(prefix, _)| prefix.clone())
        .collect();

    if config.sessions.levels.is_empty() {
        config.sessions.levels = detected_prefixes
            .into_iter()
            .enumerate()
            .map(|(index, segment_prefix)| SessionLevelConfig {
                name: generated_session_level_name(index),
                segment_prefix,
                create_command: None,
                complete_commands: Vec::new(),
                summary_fields: Vec::new(),
            })
            .collect();
    }

    if !module_paths.is_empty() {
        config.parser.format = LogFormat::RustTracing;
        config.parser.module_depth = inferred_module_depth(&module_paths);
        if let Some(prefix) = infer_module_strip_prefix(&module_roots) {
            config.parser.module_strip_prefix = prefix;
        }
    }

    config
}

fn generated_session_level_name(index: usize) -> String {
    format!("level-{}", index + 1)
}

fn session_prefix(segment: &str) -> Option<String> {
    let dash_index = segment.find('-')?;
    if dash_index == 0 {
        return None;
    }
    Some(segment[..=dash_index].to_string())
}

fn inferred_module_depth(module_paths: &[String]) -> usize {
    if module_paths.iter().any(|path| {
        path.split("::")
            .filter(|segment| !segment.is_empty())
            .count()
            >= 2
    }) {
        2
    } else {
        1
    }
}

fn infer_module_strip_prefix(module_roots: &BTreeSet<String>) -> Option<String> {
    let mut roots = module_roots.iter();
    let first = roots.next()?;
    let mut prefix = first.clone();

    for root in roots {
        let common_len = prefix
            .chars()
            .zip(root.chars())
            .take_while(|(a, b)| a == b)
            .count();
        prefix = prefix.chars().take(common_len).collect();
        if prefix.is_empty() {
            return None;
        }
    }

    let underscore_index = prefix.rfind('_')?;
    let candidate = &prefix[..=underscore_index];
    (candidate.len() >= 3).then(|| candidate.to_string())
}
