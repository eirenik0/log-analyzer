# Performance efficiency

Measurable requirements for performance efficiency. Status records the target's lifecycle, not proof that every workload meets it.

## NFR-003 Bounded processing and delivery

Resource limits must remain observable without turning interrupted processing into complete analysis.

**Target** — 0 supported assessments in the tested processing-cutoff cases; 0 undeclared report-budget overruns across the byte/character budget fixtures.

**Measurement** — Run [[tests/test_investigation_runtime.rs#limits_and_cancellation_return_valid_partial_processed_populations]] and [[tests/test_report_budget.rs#byte_and_character_limits_apply_together_on_every_returned_page]]. Use [[scripts/measure-investigation-memory.py]] to record peak RSS separately.

**Rationale** — Large captures must produce explicit partial results or errors within the tool's declared accounting and output rules.

**Scope** — Configured processing limits and serialized report budgets, including declared mandatory-metadata exceptions. No production throughput, latency or hard RSS guarantee is established.

**Owner** — Log Analyzer maintainers

**Status** — agreed

**Review cadence** — After budget, normalization or serialization changes; before release.

**Links** — [[src/processing.rs#Budget]], [[contracts/bounded-reports]], [[adr-002-snapshot-bound-evidence]].

**Last reviewed** — 2026-10-13
