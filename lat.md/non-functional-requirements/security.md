# Evidence privacy

Measurable requirements for evidence privacy. Status records the target's lifecycle, not proof that every workload meets it.

## NFR-004 Redacted artifact retention

Redacted artifacts must not retain source content that their verification guarantees declare unavailable.

**Target** — 0 retained original capture data, raw record text, messages, record field payloads or effective-profile content in the redacted-artifact regression.

**Measurement** — Run [[tests/test_investigation_runtime.rs#redacted_artifact_omits_payload_rules_query_and_paths]] and [[regression-tests#Evidence regressions#Redacted verification]]. Inspect both saved bytes and reported verification availability.

**Rationale** — Saving raw evidence behind a redacted report would disclose data the user intended to omit.

**Scope** — Explicitly redacted retained artifacts. This does not promise anonymization of arbitrary content or prevent equality inference from digests.

**Owner** — Log Analyzer maintainers

**Status** — agreed

**Review cadence** — After adding retained fields, changing redaction or changing verification guarantees; before release.

**Links** — [[src/investigation_runtime/artifact.rs#redact]], [[adr-002-snapshot-bound-evidence]].

**Last reviewed** — 2026-10-13
