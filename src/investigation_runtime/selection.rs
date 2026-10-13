//! Exact selected view of already-correlated occurrences; never pairs again.
use super::{Result, Selector, selected, source_store::Entries};
use crate::disk_index::DiskIndex;
use crate::event_rules::{Adapter, EventRule, EventSemantics, Outcome, Phase, ValueMapping};
use crate::{
    config::AnalyzerConfig, event_rules::ClassifiedRecord, parser::LogEntry,
    perf_analyzer::SourceLocation, processing::Budget,
};
use std::collections::BTreeMap;
pub(super) struct View {
    pub entries: Entries,
    pub relevant: usize,
    pub classified: usize,
    pub boundaries: usize,
    pub identity_only: usize,
    pub conflicting: usize,
    pub classification_loss: usize,
    pub pairs: usize,
    pub unmatched: usize,
    pub ambiguous: usize,
    pub rejected: usize,
    pub excluded_boundaries: usize,
    pub reliable: bool,
    pub failures_supported: bool,
    pub successes_supported: bool,
    pub some_failures_supported: bool,
    pub starts_supported: bool,
    pub ends_supported: bool,
    pub pair_supported: bool,
    pub boundary_requirements: usize,
    pub unrecognized_boundaries: usize,
    recognizable: DiskIndex,
}
impl View {
    pub fn new(
        entries: &Entries,
        selectors: &[Selector],
        config: &AnalyzerConfig,
        perf: Option<&super::Correlated>,
        budget: &mut Budget,
    ) -> Result<Self> {
        let mut view = Self {
            entries: entries.empty_like()?,
            relevant: 0,
            classified: 0,
            boundaries: 0,
            identity_only: 0,
            conflicting: 0,
            classification_loss: 0,
            pairs: 0,
            unmatched: 0,
            ambiguous: 0,
            rejected: 0,
            excluded_boundaries: 0,
            reliable: true,
            failures_supported: true,
            successes_supported: true,
            some_failures_supported: false,
            starts_supported: true,
            ends_supported: true,
            pair_supported: true,
            boundary_requirements: 0,
            unrecognized_boundaries: 0,
            recognizable: DiskIndex::new(entries.len() as u64, entries.state_bytes)?,
        };
        for entry in entries.iter()? {
            let entry = entry?;
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            if !selected(&entry, selectors, config) {
                continue;
            }
            view.entries.push(&entry)?;
            if matches!(entry.classification, Some(ClassifiedRecord::Event { .. })) {
                view.reliable &= !entry.timestamp_year_inferred && entry.source_timestamp.is_some();
            }
            match &entry.classification {
                Some(ClassifiedRecord::Event { semantics, .. }) => {
                    view.classified += 1;
                    if semantics.phase.is_some() {
                        view.boundaries += 1;
                    } else {
                        view.identity_only += 1;
                    }
                    view.relevant += 1;
                }
                Some(ClassifiedRecord::Conflict { .. }) => {
                    view.conflicting += 1;
                    view.classification_loss += 1;
                    view.relevant += 1;
                }
                Some(ClassifiedRecord::Invalid { .. }) => {
                    view.classification_loss += 1;
                    view.relevant += 1;
                }
                _ => {}
            }
        }
        view.entries.seal()?;
        let mut observed = DiskIndex::new(view.entries.len() as u64, entries.state_bytes)?;
        for entry in view.entries.iter()? {
            let entry = entry?;
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            if let Some(ClassifiedRecord::Event { semantics, .. }) = &entry.classification {
                let key = capability_key(semantics)?;
                let mut caps = Capabilities::decode(observed.get(&key)?.unwrap_or(0));
                caps.start |= semantics.phase == Some(Phase::Start);
                caps.end |= semantics.phase == Some(Phase::End);
                caps.success |= semantics.outcome == Some(Outcome::Success);
                caps.failure |= semantics.outcome == Some(Outcome::Failure);
                observed.replace(&key, caps.encode())?;
            }
        }
        'capabilities: for entry in view.entries.iter()? {
            let entry = entry?;
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            let Some(ClassifiedRecord::Event {
                semantics,
                rule_ids,
                ..
            }) = &entry.classification
            else {
                continue;
            };
            let mut caps =
                Capabilities::decode(observed.get(&capability_key(semantics)?)?.unwrap_or(0));
            if let Some(classifier) = config.event_classifier() {
                if !budget.checkpoint(
                    "calculation",
                    (classifier.schema().rules.len() as u64)
                        .saturating_mul(rule_ids.len() as u64 + 1),
                ) {
                    break 'capabilities;
                }
                let selected_rules = classifier
                    .schema()
                    .rules
                    .iter()
                    .filter(|rule| rule_ids.contains(&rule.id))
                    .collect::<Vec<_>>();
                for rule in &classifier.schema().rules {
                    if !budget.checkpoint(
                        "calculation",
                        (selected_rules.len() as u64).saturating_add(1),
                    ) {
                        break 'capabilities;
                    }
                    let related = selected_rules.iter().any(|selected| {
                        rule.id == selected.id
                            || (semantics.phase.is_some() && compatible(rule, selected, semantics))
                    });
                    if related {
                        caps.start |= phase_capability(rule, semantics, "start");
                        if rule.mapping.outcome.is_none() {
                            caps.end |= phase_capability(rule, semantics, "end");
                        }
                        caps.success |= outcome_capability(rule, semantics, "success");
                        caps.failure |= outcome_capability(rule, semantics, "failure");
                    }
                }
            }
            caps.end |= caps.success && caps.failure;
            view.failures_supported &= caps.failure;
            view.successes_supported &= caps.success;
            view.some_failures_supported |= caps.failure;
            view.starts_supported &= caps.start;
            view.ends_supported &= caps.end;
            view.pair_supported &= caps.start && caps.end;
            view.recognizable.insert(
                &super::source_store::address(
                    entry.source_line_number,
                    entry.source_row_path.as_deref(),
                )?,
                caps.encode(),
            )?;
            let required = match semantics.phase {
                Some(Phase::Start) if semantics.end_expected => Some(caps.end),
                Some(Phase::End) => Some(caps.start),
                _ => None,
            };
            if let Some(recognizable) = required {
                view.boundary_requirements += 1;
                view.unrecognized_boundaries += usize::from(!recognizable);
            }
        }
        if view.classified == 0 || view.classification_loss > 0 {
            view.pair_supported = false;
            view.failures_supported = false;
            view.successes_supported = false;
            view.starts_supported = false;
            view.ends_supported = false;
        }
        if let Some(perf) = perf {
            for operation in perf.operations.iter()? {
                let operation = operation?;
                if !budget.checkpoint("calculation", 1) {
                    break;
                }
                let start = view.source_selected(&operation.start_source)?;
                let end = view.source_selected(&operation.end_source)?;
                if start && end {
                    view.pairs += 1;
                } else if start || end {
                    view.excluded_boundaries += 1;
                }
            }
            for event in perf.unmatched_events.iter()? {
                let event = event?;
                if !budget.checkpoint("calculation", 1) {
                    break;
                }
                if !view.source_selected(&event.source)? {
                    continue;
                }
                view.unmatched += 1;
                if matches!(
                    event.reason.as_str(),
                    "overlapping_starts"
                        | "ambiguous_timestamp_order"
                        | "ambiguous_boundary"
                        | "conflicting_event_rules"
                ) {
                    view.ambiguous += 1;
                }
                if matches!(
                    event.reason.as_str(),
                    "missing_correlation_key"
                        | "missing_scope_field"
                        | "incomplete_timestamp_year"
                        | "invalid_event_data"
                ) {
                    view.rejected += 1;
                }
            }
        }
        Ok(view)
    }
    pub fn boundary_supported(&self, entry: &LogEntry, phase: Phase) -> Result<bool> {
        if self.classification_loss > 0 {
            return Ok(false);
        }
        let caps = Capabilities::decode(
            self.recognizable
                .get(&super::source_store::address(
                    entry.source_line_number,
                    entry.source_row_path.as_deref(),
                )?)?
                .unwrap_or(0),
        );
        Ok(match phase {
            Phase::Start => caps.start,
            Phase::End => caps.end,
        })
    }
    fn source_selected(&self, source: &SourceLocation) -> Result<bool> {
        Ok(self
            .entries
            .ordinal(source.line, source.row_path.as_deref())?
            .is_some())
    }
    pub fn contains(&self, entry: &LogEntry) -> Result<bool> {
        self.entries.contains(entry)
    }
}
fn capability_key(semantics: &EventSemantics) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        semantics.kind.label(),
        &semantics.name,
        &semantics.scope,
        &semantics.correlation_id,
    ))?)
}

#[derive(Default, Clone, Copy)]
struct Capabilities {
    start: bool,
    end: bool,
    success: bool,
    failure: bool,
}
impl Capabilities {
    fn encode(self) -> u64 {
        u64::from(self.start)
            | (u64::from(self.end) << 1)
            | (u64::from(self.success) << 2)
            | (u64::from(self.failure) << 3)
    }
    fn decode(bits: u64) -> Self {
        Self {
            start: bits & 1 != 0,
            end: bits & 2 != 0,
            success: bits & 4 != 0,
            failure: bits & 8 != 0,
        }
    }
}
fn maps(mapping: Option<&ValueMapping>, target: &str, adapter: &Adapter) -> bool {
    match mapping {
        Some(ValueMapping::Literal { value }) => value == target,
        Some(ValueMapping::Field { field }) => match adapter {
            Adapter::Structured { conditions } => conditions
                .iter()
                .filter(|condition| &condition.field == field)
                .all(|condition| condition.equals.as_str() == Some(target)),
            _ => false,
        },
        _ => false,
    }
}
fn compatible(rule: &EventRule, selected: &EventRule, semantics: &EventSemantics) -> bool {
    rule.mapping.kind == semantics.kind
        && match &rule.mapping.name {
            ValueMapping::Literal { value } => value == &semantics.name,
            ValueMapping::Field { .. } => {
                serde_json::to_value(&rule.mapping.name).ok()
                    == serde_json::to_value(&selected.mapping.name).ok()
                    && maps(Some(&rule.mapping.name), &semantics.name, &rule.adapter)
            }
            _ => false,
        }
        && serde_json::to_value(&rule.mapping.correlation_id).ok()
            == serde_json::to_value(&selected.mapping.correlation_id).ok()
        && semantics
            .correlation_id
            .as_ref()
            .is_none_or(|id| maps(rule.mapping.correlation_id.as_ref(), id, &rule.adapter))
        && serde_json::to_value(&rule.mapping.scope).ok()
            == serde_json::to_value(&selected.mapping.scope).ok()
        && rule
            .mapping
            .scope
            .iter()
            .zip(&semantics.scope)
            .all(|(mapping, value)| maps(Some(mapping), value, &rule.adapter))
}

fn joint(rule: &EventRule, semantics: &EventSemantics, phase: &str, outcome: Option<&str>) -> bool {
    fn assign<'a>(
        fields: &mut BTreeMap<&'a str, &'a str>,
        mapping: &'a ValueMapping,
        value: &'a str,
    ) -> bool {
        match mapping {
            ValueMapping::Literal { value: literal } => literal == value,
            ValueMapping::Field { field } => match fields.get(field.as_str()) {
                Some(existing) => *existing == value,
                None => {
                    fields.insert(field, value);
                    true
                }
            },
            ValueMapping::Capture { .. } => true,
            ValueMapping::FirstField { .. } => false,
        }
    }
    let mut fields = BTreeMap::new();
    if !assign(&mut fields, &rule.mapping.name, &semantics.name) {
        return false;
    }
    if let (Some(mapping), Some(value)) = (&rule.mapping.correlation_id, &semantics.correlation_id)
        && !assign(&mut fields, mapping, value)
    {
        return false;
    }
    for (mapping, value) in rule.mapping.scope.iter().zip(&semantics.scope) {
        if !assign(&mut fields, mapping, value) {
            return false;
        }
    }
    if let Some(mapping) = &rule.mapping.phase
        && !assign(&mut fields, mapping, phase)
    {
        return false;
    }
    if let (Some(mapping), Some(value)) = (&rule.mapping.outcome, outcome)
        && !assign(&mut fields, mapping, value)
    {
        return false;
    }
    match &rule.adapter {
        Adapter::Structured { conditions } => {
            let mut equalities = BTreeMap::new();
            conditions.iter().all(|condition| {
                if equalities
                    .insert(condition.field.as_str(), &condition.equals)
                    .is_some_and(|previous| previous != &condition.equals)
                {
                    return false;
                }
                fields
                    .get(condition.field.as_str())
                    .is_none_or(|value| condition.equals.as_str() == Some(*value))
            })
        }
        _ => true,
    }
}
fn outcome_capability(rule: &EventRule, semantics: &EventSemantics, outcome: &str) -> bool {
    maps(rule.mapping.phase.as_ref(), "end", &rule.adapter)
        && maps(rule.mapping.outcome.as_ref(), outcome, &rule.adapter)
        && joint(rule, semantics, "end", Some(outcome))
}
fn phase_capability(rule: &EventRule, semantics: &EventSemantics, phase: &str) -> bool {
    if !maps(rule.mapping.phase.as_ref(), phase, &rule.adapter)
        || !joint(rule, semantics, phase, None)
    {
        return false;
    }
    if rule.mapping.outcome.is_none() {
        return true;
    }
    phase == "end"
        && (outcome_capability(rule, semantics, "success")
            || outcome_capability(rule, semantics, "failure"))
}
