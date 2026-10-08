---
"log-analyzer": minor
---

Add shared presentation redaction for text/JSON reports and files, nested fields and encoded queries, stable optional ID masking, and explicit redaction metadata. Preserve long correlation IDs in compact payloads.

Keep numeric masks out of report metadata, truncate expanded messages on UTF-8 boundaries, and index identifier matching without rescanning the full ID set per entry.
