//! Deterministic, opt-in classification; all lifecycle evidence is cached by the parser.
pub use crate::config::{
    Adapter, CaptureDecode, EventMapping, EventRule, EventRuleConfig, FieldCondition,
    OperationKind, Outcome, Phase, ValueMapping,
};

use crate::parser::LogEntry;
use regex::{Captures, Regex, RegexBuilder};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

pub const MAX_RULES: usize = 128;
pub const MAX_CONDITIONS: usize = 16;
pub const MAX_SCOPE_FIELDS: usize = 16;
pub const MAX_PATTERN_BYTES: usize = 8192;
pub const MAX_VALUE_BYTES: usize = 4096;
/// A byte may be encoded as six ASCII bytes (\\uXXXX), plus two quotes.
pub const MAX_ENCODED_CAPTURE_BYTES: usize = 6 * MAX_VALUE_BYTES + 2;
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;
const REGEX_SIZE_LIMIT: usize = 1_048_576;

#[derive(Debug, Error)]
#[error("event rules: {0}")]
pub struct RuleError(String);

#[derive(Debug)]
struct CompiledRule {
    rule: EventRule,
    regex: Option<Regex>,
}

#[derive(Debug)]
struct CompiledProfile {
    schema: EventRuleConfig,
    rules: Vec<CompiledRule>,
}

/// Immutable schema and compiled rules travel together, including across config clones.
#[derive(Debug, Clone)]
pub struct CompiledEventRules(Arc<CompiledProfile>);

impl Serialize for CompiledEventRules {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.schema().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CompiledEventRules {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::compile(EventRuleConfig::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// JSON fields retain their types. Flat parser fields are strings, never coerced.
#[derive(Debug, Clone, Copy)]
pub enum StructuredFields<'a> {
    Json(&'a serde_json::Map<String, Value>),
    Flat(&'a HashMap<String, String>),
    WithPayload {
        fields: &'a StructuredFields<'a>,
        payload: Option<&'a Value>,
    },
}

/// Call at the record-parsing seam with the message BEFORE display cleanup.
/// Payload decoding belongs to normalization, not to this classifier.
#[derive(Debug, Clone, Copy)]
pub struct RecordInput<'a> {
    pub record: &'a LogEntry,
    pub original_message: &'a str,
    pub fields: StructuredFields<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSemantics {
    pub kind: OperationKind,
    pub name: String,
    pub phase: Option<Phase>,
    pub outcome: Option<Outcome>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    pub scope: Vec<String>,
    /// `false` for a start whose rule says no end record follows.
    #[serde(default = "end_expected_default", skip_serializing_if = "is_true")]
    pub end_expected: bool,
}

fn end_expected_default() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// Provenance stays in the existing record: timestamp/offset/year inference,
/// source file/line/row, component, raw line and normalized record.
#[derive(Debug)]
pub struct NormalizedEvent<'a> {
    pub semantics: EventSemantics,
    pub record: &'a LogEntry,
    pub profile: &'a str,
    pub rule_ids: Vec<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic<'a> {
    pub rule_id: Option<&'a str>,
    pub reason: &'static str,
    pub target: &'static str,
}

#[derive(Debug)]
pub enum Classification<'a> {
    Recognized(NormalizedEvent<'a>),
    IdentityOnly(NormalizedEvent<'a>),
    Unclassified,
    Conflict {
        rule_ids: Vec<&'a str>,
    },
    Invalid {
        diagnostics: Vec<Diagnostic<'a>>,
        kinds: Vec<OperationKind>,
    },
}

/// Owned evidence cached once on a parsed record; no self-referential record copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClassifiedRecord {
    Event {
        #[serde(default)]
        legacy: bool,
        semantics: EventSemantics,
        profile: String,
        rule_ids: Vec<String>,
    },
    Unclassified,
    Conflict {
        #[serde(default)]
        kinds: Vec<OperationKind>,
        profile: String,
        rule_ids: Vec<String>,
    },
    Invalid {
        #[serde(default)]
        kinds: Vec<OperationKind>,
        profile: String,
        diagnostics: Vec<RecordDiagnostic>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordDiagnostic {
    pub rule_id: Option<String>,
    pub reason: String,
    pub target: String,
}

impl Classification<'_> {
    pub fn into_owned(self, profile: &str) -> ClassifiedRecord {
        match self {
            Self::Recognized(event) | Self::IdentityOnly(event) => ClassifiedRecord::Event {
                legacy: false,
                semantics: event.semantics,
                profile: event.profile.into(),
                rule_ids: event.rule_ids.into_iter().map(str::to_owned).collect(),
            },
            Self::Unclassified => ClassifiedRecord::Unclassified,
            Self::Conflict { rule_ids } => ClassifiedRecord::Conflict {
                kinds: Vec::new(),
                profile: profile.into(),
                rule_ids: rule_ids.into_iter().map(str::to_owned).collect(),
            },
            Self::Invalid { diagnostics, kinds } => ClassifiedRecord::Invalid {
                kinds,
                profile: profile.into(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| RecordDiagnostic {
                        rule_id: diagnostic.rule_id.map(str::to_owned),
                        reason: diagnostic.reason.into(),
                        target: diagnostic.target.into(),
                    })
                    .collect(),
            },
        }
    }
}

/// Keeps a record-field scope key within `MAX_VALUE_BYTES`.
///
/// Values over the limit become a readable prefix plus the byte length and a 128-bit
/// FNV-1a digest of the full value. Equal values give equal keys, so long scopes still
/// correlate. Short values containing the digest marker are also encoded, preventing
/// a raw value from impersonating a generated key. Digest collisions remain possible.
pub fn bounded_scope_value(value: &str) -> String {
    const DIGEST_MARKER: &str = " bytes, fnv1a128:";
    if value.len() <= MAX_VALUE_BYTES && !value.contains(DIGEST_MARKER) {
        return value.to_string();
    }
    const PREFIX_BYTES: usize = 64;
    let mut cut = PREFIX_BYTES.min(value.len());
    while !value.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    for byte in value.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x0000000001000000000000000000013B);
    }
    format!(
        "{}…[{}{DIGEST_MARKER}{hash:032x}]",
        &value[..cut],
        value.len()
    )
}

fn bounded_nonempty(value: &str) -> bool {
    value.len() <= MAX_VALUE_BYTES && !value.trim().is_empty()
}

fn phase(value: &str) -> Option<Phase> {
    match value {
        "start" => Some(Phase::Start),
        "end" => Some(Phase::End),
        _ => None,
    }
}

fn outcome(value: &str) -> Option<Outcome> {
    match value {
        "success" => Some(Outcome::Success),
        "failure" => Some(Outcome::Failure),
        _ => None,
    }
}

impl CompiledEventRules {
    pub fn schema(&self) -> &EventRuleConfig {
        &self.0.schema
    }

    pub fn compile(schema: EventRuleConfig) -> Result<Self, RuleError> {
        if !matches!(schema.version, 1 | 2) {
            return Err(RuleError(
                "unsupported version; expected version = 1 or 2".into(),
            ));
        }
        if schema.rules.len() > MAX_RULES {
            return Err(RuleError(format!(
                "at most {MAX_RULES} rules are supported"
            )));
        }
        let mut ids = HashSet::new();
        let mut rules = Vec::new();
        for rule in &schema.rules {
            // Validate IDs before including them in diagnostics.
            if rule.id.len() > 128
                || rule.id.is_empty()
                || !rule
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
            {
                return Err(RuleError(
                    "rule IDs must be 1..128 ASCII letters, digits, _, - or .".into(),
                ));
            }
            let fail = |reason: &str| RuleError(format!("rule '{}': {reason}", rule.id));
            if !ids.insert(&rule.id) {
                return Err(fail("duplicate ID; choose a unique rule ID"));
            }
            let regex = match &rule.adapter {
                Adapter::Text { pattern } => {
                    if pattern.len() > MAX_PATTERN_BYTES {
                        return Err(fail("pattern exceeds 8192 bytes"));
                    }
                    Some(
                        RegexBuilder::new(&format!(r"\A(?:{pattern})\z"))
                            .size_limit(REGEX_SIZE_LIMIT)
                            .dfa_size_limit(REGEX_SIZE_LIMIT)
                            .build()
                            .map_err(|error| {
                                let message = error.to_string();
                                let detail: String = message
                                    .lines()
                                    .last()
                                    .unwrap_or("invalid regex")
                                    .chars()
                                    .take(256)
                                    .collect();
                                fail(&format!("invalid or oversized regex: {detail}"))
                            })?,
                    )
                }
                Adapter::Structured { conditions } => {
                    if conditions.is_empty() || conditions.len() > MAX_CONDITIONS {
                        return Err(fail("structured adapter requires 1..16 conditions"));
                    }
                    for condition in conditions {
                        if !bounded_nonempty(&condition.field) {
                            return Err(fail(
                                "condition field must be nonempty and at most 4096 bytes",
                            ));
                        }
                        match &condition.equals {
                            Value::String(v) if v.len() <= MAX_VALUE_BYTES => (),
                            Value::Bool(_) | Value::Number(_) => (),
                            _ => {
                                return Err(fail(
                                    "conditions require a bounded string, boolean or number; null/containers are unsupported",
                                ));
                            }
                        }
                    }
                    None
                }
            };
            if rule.mapping.scope.len() > MAX_SCOPE_FIELDS {
                return Err(fail("at most 16 scope mappings are supported"));
            }
            let mapping = &rule.mapping;
            if schema.version == 1 && (mapping.direction.is_some() || mapping.endpoint.is_some()) {
                return Err(fail(
                    "direction and endpoint mappings require event-rule version 2",
                ));
            }
            let mappings = std::iter::once(&mapping.name)
                .chain(mapping.phase.iter())
                .chain(mapping.outcome.iter())
                .chain(mapping.correlation_id.iter())
                .chain(mapping.direction.iter())
                .chain(mapping.endpoint.iter())
                .chain(mapping.scope.iter());
            for source in mappings {
                if schema.version == 1 && matches!(source, ValueMapping::FirstField { .. }) {
                    return Err(fail("first_field mappings require event-rule version 2"));
                }
                match source {
                    ValueMapping::Literal { value } if bounded_nonempty(value) => (),
                    ValueMapping::Field { field } if bounded_nonempty(field) => (),
                    ValueMapping::FirstField { fields }
                        if !fields.is_empty()
                            && fields.len() <= 16
                            && fields.iter().all(|f| bounded_nonempty(f)) => {}
                    ValueMapping::Capture { capture, .. } if bounded_nonempty(capture) => {
                        if !regex
                            .as_ref()
                            .is_some_and(|r| r.capture_names().flatten().any(|n| n == capture))
                        {
                            return Err(fail(
                                "capture mapping requires an existing named capture in a text adapter",
                            ));
                        }
                    }
                    _ => {
                        return Err(fail(
                            "mapping source must be nonempty and at most 4096 bytes",
                        ));
                    }
                }
            }
            if let Some(ValueMapping::Literal { value }) = &mapping.phase
                && phase(value).is_none()
            {
                return Err(fail("phase must be start or end"));
            }
            if let Some(ValueMapping::Literal { value }) = &mapping.outcome
                && outcome(value).is_none()
            {
                return Err(fail("outcome must be success or failure"));
            }
            if let Some(ValueMapping::Literal { value }) = &mapping.direction
                && !valid_direction(mapping.kind, value)
            {
                return Err(fail(
                    "direction must be send/receive for requests or emit/receive for events",
                ));
            }
            if let Some(end_expected) = mapping.end_expected {
                if schema.version == 1 {
                    return Err(fail("end_expected requires event-rule version 2"));
                }
                if !end_expected
                    && (mapping.phase.is_none()
                        || matches!(&mapping.phase, Some(ValueMapping::Literal { value }) if value != "start"))
                {
                    return Err(fail("end_expected = false requires a start phase"));
                }
            }
            if mapping.outcome.is_some()
                && (mapping.phase.is_none()
                    || matches!(&mapping.phase, Some(ValueMapping::Literal { value }) if value != "end"))
            {
                return Err(fail("outcome requires an end phase"));
            }
            rules.push(CompiledRule {
                rule: rule.clone(),
                regex,
            });
        }
        Ok(Self(Arc::new(CompiledProfile { schema, rules })))
    }

    pub fn classify_owned(&self, profile: &str, input: RecordInput<'_>) -> ClassifiedRecord {
        let mut result = self.classify(profile, input).into_owned(profile);
        if let ClassifiedRecord::Conflict {
            rule_ids, kinds, ..
        } = &mut result
        {
            for rule in &self.schema().rules {
                if rule_ids.contains(&rule.id) && !kinds.contains(&rule.mapping.kind) {
                    kinds.push(rule.mapping.kind);
                }
            }
        }
        result
    }

    pub fn classify<'a>(&'a self, profile: &'a str, input: RecordInput<'a>) -> Classification<'a> {
        if input.original_message.len() > MAX_MESSAGE_BYTES {
            return Classification::Invalid {
                kinds: self.schema().rules.iter().map(|r| r.mapping.kind).fold(
                    Vec::new(),
                    |mut kinds, kind| {
                        if !kinds.contains(&kind) {
                            kinds.push(kind);
                        }
                        kinds
                    },
                ),
                diagnostics: vec![Diagnostic {
                    rule_id: None,
                    reason: "message_limit_exceeded",
                    target: "message",
                }],
            };
        }
        let mut matches = Vec::new();
        let mut diagnostics = Vec::new();
        let mut matched_kinds = Vec::new();
        for compiled in &self.0.rules {
            let captures = match &compiled.rule.adapter {
                Adapter::Text { .. } => {
                    let Some(captures) = compiled
                        .regex
                        .as_ref()
                        .unwrap()
                        .captures(input.original_message)
                    else {
                        continue;
                    };
                    Some(captures)
                }
                Adapter::Structured { conditions } => {
                    if !conditions
                        .iter()
                        .all(|condition| input.fields.equals(&condition.field, &condition.equals))
                    {
                        continue;
                    }
                    None
                }
            };
            if !matched_kinds.contains(&compiled.rule.mapping.kind) {
                matched_kinds.push(compiled.rule.mapping.kind);
            }
            match map_event(&compiled.rule.mapping, input.fields, captures.as_ref()) {
                Ok(event) => matches.push((compiled.rule.id.as_str(), event)),
                Err((reason, target)) => diagnostics.push(Diagnostic {
                    rule_id: Some(&compiled.rule.id),
                    reason,
                    target,
                }),
            }
        }
        // Malformed matching evidence cannot be hidden by a valid sibling rule.
        if !diagnostics.is_empty() {
            return Classification::Invalid {
                diagnostics,
                kinds: matched_kinds,
            };
        }
        let Some((_, first)) = matches.first() else {
            return Classification::Unclassified;
        };
        if matches.iter().any(|(_, event)| event != first) {
            return Classification::Conflict {
                rule_ids: matches.iter().map(|(id, _)| *id).collect(),
            };
        }
        let event = NormalizedEvent {
            semantics: first.clone(),
            record: input.record,
            profile,
            rule_ids: matches.iter().map(|(id, _)| *id).collect(),
        };
        if event.semantics.phase.is_some() {
            Classification::Recognized(event)
        } else {
            Classification::IdentityOnly(event)
        }
    }
}

impl<'a> StructuredFields<'a> {
    fn equals(self, key: &str, expected: &Value) -> bool {
        match self {
            Self::WithPayload { fields, payload } => {
                if let Some(key) = key.strip_prefix("payload.") {
                    payload
                        .and_then(|p| p.get(key))
                        .is_some_and(|v| v == expected)
                } else {
                    fields.equals(key, expected)
                }
            }
            Self::Json(fields) => fields.get(key).is_some_and(|value| value == expected),
            Self::Flat(fields) => expected
                .as_str()
                .is_some_and(|s| fields.get(key).is_some_and(|v| v == s)),
        }
    }

    fn present(self, key: &str) -> bool {
        match self {
            Self::Json(fields) => fields.contains_key(key),
            Self::Flat(fields) => fields.contains_key(key),
            Self::WithPayload { fields, payload } => {
                if let Some(key) = key.strip_prefix("payload.") {
                    payload.is_some_and(|p| p.get(key).is_some())
                } else {
                    fields.present(key)
                }
            }
        }
    }

    fn string(self, key: &str) -> Option<&'a str> {
        match self {
            Self::WithPayload { fields, payload } => {
                if let Some(key) = key.strip_prefix("payload.") {
                    payload?.get(key)?.as_str()
                } else {
                    fields.string(key)
                }
            }
            Self::Json(fields) => fields.get(key)?.as_str(),
            Self::Flat(fields) => fields.get(key).map(String::as_str),
        }
    }
}

type MappingFailure = (&'static str, &'static str);

fn resolve(
    mapping: &ValueMapping,
    fields: StructuredFields<'_>,
    captures: Option<&Captures<'_>>,
) -> Result<String, &'static str> {
    let value = match mapping {
        ValueMapping::Literal { value } => value.as_str(),
        ValueMapping::FirstField {
            fields: alternatives,
        } => alternatives
            .iter()
            .find(|key| fields.present(key))
            .and_then(|key| fields.string(key))
            .ok_or("missing_or_non_string_field")?,
        ValueMapping::Field { field } => {
            fields.string(field).ok_or("missing_or_non_string_field")?
        }
        ValueMapping::Capture { capture, .. } => captures
            .and_then(|c| c.name(capture))
            .map(|c| c.as_str())
            .ok_or("missing_capture")?,
    };
    let decoded = match mapping {
        ValueMapping::Capture {
            decode: CaptureDecode::JsonString,
            ..
        } => {
            if value.len() > MAX_ENCODED_CAPTURE_BYTES {
                return Err("encoded_capture_limit_exceeded");
            }
            serde_json::from_str::<String>(value).map_err(|_| "invalid_json_string_capture")?
        }
        _ => {
            if !bounded_nonempty(value) {
                return Err("empty_or_oversized_value");
            }
            value.to_string()
        }
    };
    if !bounded_nonempty(&decoded) {
        return Err("empty_or_oversized_value");
    }
    Ok(decoded)
}

fn map_event(
    mapping: &EventMapping,
    fields: StructuredFields<'_>,
    captures: Option<&Captures<'_>>,
) -> Result<EventSemantics, MappingFailure> {
    let get = |source: &ValueMapping, target| {
        resolve(source, fields, captures).map_err(|reason| (reason, target))
    };
    let name = get(&mapping.name, "name")?;
    let phase = mapping
        .phase
        .as_ref()
        .map(|source| {
            let value = get(source, "phase")?;
            phase(&value).ok_or(("invalid_phase", "phase"))
        })
        .transpose()?;
    let outcome = mapping
        .outcome
        .as_ref()
        .map(|source| {
            let value = get(source, "outcome")?;
            outcome(&value).ok_or(("invalid_outcome", "outcome"))
        })
        .transpose()?;
    if outcome.is_some() && phase != Some(Phase::End) {
        return Err(("outcome_requires_end", "outcome"));
    }
    let end_expected = mapping.end_expected.unwrap_or(true);
    if !end_expected && phase != Some(Phase::Start) {
        return Err(("end_expected_requires_start", "phase"));
    }
    Ok(EventSemantics {
        kind: mapping.kind,
        name,
        phase,
        outcome,
        end_expected,
        correlation_id: mapping
            .correlation_id
            .as_ref()
            .map(|source| {
                if let ValueMapping::FirstField {
                    fields: alternatives,
                } = source
                    && alternatives.iter().all(|key| !fields.present(key))
                {
                    return Ok(None);
                }
                get(source, "correlation_id").map(Some)
            })
            .transpose()?
            .flatten(),
        direction: mapping
            .direction
            .as_ref()
            .map(|source| {
                let value = get(source, "direction")?;
                if valid_direction(mapping.kind, &value) {
                    Ok(value)
                } else {
                    Err(("invalid_direction", "direction"))
                }
            })
            .transpose()?,
        endpoint: mapping
            .endpoint
            .as_ref()
            .map(|source| {
                if let ValueMapping::Capture { capture, .. } = source
                    && captures.and_then(|c| c.name(capture)).is_none()
                {
                    return Ok(None);
                }
                get(source, "endpoint").map(Some)
            })
            .transpose()?
            .flatten(),
        scope: mapping
            .scope
            .iter()
            .map(|source| get(source, "scope").map(|value| bounded_scope_value(&value)))
            .collect::<Result<_, _>>()?,
    })
}

fn valid_direction(kind: OperationKind, direction: &str) -> bool {
    match kind {
        OperationKind::Command => false,
        OperationKind::Request => matches!(direction, "send" | "receive"),
        OperationKind::Event => matches!(direction, "emit" | "receive"),
    }
}
