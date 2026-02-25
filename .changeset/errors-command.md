---
default: minor
---

Add an `errors` command for single-command failure diagnosis across one or more log files.

- Clusters ERROR entries (and optionally WARN entries via `--warn`) by normalized message pattern.
- Shows per-cluster severity, counts, emitting components, first/last timestamps, and a sample message.
- Optionally cross-references affected `component_id` sessions via `--sessions`, including `completed` vs `orphaned` outcomes using perf-style orphan detection heuristics.
- Adds impact-oriented cluster sorting (`--sort-by impact`) plus blocking-span estimates in the summary.
