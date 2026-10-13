---
status: accepted
date: 2026-10-13
authors: [Codex]
consulted: []
supersedes: []
superseded-by: null
---
# ADR 003: Verification boundaries and independent expectations

Independent checks establish specific guarantees without treating consistency as universal truth.

## Context

A schema-valid report can contain wrong arithmetic; a consistent fact can refer to the wrong event; a plausible explanation can assert unsupported causality.

## Decision

Separate schema, relational, subject, profile-expectation and prose checks.

Keep deterministic validation in the tool, require independent expectations for semantic adaptation, and disclose unavailable checks rather than declaring universal verification.

## Alternatives considered

The alternatives below explain the tradeoffs of the implemented choice.

- Treat successful schema validation as proof: it checks shape, not arithmetic, evidence support or intended meaning.
- Use generated claims as their own ground truth: this preserves a wrong subject or a profile that consistently misclassifies records.

## Consequences

Checks give precise guarantees and actionable failures. The caller still owns intended subject and domain meaning; optional claim verification does not enforce workflow completion or establish the truth of arbitrary prose.

## Scope of applicability

Retained relation validation, optional subject-bound claims and profile validation. The private checked-result interface in [[checked-analysis]] remains a proposal.

## Review triggers

Review when adding claim kinds, changing checklist semantics, or requiring runner enforcement. Re-evaluate using independently specified facts and held-out cases.

## Links

Implementation and quality requirements constrain this decision.

[[src/investigation.rs#validate_relations]], [[src/investigation_runtime/claims.rs#subject_check]], [[src/profile_validation.rs#analyze]], [[contracts/profile-validation]], [[reliability]], [[usability]].
