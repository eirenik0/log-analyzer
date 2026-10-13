---
status: accepted
date: 2026-10-13
authors: [Codex]
consulted: []
supersedes: []
superseded-by: null
---
# ADR 001: Shared event semantics

Shared event semantics keep analysis consumers consistent without inferring missing domain meaning.

## Context

Parsing, performance analysis, retained investigations and profile validation need consistent event meaning and rule provenance.

## Decision

Classify each parsed record once and retain owned semantics, rule IDs and conflicts. Consumers use the shared classification and correlation engines, with explicit scope and timestamp provenance.

## Alternatives considered

The alternatives below explain the tradeoffs of the implemented choice.

- Classify independently in every consumer: divergent matching and pairing would make reports disagree.
- Infer lifecycle meaning from generic message similarity: matching text cannot establish scope, phase or completion.

## Consequences

Shared engines reduce semantic drift and make profile validation representative. They also couple consumers to the classification contract; extending that contract needs cross-command regression coverage.

## Scope of applicability

Profile-driven classification and lifecycle analysis. Parsing a record does not prove that the profile models its domain correctly.

## Review triggers

Review when adding an event kind, changing correlation identity, or introducing a consumer that cannot represent classification conflicts.

## Links

Implementation and quality requirements constrain this decision.

[[src/event_rules/mod.rs#CompiledEventRules#classify_owned]], [[src/perf_analyzer/mod.rs#analyze_performance_checked]], [[contracts/event-classification]], [[reliability]].
