---
default: minor
---

Add an evidence-backed completion checklist to the analysis skill, distinguishing
processing coverage, scoped terminal events and outcome records. Add completion
regressions and positive/negative evaluation coverage, with an opt-in fingerprinted
skill entrypoint for model comparisons. Scripted checks do not establish improved
model reasoning.

Add bounded automatic profile detection to `investigate`, with explicit
`--config`/`--preset` overrides, evidence metadata and generic fallback on ambiguity.
Require an initial bounded investigation before
custom parsing, with a documented remaining gap for supplementary scripts. Clarify
structural coverage disclaimers and add matching text/JSON guidance to `info`.

Include profiles from the working directory's `config` folder (or `--profiles-dir`)
in automatic detection. Bound recursive discovery and inherited-source reads,
deduplicate equivalent effective configurations, retain origin/dependency hashes,
and report invalid or incomplete discovery. Support custom normalization candidates
without requiring built-in parsers to recognize their inputs.
