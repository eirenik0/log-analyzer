# Checked analysis design

This proposed implementation applies the [[philosophy|design philosophy]]. It describes future work, not current behavior.

## Product contract

Keep `log-analyzer investigate application.log` as the entry point. Default text
is concise; `--verbose` exposes diagnostics. JSON and text use the same findings.

Follow-up evidence retrieval uses the original capture, without parsing a changing
log again. Existing explicit profile selection remains available.

## One checked result

Introduce a private checked-analysis module between evidence construction and
presentation. It accepts captured evidence and profile semantics, returning
a checked result or an error. Renderers cannot publish
unchecked measurements.

Reuse [[src/investigation.rs#validate_relations|the existing relation validator]] and retained
evidence; do not build a second claim verifier or parallel artifact format.

Each answerable finding keeps these together:

- Subject: independent input, scoped event identity and source occurrence.
- Fact: observed outcome, interval with exact endpoints, or count with its exact
  population and completeness. Missing measurements stay absent, never zero.
- Support: stable finding ID, source citations and the rules used.
- Limits: ambiguity, missing semantics, partial coverage or unavailable evidence.

Unknowns identify the affected subject or question and the missing support.
Evidence consistency, profile validation and question coverage are separate states;
none is a universal "answer verified" flag. A consistent result about the wrong
subject must not satisfy a requested question.

```mermaid
flowchart TD
  L[Captured logs] --> E[Scoped evidence and profile semantics]
  E --> C[Checked analysis]
  C --> T[Concise text]
  C --> J[Structured answer]
  C --> R[Retained evidence for follow-ups]
```

## Responsibility and failure behavior

The agent supplies intended subjects explicitly when a question needs selection.
The tool returns all matching candidates when selection is ambiguous; it does not
pick the first or nearest label. Reused IDs remain separated by input and scope.

Compute counts from declared populations and elapsed time from observed, named
endpoints. A failed terminal outcome can still establish completion. Cached results,
later activity and temporal order do not establish completion or causality.
Render factual sentences from checked values; optional agent explanations must
distinguish supported observations from hypotheses.

Before presentation, validate calculations and source bindings. Invalid derived
facts fail publication. Unavailable source verification, redaction and processing
limits remain visible; they must not produce a clean or fully verified result.
Presentation truncation is separate from processing completeness. Keep unknowns
and coverage visible even when routine findings consume the display budget.

Profile adaptation is a later bounded transaction: propose, validate against
independent expectations and negative cases, then rerun. Candidate output cannot
supply its own ground truth. Unsupported semantics remain explicit unknowns.

Mandatory runner enforcement is also a later milestone. The runner owns artifact
binding and question coverage, using the native result. It rejects contradictory
structured answers and reports specific missing support within the original
budget. Arbitrary prose and arbitrary question meaning require separate assessment;
an additional model reviewer is not the source of truth.

## First implementation and acceptance

Implement the checked result and concise renderer for scoped capture/close
intervals, record counts and causal uncertainty.

Preserve parsing, correlation,
source offsets, retention and existing independent-input behavior. Keep advanced
claim verification optional; do not add new CLI steps or runner protocols here.
Capture, close and poll meanings come from profile rules, not hard-coded event
names or evaluation fixtures.

Test through the CLI with independently specified expectations:

- Similar names and reused IDs across scopes select the right event or stay ambiguous.
- A terminal event, including failure, yields its exact interval; a missing end
  yields an unknown. Overlapping starts do not invent a duration.
- Poll sends, responses and cached results have separate count populations.
- Partial, malformed, redacted and empty-filtered captures disclose their limits.
- Text and JSON agree on subjects, endpoints, counts, unknowns and coverage.
- Late terminal events and findings beyond the first page remain recoverable.
- Renaming IDs or adding unrelated input cannot change another subject's answer.

Use existing synthetic evaluation cases as regressions, plus unseen formats and
multi-turn subject changes. Freeze inputs, expected facts, binaries and budgets
before comparison. Use the success criteria in [[philosophy]]. Keep natural
use and mandatory enforcement separate. Paid model trials require an explicit
run budget.

## Existing foundations

The current analyzer already validates retained relations and subject-bound claims.
The proposed interface should reuse these checks rather than duplicate them.

- [[src/investigation.rs#validate_relations|Relation validation]] checks retained
  calculations, populations and source references.
- [[tests/test_investigation_runtime.rs#subject_bound_links_reject_adjacent_names_scopes_ids_and_endpoints|Subject-binding regression]] rejects adjacent names, scopes, IDs and endpoints.
- [[tests/test_investigation_runtime.rs#unparsed_and_generic_inputs_do_not_claim_supported_zero_lifecycles|Coverage regression]] prevents unavailable analysis from becoming a supported zero.
