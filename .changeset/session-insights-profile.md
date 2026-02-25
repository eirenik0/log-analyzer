---
default: minor
---

Add profile-driven hierarchical session insights for `info` using a new optional `[[sessions.levels]]` config format.

- Supports named session levels with `segment_prefix`, `create_command`, `complete_commands`, and `summary_fields`.
- Upgrades profile analysis to build per-session lifecycle state (created/completed), parent-child links, operation counts, and create-time summary field extraction in a single pass.
- `info` now renders per-level session completion health summaries (completed vs incomplete) and stable configured summary field values when available.
- `generate-config` now emits detected session prefixes as generic `[[sessions.levels]]` entries (`level-1`, `level-2`, ...) while preserving template-defined session levels (for example `config/profiles/eyes.toml`).
