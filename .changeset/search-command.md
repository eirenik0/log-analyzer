---
default: minor
---

Add a `search` command for structured grep-style log inspection.

- `log-analyzer search <file>` prints matching log entries using the existing `-f/--filter` expression syntax.
- Supports entry-based context windows via `--context <n>` and optional parsed payload display with `--payloads`.
- Supports grouped counting mode via `--count-by <matches|component|level|type|payload>` (including payload-based occurrence grouping).
