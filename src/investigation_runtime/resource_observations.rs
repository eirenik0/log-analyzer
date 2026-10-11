//! Profile-defined resource joins and fingerprints with exact scope and source citations.
use super::findings::{excerpt, fact, occurrence};
use crate::{
    evidence::Context, parser::LogEntry, processing::Budget,
    resource_observations::ResourceObservationRules,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn identity(entry: &LogEntry, rule: &ResourceObservationRules, prefix: &str) -> Option<String> {
    let source = match rule.scope_field.as_str() {
        "component_id" => Some(entry.component_id.as_str()),
        "component" => Some(entry.component.as_str()),
        field => entry.structured_field(field),
    }?;
    let parts: Vec<_> = source.split(&rule.scope_separator).collect();
    parts
        .iter()
        .position(|part| part.starts_with(prefix))
        .map(|index| parts[..=index].join(&rule.scope_separator))
}
fn payload_field<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |value, field| value.get(field))
}
// Detection requires configured scope, marker and payload structure together.
pub(super) fn matches_sample(
    entry: &LogEntry,
    rule: &ResourceObservationRules,
    budget: &mut Budget,
) -> bool {
    if identity(entry, rule, &rule.namespace_prefix).is_none() {
        return false;
    }
    if entry.message.contains(&rule.manifest_marker)
        && identity(entry, rule, &rule.owner_prefix).is_some()
        && entry
            .payload()
            .and_then(Value::as_array)
            .is_some_and(|urls| {
                urls.iter().filter_map(Value::as_str).any(|url| {
                    budget.checkpoint("profile_detection", 1) && url.contains(&rule.url_contains)
                })
            })
    {
        return true;
    }
    rule.resource_markers
        .iter()
        .all(|marker| entry.message.contains(marker))
        && entry
            .payload()
            .and_then(Value::as_array)
            .is_some_and(|batches| {
                batches
                    .iter()
                    .filter_map(|batch| payload_field(batch, &rule.resources_path))
                    .any(|resources| {
                        if !budget.checkpoint("profile_detection", 1) {
                            return false;
                        }
                        if let Some(items) = resources[&rule.entries_field].as_array() {
                            items.iter().any(|item| {
                                budget.checkpoint("profile_detection", 1)
                                    && item[&rule.url_field]
                                        .as_str()
                                        .is_some_and(|url| url.contains(&rule.url_contains))
                                    && item[&rule.hash_field].is_string()
                            })
                        } else {
                            resources.as_object().is_some_and(|items| {
                                items.iter().any(|(url, item)| {
                                    budget.checkpoint("profile_detection", 1)
                                        && url.contains(&rule.url_contains)
                                        && item[&rule.hash_field].is_string()
                                })
                            })
                        }
                    })
            })
}
struct Resource<'a> {
    hash: &'a str,
    source: &'a LogEntry,
}
#[allow(clippy::too_many_arguments)]
pub(super) fn analyze(
    entries: &[&LogEntry],
    scope: &str,
    context: &Context,
    snapshot: &Value,
    rule: &ResourceObservationRules,
    budget: &mut Budget,
    findings: &mut Vec<Value>,
) {
    let rule_key = crate::evidence::digest(rule.id.as_bytes());
    let mut owners: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut resources: BTreeMap<(String, String), Vec<Resource<'_>>> = BTreeMap::new();
    for entry in entries {
        if !budget.checkpoint("calculation", 1) {
            return;
        }
        let Some(namespace) = identity(entry, rule, &rule.namespace_prefix) else {
            continue;
        };
        if entry.message.contains(&rule.manifest_marker)
            && let (Some(owner), Some(urls)) = (
                identity(entry, rule, &rule.owner_prefix),
                entry.payload().and_then(Value::as_array),
            )
        {
            for url in urls
                .iter()
                .filter_map(Value::as_str)
                .filter(|url| url.contains(&rule.url_contains))
            {
                if !budget.checkpoint("calculation", 1) {
                    return;
                }
                owners
                    .entry((namespace.clone(), url.into()))
                    .or_default()
                    .insert(owner.clone());
            }
        }
        if rule
            .resource_markers
            .iter()
            .all(|marker| entry.message.contains(marker))
        {
            for render in entry
                .payload()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(resource) = payload_field(render, &rule.resources_path) else {
                    continue;
                };
                if let Some(items) = resource[&rule.entries_field].as_array() {
                    for item in items {
                        if let (Some(url), Some(hash)) = (
                            item[&rule.url_field].as_str(),
                            item[&rule.hash_field].as_str(),
                        ) {
                            if !budget.checkpoint("calculation", 1) {
                                return;
                            }
                            resources
                                .entry((namespace.clone(), url.into()))
                                .or_default()
                                .push(Resource {
                                    hash,
                                    source: entry,
                                });
                        }
                    }
                } else if let Some(items) = resource.as_object() {
                    for (url, item) in items {
                        if !budget.checkpoint("calculation", 1) {
                            return;
                        }
                        if let Some(hash) = item[&rule.hash_field].as_str() {
                            resources
                                .entry((namespace.clone(), url.clone()))
                                .or_default()
                                .push(Resource {
                                    hash,
                                    source: entry,
                                });
                        }
                    }
                }
            }
        }
    }
    let mut viewports: BTreeMap<String, (&Value, &LogEntry)> = BTreeMap::new();
    let mut index = 0usize;
    for entry in entries {
        if !budget.checkpoint("calculation", 1) {
            return;
        }
        let Some(namespace) = identity(entry, rule, &rule.namespace_prefix) else {
            continue;
        };
        let Some(owner) = identity(entry, rule, &rule.owner_prefix) else {
            continue;
        };
        if entry.message.starts_with(&rule.viewport_reset_marker) {
            viewports.remove(&owner);
        }
        if entry.message.starts_with(&rule.viewport_marker)
            && let Some(payload) = entry.payload().filter(|value| {
                value[&rule.width_field].is_number() && value[&rule.height_field].is_number()
            })
        {
            viewports.insert(owner.clone(), (payload, entry));
        }
        if !entry.message.contains(&rule.manifest_marker) {
            continue;
        }
        let Some(urls) = entry.payload().and_then(Value::as_array) else {
            continue;
        };
        if urls.is_empty() {
            findings.push(fact(format!("{scope}-resource-observation-{rule_key}-{index}"), scope, "observation", "Manifest lists no resources; absent displayed content cannot be established from this list.", vec![excerpt(entry,context,snapshot)], json!({"resource_status":"empty_manifest","supporting_occurrences":[occurrence(entry,context,snapshot)]})));
            index += 1;
        }
        for url in urls
            .iter()
            .filter_map(Value::as_str)
            .filter(|url| url.contains(&rule.url_contains))
        {
            if !budget.checkpoint("calculation", 1) {
                return;
            }
            let key = Some((namespace.clone(), url.to_owned()));
            let matches = key
                .as_ref()
                .filter(|key| owners.get(*key).is_some_and(|owners| owners.len() == 1))
                .and_then(|key| resources.get(key));
            let mut hashes = BTreeSet::new();
            let mut invalid_hash = false;
            for resource in matches.into_iter().flatten() {
                if !budget.checkpoint("calculation", 1) {
                    return;
                }
                if resource.hash.len() != 64
                    || !resource.hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    invalid_hash = true;
                } else {
                    hashes.insert(resource.hash.to_ascii_lowercase());
                }
            }
            let fingerprint = (!invalid_hash && hashes.len() == 1)
                .then(|| {
                    rule.fingerprints.iter().find(|fingerprint| {
                        hashes.contains(&fingerprint.sha256.to_ascii_lowercase())
                    })
                })
                .flatten();
            let resource_status = if invalid_hash || hashes.len() != 1 {
                "unavailable"
            } else if fingerprint.is_some() {
                "fingerprint_match"
            } else if rule.fingerprints.is_empty() {
                "unconfigured_fingerprint"
            } else {
                "different_fingerprint"
            };
            let status = if invalid_hash || hashes.len() != 1 {
                "Resource bytes unavailable or ambiguous: no unique scoped hash is established."
                    .to_owned()
            } else if let Some(fingerprint) = fingerprint {
                format!(
                    "Resource matches profile-declared fingerprint: {}.",
                    fingerprint.label
                )
            } else if rule.fingerprints.is_empty() {
                "Resource reports a unique scoped hash; no reference fingerprints are configured. Its content is unknown.".to_owned()
            } else {
                "Resource reports different bytes from the profile-declared fingerprints; its content is unknown.".to_owned()
            };
            if !budget.reserve(
                "calculation",
                (8192 + status.len() as u64).saturating_mul(16),
            ) {
                return;
            }
            let viewport = viewports.get(&owner);
            let mut support = vec![occurrence(entry, context, snapshot)];
            let mut excerpts = vec![excerpt(entry, context, snapshot)];
            if let Some((_, source)) = viewport {
                support.push(occurrence(source, context, snapshot));
                excerpts.push(excerpt(source, context, snapshot));
            }
            if let Some(matches) = matches {
                let mut seen = BTreeSet::new();
                for resource in matches {
                    if !budget.checkpoint("calculation", 1) {
                        return;
                    }
                    if seen.insert(resource.source.source_line_number) {
                        if !budget.reserve("calculation", 8192 * 16) {
                            return;
                        }
                        support.push(occurrence(resource.source, context, snapshot));
                        excerpts.push(excerpt(resource.source, context, snapshot));
                    }
                }
            }
            let claim = format!(
                "{status} Observed dimensions: {}. Matching requires exact input, namespace and URL with a unique originating owner. Neither name nor hash establishes displayed content or failure cause.",
                viewport.map_or_else(
                    || "unknown".into(),
                    |(value, _)| format!(
                        "{}x{}",
                        value[&rule.width_field], value[&rule.height_field]
                    )
                )
            );
            findings.push(fact(
                format!("{scope}-resource-observation-{rule_key}-{index}"),
                scope,
                "observation",
                &claim,
                excerpts,
                json!({"resource_status":resource_status,"rule_id":rule.id,
                    "viewport":viewport.map(|(value, _)| json!({"width":value[&rule.width_field],"height":value[&rule.height_field]})),
                    "fingerprint":fingerprint.map(|fingerprint| json!({"sha256":fingerprint.sha256.to_ascii_lowercase(),"label":fingerprint.label})),
                    "supporting_occurrences":support}),
            ));
            index += 1;
        }
    }
    if index == 0 {
        findings.push(fact(format!("{scope}-resource-observation-{rule_key}-unavailable"), scope, "unknown", "No configured resource observation could be established in the selected processed records.", Vec::new(), json!({"reason":"Unobserved manifest data does not establish absent content.","supporting_occurrences":[]})));
    }
}
