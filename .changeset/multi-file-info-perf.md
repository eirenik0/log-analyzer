---
default: minor
---

Add multi-file input support for `info` and `perf` commands.

- `log-analyzer info` now accepts one or more log files and aggregates analysis across all inputs.
- `log-analyzer perf` now accepts one or more log files and analyzes them as a single timeline.
- Parsed entries from all provided files are concatenated and sorted by timestamp before analysis, which improves cross-file operation pairing (including orphan detection).
