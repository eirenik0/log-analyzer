---
default: patch
---

Release unused worst-case source amplification after parsing native text records,
accounting for retained text, structured fields and payload storage. Preserve
pre-parse reservations, dense-data allowances, classification accounting and
default processing limits so multiline investigations can retain more evidence
without raising the configured budget.
