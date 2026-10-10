---
default: minor
---

Add an evidence-backed completion checklist to the analysis skill, distinguishing
processing coverage, scoped terminal events and outcome records. Add completion
regressions and positive/negative evaluation coverage, with an opt-in fingerprinted
skill entrypoint for model comparisons. Scripted checks do not establish improved
model reasoning.

Add bounded automatic profile detection to `investigate`, with explicit
`--profile` overrides (with `--config`/`--preset` compatibility), evidence metadata and generic fallback on ambiguity.
Require an initial bounded investigation before
custom parsing, with a documented remaining gap for supplementary scripts. Clarify
structural coverage disclaimers and add matching text/JSON guidance to `info`.

Include profiles from the working directory's `config` folder (or `--profiles-dir`)
in automatic detection. Bound recursive discovery and inherited-source reads,
deduplicate equivalent effective configurations, retain origin/dependency hashes,
and report invalid or incomplete discovery. Support custom normalization candidates
without requiring built-in parsers to recognize their inputs.

Unify built-in and file-based profile selection under the global `--profile` option
and `LOG_ANALYZER_PROFILE`, preserving hidden legacy selectors and persisted contracts. Show only `--profile`
in CLI help and current usage guidance.
Advertise the interface in capabilities, include selected-profile origins and hashes,
and update portable/Claude skill guidance and current examples.

Keep the skill concise and current: investigate, retrieve, verify completion,
resolve specific gaps, and report supported conclusions. Remove legacy command
workflows and keep only `--profile` for explicit selection.
