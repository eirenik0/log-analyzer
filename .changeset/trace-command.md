---
default: minor
---

Add a `trace` command for following a single operation/session lifecycle across log files.

- `log-analyzer trace` accepts one or more log files and merges/sorts entries by timestamp.
- Supports `--id <substring>` to trace by correlation/request ID fragments and `--session <substring>` to trace by `component_id` hierarchy.
- Text output shows chronological entries with per-step timing deltas; JSON output is also available via global `-F json` / `-j`.
