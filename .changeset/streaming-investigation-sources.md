---
default: minor
---

Add a fallible record visitor and stream investigation source capture, parsing,
and artifact source serialization through bounded buffers. Preserve normalized
row addresses, multiline boundaries, checksums, redaction, and partial-processing
contracts. Use temporary source snapshots instead of retaining complete input
buffers; keep version-1 JSON artifacts and existing parser collection APIs.

Spill pending correlation indexes to SQLite only after the resident index limit,
with explicit disk limits, cancellation, temporary-state cleanup and whole-key
ambiguity decisions. Expose a consuming library correlator and measure its RSS
using selected starts followed by late completions.

Retrieve version-1 artifacts through bounded JSON offset/reference indexes and
semantic validation passes without materializing the entire artifact. Use plain
temporary files for retrieval state, preserve exact arithmetic and source checks,
and spool complete output pages. Add item, index, validation-state, work and time
limits, corruption regressions, and selected-record retrieval RSS measurements.

Stream CLI source replay, completed correlations, exact distributions, membership
and sequence evidence, and report construction through disposable row files.
Use spillable keyed analysis for policy, chains, resource joins and scope ambiguity.
Preserve deterministic validation, scoped uncertainty and independent evaluation
oracles. Stream complete text/JSON and saved reports without retaining the output.
Add a construction disk allowance and selected-input end-to-end RSS regression
checks covering capture through checked artifact publication and report delivery.
