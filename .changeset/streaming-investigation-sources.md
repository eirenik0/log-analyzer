---
default: minor
---

Add a fallible record visitor and stream investigation source capture, parsing,
and artifact source serialization through bounded buffers. Preserve normalized
row addresses, multiline boundaries, checksums, redaction, and partial-processing
contracts. Use temporary source snapshots instead of retaining complete input
buffers; keep version-1 JSON artifacts and existing parser collection APIs.

Correlation and retained analysis remain memory-budgeted and do not yet spill.
