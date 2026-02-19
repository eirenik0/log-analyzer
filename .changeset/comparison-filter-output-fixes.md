---
default: patch
---

Fix comparison, filter, and output behavior regressions:

- Pair repeated shared keys one-to-one and surface unmatched occurrences as unique entries instead of dropping them.
- Fix `--sort-by time/component/level/type` behavior to use structured key parts and actual timestamps.
- Make `--full` comparison output print full payload JSON.
- Ensure `-o/--output` writes correct output for compare/diff/llm-diff/process and perf (text and JSON).
- Update filter semantics so different filter types are AND-ed while multiple values of the same type are OR-ed.
- Improve unique entry display so request details and unpaired annotations remain visible.
