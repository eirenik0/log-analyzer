---
default: major
---

Remove legacy `[profile.session_prefixes]` configuration in favor of `[[sessions.levels]]` only.

- `AnalyzerConfig` session insights now read only from `sessions.levels`.
- `generate-config` no longer writes `profile.session_prefixes` and uses `level-1`, `level-2`, ... for inferred generic level names.
- Built-in templates, README examples, and Claude skill templates/docs now use `[[sessions.levels]]` exclusively.
