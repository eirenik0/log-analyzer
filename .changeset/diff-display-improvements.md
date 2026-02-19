---
default: minor
---

Improve `diff` output readability and diagnostics:

- Differences are now classified as added, removed, or modified (`+`, `-`, `~`).
- Summary output now includes counts of additions, removals, and modifications.
- Diff entries now include source line numbers for faster navigation to original logs.
- JSON diff output now includes `change_type`, and text diffs are split into `text1`/`text2`.
