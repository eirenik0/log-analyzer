---
default: patch
---

Keep retained input coverage and normalization diagnostics charged against the
investigation memory budget when releasing independent parser working sets. Measure
new metadata without allocating a serialized copy; preserve diagnostics and report
an explicit partial outcome when cumulative retention exhausts the allowance.
