//! Scope ambiguity is checked over every source witness before selected delivery.
use super::{
    Result, finding_store::FindingSink, findings, grouped::Groups, selection::View,
    source_store::Entries,
};
use crate::{
    config::AnalyzerConfig, event_rules::ClassifiedRecord, evidence, parser::LogEntry,
    processing::Budget,
};
use serde_json::{Value, json};

fn identity(
    entry: &LogEntry,
    config: &AnalyzerConfig,
    witness: &mut AnalyzerConfig,
) -> Vec<String> {
    let mut identity = vec![entry.component_id.clone()];
    for field in &config.perf.correlation_scope_fields {
        witness.perf.correlation_scope_fields = vec![field.clone()];
        let value = crate::parser::record_correlation_scope(entry, witness)
            .and_then(|values| values.into_iter().next());
        identity.push(format!(
            "{field}={}",
            value.as_deref().unwrap_or("<unavailable>")
        ));
    }
    identity
}
#[allow(clippy::too_many_arguments)]
pub(super) fn analyze(
    entries: &Entries,
    view: &View,
    config: &AnalyzerConfig,
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    budget: &mut Budget,
    sink: &mut dyn FindingSink,
) -> Result<Option<usize>> {
    let mut groups = Groups::new(entries)?;
    for (ordinal, entry) in entries.iter()?.enumerate() {
        let entry = entry?;
        if !budget.checkpoint(
            "calculation",
            (config.perf.correlation_scope_fields.len() + 1) as u64,
        ) {
            return Ok(None);
        }
        if let Some(ClassifiedRecord::Event { semantics, .. }) = &entry.classification
            && let Some(id) = &semantics.correlation_id
        {
            let mut key = vec![
                semantics.kind.label().into(),
                semantics.name.clone(),
                id.clone(),
            ];
            key.extend(crate::perf_analyzer::correlation_scope(&entry, config).unwrap_or_default());
            groups.insert("scope", &key, ordinal)?;
        }
    }
    let mut aliases = 0;
    let mut witness_config = config.clone();
    for key in groups.keys("scope", false) {
        let key = key?;
        let mut first = None;
        let (mut different, mut selected, mut count) = (false, false, 0usize);
        for item in groups.entries(entries, "scope", &key) {
            if !budget.checkpoint("calculation", 1) {
                return Ok(None);
            }
            let (_, entry) = item?;
            let current = identity(&entry, config, &mut witness_config);
            if let Some(first) = &first {
                different |= *first != current;
            } else {
                first = Some(current);
            }
            selected |= view.contains(&entry)?;
            count += 1;
        }
        if !different || !selected {
            continue;
        }
        let mut evidence = Vec::new();
        let mut occurrences = Vec::new();
        let (mut chunk, mut bytes) = (0usize, 0usize);
        for item in groups.entries(entries, "scope", &key) {
            if !budget.checkpoint("calculation", 1) {
                return Ok(None);
            }
            let (_, entry) = item?;
            let excerpt = findings::excerpt(&entry, context, snapshot);
            let occurrence = findings::occurrence(&entry, context, snapshot);
            let size = serde_json::to_vec(&excerpt)?.len() + serde_json::to_vec(&occurrence)?.len();
            if !evidence.is_empty()
                && (evidence.len() >= 128 || bytes.saturating_add(size) > 1024 * 1024)
            {
                emit(
                    scope,
                    aliases,
                    chunk,
                    count,
                    &mut evidence,
                    &mut occurrences,
                    sink,
                )?;
                chunk += 1;
                bytes = 0;
            }
            evidence.push(excerpt);
            occurrences.push(occurrence);
            bytes = bytes.saturating_add(size);
        }
        if !evidence.is_empty() {
            emit(
                scope,
                aliases,
                chunk,
                count,
                &mut evidence,
                &mut occurrences,
                sink,
            )?;
        }
        aliases += 1;
    }
    Ok(Some(aliases))
}
fn emit(
    scope: &str,
    alias: usize,
    chunk: usize,
    count: usize,
    evidence: &mut Vec<Value>,
    occurrences: &mut Vec<Value>,
    sink: &mut dyn FindingSink,
) -> Result<()> {
    let id = if chunk == 0 {
        format!("{scope}-scope-adequacy-{alias}")
    } else {
        format!("{scope}-scope-adequacy-{alias}-context-{chunk}")
    };
    let mut details = json!({"reason":"Source-identity witnesses do not prove intended domain scope. Review the explicit scope before relying on paired lifecycle meaning.", "supporting_occurrences":std::mem::take(occurrences)});
    let summary = if evidence.len() == count {
        "The selected population intersects a shared effective correlation key; all source-identity witnesses are retained as context, including any outside selection. Intended scope adequacy is unknown."
    } else {
        details["reason"] = json!(format!(
            "Source-identity witnesses do not prove intended domain scope. Review the explicit scope before relying on paired lifecycle meaning. Context chunk {} contains {} of {count} witnesses. Processing coverage determines whether later context chunks were completed.",
            chunk + 1,
            evidence.len()
        ));
        "The selected population intersects a shared effective correlation key; this finding contains one bounded chunk of source-identity witnesses, including any outside selection. Intended scope adequacy is unknown."
    };
    sink.push(findings::fact(
        id,
        scope,
        "unknown",
        summary,
        std::mem::take(evidence),
        details,
    ))
}
