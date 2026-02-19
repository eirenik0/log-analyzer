---
default: patch
---

Fix regressions in compare/diff filtering and output:

- Repeated shared keys are now paired one-to-one; unmatched occurrences are kept as unique entries.
- `--sort-by time/component/level/type` now sorts correctly.
- `--full` now prints full payload JSON in comparison output.
- `-o, --output` now writes the correct output for `compare`, `diff`, `llm-diff`, `process`, and `perf` (text and JSON).
- Filter logic is now consistent: different filter types are AND-ed, multiple values of the same type are OR-ed.
- `diff` output now includes unpaired unique entries in both text and JSON modes.
- Parser no longer panics when profile config has empty `command_payload_markers`.
