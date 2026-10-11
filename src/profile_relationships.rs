//! Source-addressed link assertions observe the investigation engines before sampling.
use crate::{
    comparator::LogFilter,
    config::AnalyzerConfig,
    event_rules::ClassifiedRecord,
    evidence,
    investigation_policy::{Chain, ChainStep, Relationship},
    parser::LogEntry,
    processing::{Budget, Limits},
    relationship_rules::{Matcher, Observer, key},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Address {
    pub input: usize,
    pub line: usize,
    #[serde(deserialize_with = "required_row_path")]
    pub row_path: Option<String>,
}
fn required_row_path<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
impl Address {
    fn matches(&self, input: usize, entry: &LogEntry) -> bool {
        self.input == input
            && self.line == entry.source_line_number
            && self.row_path == entry.source_row_path
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChainLink {
    pub chain: String,
    pub step: String,
    pub root: Address,
    pub source: Address,
    pub target: Address,
    pub linked: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelationshipLink {
    pub relationship: String,
    pub source: Address,
    pub target: Address,
    pub linked: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum LinkFact {
    Chain(ChainLink),
    Relationship(RelationshipLink),
}
impl LinkFact {
    pub fn source(&self) -> &Address {
        match self {
            Self::Chain(f) => &f.source,
            Self::Relationship(f) => &f.source,
        }
    }
    pub fn target(&self) -> &Address {
        match self {
            Self::Chain(f) => &f.target,
            Self::Relationship(f) => &f.target,
        }
    }
    pub fn root(&self) -> Option<&Address> {
        match self {
            Self::Chain(f) => Some(&f.root),
            _ => None,
        }
    }
    pub fn linked(&self) -> bool {
        match self {
            Self::Chain(f) => f.linked,
            Self::Relationship(f) => f.linked,
        }
    }
    pub fn valid_names(&self) -> bool {
        let valid = |s: &str| !s.is_empty() && s.len() <= 4096;
        match self {
            Self::Chain(f) => valid(&f.chain) && valid(&f.step),
            Self::Relationship(f) => valid(&f.relationship),
        }
    }
}

struct State {
    available: bool,
    reachable: bool,
    observed: bool,
    reason: &'static str,
}
struct Checker<'a> {
    facts: &'a [LinkFact],
    states: Vec<State>,
    input: usize,
    supported_links: usize,
    positive_witnesses: BTreeSet<String>,
    roots: BTreeMap<String, usize>,
    chain_summaries: Vec<Value>,
}
impl Observer for Checker<'_> {
    fn work_units(&self) -> u64 {
        self.facts.len() as u64 + 1
    }
    fn chain_step(
        &mut self,
        chain: &Chain,
        step: &ChainStep,
        roots: &[&LogEntry],
        sources: &[&LogEntry],
    ) {
        let root = roots[0];
        let root_id = format!(
            "{}:{}:{}:{:?}",
            chain.id, self.input, root.source_line_number, root.source_row_path
        );
        if !self.roots.contains_key(&root_id) {
            self.roots.insert(root_id, self.chain_summaries.len());
            self.chain_summaries.push(json!({"input":self.input,"chain":chain.id,"root_source":evidence::source(root),"root_occurrences":roots.len()}));
        }
        for (fact, state) in self.facts.iter().zip(&mut self.states) {
            if let LinkFact::Chain(f) = fact
                && f.chain == chain.id
                && f.step == step.id
                && roots.iter().any(|e| f.root.matches(self.input, e))
                && sources.iter().any(|e| f.source.matches(self.input, e))
            {
                state.reachable = true;
            }
        }
    }
    fn chain_group(
        &mut self,
        chain: &Chain,
        step: &ChainStep,
        roots: &[&LogEntry],
        sources: &[&LogEntry],
        targets: &[&LogEntry],
        supported: bool,
    ) {
        if supported {
            self.supported_links = self
                .supported_links
                .saturating_add(sources.len().saturating_mul(targets.len()));
        }
        for fact in self.facts {
            if let LinkFact::Chain(f) = fact
                && f.chain == chain.id
                && f.step == step.id
                && roots.iter().any(|e| f.root.matches(self.input, e))
                && sources.iter().any(|e| f.source.matches(self.input, e))
                && targets.iter().any(|e| f.target.matches(self.input, e))
                && supported
                && f.linked
            {
                self.positive_witnesses.insert(
                    json!([
                        chain.id,
                        step.id,
                        self.input,
                        roots[0].source_line_number,
                        roots[0].source_row_path,
                        f.source,
                        f.target
                    ])
                    .to_string(),
                );
            }
        }
        for (fact, state) in self.facts.iter().zip(&mut self.states) {
            if let LinkFact::Chain(f) = fact
                && f.chain == chain.id
                && f.step == step.id
                && roots.iter().any(|e| f.root.matches(self.input, e))
                && sources.iter().any(|e| f.source.matches(self.input, e))
                && targets.iter().any(|e| f.target.matches(self.input, e))
            {
                state.observed = supported;
                state.reason = if supported {
                    "scoped_link_observed"
                } else {
                    "ambiguous_cardinality_or_self_join"
                };
            }
        }
    }
    fn chain_later(&mut self, chain: &Chain, roots: &[&LogEntry], activity: Option<&[&LogEntry]>) {
        let root = roots[0];
        let root_id = format!(
            "{}:{}:{}:{:?}",
            chain.id, self.input, root.source_line_number, root.source_row_path
        );
        let summary = &mut self.chain_summaries[self.roots[&root_id]];
        let records:Vec<_> = activity.unwrap_or(&[]).iter().rev().take(3).collect::<Vec<_>>().into_iter().rev().map(|entry|json!({"source":evidence::source(entry),"text":entry.message.chars().take(512).collect::<String>(),"omitted_characters":entry.message.chars().count().saturating_sub(512)})).collect();
        summary["later_activity_observed_count"] = json!(activity.map(<[_]>::len));
        summary["omitted_later_activity"] =
            json!(activity.map(|events| events.len().saturating_sub(records.len())));
        summary["later_activity"] = json!(records);
    }
    fn relationship_group(
        &mut self,
        relationship: &Relationship,
        sources: &[&LogEntry],
        targets: &[&LogEntry],
        supported: bool,
    ) {
        if supported {
            let self_joins = sources
                .iter()
                .filter(|s| targets.iter().any(|t| std::ptr::eq(**s, *t)))
                .count();
            self.supported_links = self.supported_links.saturating_add(
                sources
                    .len()
                    .saturating_mul(targets.len())
                    .saturating_sub(self_joins),
            );
        }
        for (fact, state) in self.facts.iter().zip(&mut self.states) {
            if let LinkFact::Relationship(f) = fact
                && f.relationship == relationship.id
                && let Some(source) = sources.iter().find(|e| f.source.matches(self.input, e))
                && let Some(target) = targets.iter().find(|e| f.target.matches(self.input, e))
            {
                state.observed = supported && !std::ptr::eq(*source, *target);
                state.reason = if state.observed {
                    "scoped_link_observed"
                } else {
                    "ambiguous_cardinality_or_self_join"
                };
                if state.observed && f.linked {
                    self.positive_witnesses
                        .insert(serde_json::to_string(fact).unwrap());
                }
            }
        }
    }
}
fn resolve<'a>(
    address: &Address,
    inputs: &'a [Vec<LogEntry>],
    filter: &LogFilter,
) -> Result<&'a LogEntry, &'static str> {
    let entries = inputs.get(address.input).ok_or("missing_input")?;
    let mut found = entries.iter().filter(|e| address.matches(address.input, e));
    let entry = found.next().ok_or("missing_source")?;
    if found.next().is_some() {
        return Err("ambiguous_source");
    }
    if !filter.matches(entry) {
        return Err("filtered_out_source");
    }
    Ok(entry)
}
fn initial_state(
    fact: &LinkFact,
    inputs: &[Vec<LogEntry>],
    filter: &LogFilter,
    config: &AnalyzerConfig,
    matchers: &[Matcher<'_>],
) -> Result<bool, &'static str> {
    let source = resolve(fact.source(), inputs, filter)?;
    let target = resolve(fact.target(), inputs, filter)?;
    let policy = config
        .investigation
        .as_ref()
        .ok_or("relationship_policy_unavailable")?;
    let role = |id: &str, entry: &LogEntry| {
        matches!(entry.classification, Some(ClassifiedRecord::Event { .. }))
            && matchers.iter().any(|r| r.role.id == id && r.matches(entry))
    };
    match fact {
        LinkFact::Chain(f) => {
            let chain = policy
                .chains
                .iter()
                .find(|c| c.id == f.chain)
                .ok_or("chain_not_configured")?;
            let step = chain
                .steps
                .iter()
                .find(|s| s.id == f.step)
                .ok_or("step_not_configured")?;
            let root = resolve(&f.root, inputs, filter)?;
            if !role(&chain.root_role, root) {
                return Err("root_role_not_recognized");
            }
            let root_fields: Vec<_> = chain
                .identity_fields
                .iter()
                .chain(&chain.scope_fields)
                .cloned()
                .collect();
            if key(root, &root_fields).is_none() {
                return Err("root_identity_unavailable");
            }
            let from_role = if step.from == "$root" {
                &chain.root_role
            } else {
                &chain.steps.iter().find(|s| s.id == step.from).unwrap().role
            };
            if !role(from_role, source) || !role(&step.role, target) {
                return Err("endpoint_role_not_recognized");
            }
            let fields: Vec<_> = step
                .join_fields
                .iter()
                .chain(&chain.scope_fields)
                .cloned()
                .collect();
            if key(source, &fields).is_none() || key(target, &fields).is_none() {
                return Err("join_identity_unavailable");
            }
            Ok(false)
        }
        LinkFact::Relationship(f) => {
            let relation = policy
                .relationships
                .iter()
                .find(|r| r.id == f.relationship)
                .ok_or("relationship_not_configured")?;
            if !role(&relation.source_role, source) || !role(&relation.target_role, target) {
                return Err("endpoint_role_not_recognized");
            }
            let mut fields = relation.join_fields.clone();
            for field in &relation.required_scope_fields {
                if !fields.contains(field) {
                    fields.push(field.clone());
                }
            }
            if crate::investigation_runtime::policy::identity(source, &fields).is_none()
                || crate::investigation_runtime::policy::identity(target, &fields).is_none()
                || crate::perf_analyzer::correlation_scope(source, config).is_none()
                || crate::perf_analyzer::correlation_scope(target, config).is_none()
            {
                return Err("join_identity_unavailable");
            }
            // Direct relationships abstain entirely when any role-matching record is unresolved.
            if inputs[f.source.input]
                .iter()
                .filter(|e| filter.matches(e))
                .any(|e| {
                    matchers.iter().any(|r| {
                        (r.role.id == relation.source_role || r.role.id == relation.target_role)
                            && r.matches(e)
                    }) && !matches!(e.classification, Some(ClassifiedRecord::Event { .. }))
                })
            {
                return Err("unresolved_role_cardinality");
            }
            Ok(true)
        }
    }
}

pub(crate) fn analyze(
    inputs: &[Vec<LogEntry>],
    config: &AnalyzerConfig,
    filter: &LogFilter,
    facts: &[LinkFact],
) -> (Value, Vec<Value>) {
    let matchers: Vec<_> = config
        .investigation
        .as_ref()
        .map_or_else(Vec::new, |p| p.roles.iter().map(Matcher::new).collect());
    let mut checker = Checker {
        facts,
        states: facts
            .iter()
            .map(
                |f| match initial_state(f, inputs, filter, config, &matchers) {
                    Ok(reachable) => State {
                        available: true,
                        reachable,
                        observed: false,
                        reason: "no_scoped_link",
                    },
                    Err(reason) => State {
                        available: false,
                        reachable: false,
                        observed: false,
                        reason,
                    },
                },
            )
            .collect(),
        input: 0,
        supported_links: 0,
        positive_witnesses: BTreeSet::new(),
        roots: BTreeMap::new(),
        chain_summaries: Vec::new(),
    };
    let mut budget = Budget::new(Limits {
        input_bytes: u64::MAX,
        records: u64::MAX,
        expanded_records: u64::MAX,
        work_units: 20_000_000,
        elapsed_ms: 10_000,
        memory_bytes: 256 * 1024 * 1024,
        artifact_bytes: u64::MAX,
        record_bytes: usize::MAX,
        cancel_file: None,
    });
    let mut summaries = Vec::new();
    if let Some(policy) = &config.investigation {
        for (input, entries) in inputs.iter().enumerate() {
            checker.input = input;
            let mut selected = Vec::new();
            for entry in entries.iter().filter(|e| filter.matches(e)) {
                if !budget.checkpoint("relationships", 1)
                    || !budget.reserve(
                        "relationships",
                        (entry.raw_logline.len() as u64)
                            .saturating_mul(64)
                            .saturating_add(16 * 1024),
                    )
                {
                    break;
                }
                selected.push(entry.clone());
            }
            if budget.stop.is_some() {
                break;
            }
            let mut findings = Vec::new();
            crate::investigation_runtime::chains::calculate(
                policy,
                config,
                &selected,
                &[],
                "validation",
                &evidence::Context::default(),
                &Value::Null,
                true,
                &mut budget,
                &mut findings,
                Some(&mut checker),
            );
            for finding in &findings {
                if let Some(chain) = finding["details"].get("event_chain") {
                    summaries.push(json!({"input":input,"chain":chain["id"],"root_occurrences":chain["root_occurrences"],"ambiguity":chain["ambiguity"],"completeness":chain["completeness"],"steps":chain["steps"].as_array().unwrap().iter().map(|s|json!({"step":s["id"],"from":s["from"],"status":s["status"],"observed_count":s["observed_count"],"linked_count":s["linked_count"],"count_completeness":s["count_completeness"],"ambiguity":s["ambiguity"]})).collect::<Vec<_>>(),"later_activity_observed_count":chain["later_activity_observed_count"]}));
                }
            }
            // Population reports are not needed for link validation.
            let mut direct_policy = policy.clone();
            direct_policy.populations.clear();
            direct_policy.chains.clear();
            crate::investigation_runtime::policy::calculate(
                &direct_policy,
                config,
                &selected,
                &[],
                "validation",
                &evidence::Context::default(),
                &Value::Null,
                &mut budget,
                &mut Vec::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                Some(&mut checker),
            );
            if budget.stop.is_some() {
                break;
            }
        }
    }
    let checks:Vec<_> = facts.iter().zip(&checker.states).map(|(fact,state)| {
        let available = state.available && state.reachable && budget.stop.is_none();
        let passed = available && state.observed == fact.linked();
        let mut check = json!({"type":"link","assertion":fact,"source":resolve(fact.source(),inputs,filter).ok().map(evidence::source),"target_source":resolve(fact.target(),inputs,filter).ok().map(evidence::source),"root_source":fact.root().and_then(|a|resolve(a,inputs,filter).ok()).map(evidence::source),"observed_linked":if available{Some(state.observed)}else{None},"observed_available":available,"status":if passed{"passed"}else{"failed"},"reason":if budget.stop.is_some(){"relationship_analysis_stopped"}else if !state.available{state.reason}else if !state.reachable{"predecessor_not_linked"}else if passed{"matches_expected_link"}else{"link_mismatch"},"link_reason":state.reason});
        for field in ["source", "target_source", "root_source"] {
            if check[field].is_null() { check.as_object_mut().unwrap().remove(field); }
        }
        check
    }).collect();
    let failed = checks.iter().filter(|c| c["status"] == "failed").count();
    let configured = config
        .investigation
        .as_ref()
        .is_some_and(|p| !p.chains.is_empty() || !p.relationships.is_empty());
    (
        json!({"status":if budget.stop.is_some(){"unavailable"}else if !configured{"not_configured"}else if facts.is_empty(){"not_asserted"}else if failed>0{"failed"}else{"passed_on_asserted_links"},"processing":{"status":if budget.stop.is_some(){"partial"}else{"complete"},"stop":budget.stop,"limits":{"work_units":budget.limits.work_units,"elapsed_ms":budget.limits.elapsed_ms,"memory_bytes":budget.limits.memory_bytes}},"asserted_links":facts.len(),"positive_checks":facts.iter().filter(|f|f.linked()).count(),"negative_checks":facts.iter().filter(|f|!f.linked()).count(),"failed_checks":failed,"supported_links":if budget.stop.is_none(){Some(checker.supported_links)}else{None},"unasserted_supported_links":if budget.stop.is_none(){Some(checker.supported_links.saturating_sub(checker.positive_witnesses.len()))}else{None},"roots":checker.chain_summaries,"chains":summaries,"limitations":["Assertions verify declared scoped links on this sample; source order proves neither cause nor elapsed time","Negative assertions require recognized endpoints and usable identities; unavailable predecessors do not establish separation","Passing witnesses do not establish missed relationships, capture completeness or intended domain meaning"]}),
        checks,
    )
}
