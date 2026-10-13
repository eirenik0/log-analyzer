# Usability

Measurable requirements for usability. Status records the target's lifecycle, not proof that every workload meets it.

## NFR-005 One-command initial analysis

A supported capture should produce useful initial analysis through the binary alone.

**Target** — 1 `investigate` invocation and 0 helper-script invocations produce a readable initial report and fresh evidence artifact for the maintained default-output fixture.

**Measurement** — Run [[tests/test_investigation_runtime.rs#readable_default_creates_fresh_artifacts_and_reports_observed_errors]]. This checks the entry point, not completion of every requested question.

**Rationale** — Basic investigation must not require agents or people to assemble proof files before seeing useful results.

**Scope** — Default readable CLI behavior on the maintained fixture. General question completion and concise output across unseen dialogs remain unqualified.

**Owner** — Log Analyzer maintainers

**Status** — agreed

**Review cadence** — After CLI, default-output or skill-workflow changes; before release.

**Links** — [[philosophy]], [[architecture]], [[adr-003-verification-boundaries]].

**Last reviewed** — 2026-10-13
