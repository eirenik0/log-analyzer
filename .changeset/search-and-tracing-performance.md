---
default: patch
---

Avoid repeatedly visiting overlapping search context windows and scanning whole
tracing-message suffixes for structured field separators. Preserve search ordering,
match markers, chunk boundaries, and tracing field semantics. Add a reproducible
synthetic release-binary comparison script with output equality checks.
