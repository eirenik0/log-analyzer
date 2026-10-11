//! Profile-defined resource observations; the runtime has no product-specific rules.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceObservationRules {
    pub id: String,
    pub scope_field: String,
    pub scope_separator: String,
    pub namespace_prefix: String,
    pub owner_prefix: String,
    pub manifest_marker: String,
    pub url_contains: String,
    pub resource_markers: Vec<String>,
    pub resources_path: String,
    pub entries_field: String,
    pub url_field: String,
    pub hash_field: String,
    pub viewport_marker: String,
    pub viewport_reset_marker: String,
    pub width_field: String,
    pub height_field: String,
    #[serde(default)]
    pub fingerprints: Vec<Fingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub sha256: String,
    pub label: String,
}

pub(crate) fn validate(rules: &[ResourceObservationRules]) -> Result<(), String> {
    if rules.len() > 32 {
        return Err("At most 32 resource observation declarations are allowed".into());
    }
    let mut ids = BTreeSet::new();
    for rule in rules {
        let strings = [
            &rule.id,
            &rule.scope_field,
            &rule.scope_separator,
            &rule.namespace_prefix,
            &rule.owner_prefix,
            &rule.manifest_marker,
            &rule.url_contains,
            &rule.resources_path,
            &rule.entries_field,
            &rule.url_field,
            &rule.hash_field,
            &rule.viewport_marker,
            &rule.viewport_reset_marker,
            &rule.width_field,
            &rule.height_field,
        ];
        if !ids.insert(&rule.id)
            || strings
                .iter()
                .any(|value| value.is_empty() || value.len() > 4096)
            || rule.resource_markers.is_empty()
            || rule.resource_markers.len() > 16
            || rule
                .resource_markers
                .iter()
                .any(|value| value.is_empty() || value.len() > 4096)
            || rule.fingerprints.len() > 32
        {
            return Err(
                "Resource observation rules require unique IDs and bounded nonempty selectors"
                    .into(),
            );
        }
        let mut hashes = BTreeSet::new();
        for fingerprint in &rule.fingerprints {
            if fingerprint.sha256.len() != 64
                || !fingerprint
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !hashes.insert(fingerprint.sha256.to_ascii_lowercase())
                || fingerprint.label.is_empty()
                || fingerprint.label.len() > 4096
            {
                return Err(
                    "Resource fingerprints require unique SHA-256 values and bounded labels".into(),
                );
            }
        }
    }
    Ok(())
}
