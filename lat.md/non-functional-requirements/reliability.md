# Reliability

Measurable requirements for reliability. Status records the target's lifecycle, not proof that every workload meets it.

## NFR-001 Evidence correctness

Evidence checks must reject unsupported certainty in the declared regression corpus.

**Target** — 0 accepted wrong-subject or contradicted claims, 0 invented intervals or supported zeros from unavailable evidence, and 0 hidden verification losses across the 7 linked evidence regressions.

**Measurement** — Run the tests linked from [[regression-tests]] through `cargo test --locked`; record failures and fixture changes. Passing this finite corpus is not proof of correctness on arbitrary logs.

**Rationale** — Incorrect certainty can turn an unrelated event or missing boundary into a false incident explanation.

**Scope** — Existing synthetic evidence regressions and their declared semantics.

**Owner** — Log Analyzer maintainers

**Status** — agreed

**Review cadence** — After changes to parsing, correlation, verification or profile semantics; before release.

**Links** — [[regression-tests]], [[adr-001-shared-event-semantics]], [[adr-003-verification-boundaries]].

**Last reviewed** — 2026-10-13


## NFR-002 Historical retrieval

Retained retrieval must remain usable after original sources disappear and reject altered artifact identities.

**Target** — 0 parsing and correlation passes during retained retrieval; reject every changed artifact digest and incompatible cursor in the retrieval regression.

**Measurement** — Run [[tests/test_investigation_runtime.rs#retrieval_is_snapshot_bound_and_does_not_need_sources]]; it removes the source, checks missing-source disclosure, and mutates the artifact and cursor selection.

**Rationale** — Follow-up evidence must refer to the same analysis rather than silently analyzing a newer capture.

**Scope** — Local retained-artifact retrieval; current-original revalidation is a separate check.

**Owner** — Log Analyzer maintainers

**Status** — agreed

**Review cadence** — After artifact, cursor or source-verification changes; before release.

**Links** — [[src/investigation_runtime/artifact.rs#retrieve]], [[adr-002-snapshot-bound-evidence]].

**Last reviewed** — 2026-10-13
