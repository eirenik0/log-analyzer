---
status: accepted
date: 2026-10-13
authors: [Codex]
consulted: []
supersedes: []
superseded-by: null
---
# ADR 002: Snapshot-bound evidence and bounded reports

Snapshot-bound artifacts separate historical evidence from mutable inputs and bounded presentation.

## Context

Logs may change or disappear after analysis, while display limits omit evidence. Reused identifiers, partial captures and pagination must not silently change a conclusion.

## Decision

Bind evidence to consumed inputs, effective configuration and query. Retain scoped occurrences, populations and findings separately from presentation, validate exact artifact bytes before publication, and bind retrieval to the saved digest.

## Alternatives considered

The alternatives below explain the tradeoffs of the implemented choice.

- Reparse original files for every follow-up: changing inputs would replace the evidence behind an earlier answer.
- Retain only the displayed summary: omitted witnesses and population membership would be unavailable for verification.

## Consequences

Historical retrieval remains stable and bounded output can expose omissions honestly.

Retention costs disk space and can preserve sensitive source data; deletion and explicit redaction remain necessary. Processing bounds are distinct from presentation bounds.

## Scope of applicability

Retained investigations and their retrieval. Command-specific report cursors bind to recomputed reports instead. Conservative memory accounting is not a hard RSS ceiling.

## Review triggers

Review when changing artifact format, redaction, retention, cursor identity or moving storage outside the local filesystem.

## Links

Implementation and quality requirements constrain this decision.

[[src/investigation_runtime.rs#investigate]], [[src/investigation_runtime/artifact.rs#retrieve]], [[src/processing.rs#Budget]], [[contracts/investigation-contract]], [[performance-efficiency]], [[security]].
