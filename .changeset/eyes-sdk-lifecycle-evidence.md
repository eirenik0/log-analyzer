---
"log-analyzer": patch
---

Restrict Eyes SDK request completion suffixes to recognized response and retry forms so pending prose cannot create measured operations. Decode default-driver command settings using the same payload rules as regular settings. Keep the analysis skill template synchronized and cover both cases with synthetic regression tests.

Validate standalone response body container boundaries with an opt-in version-2 text capture guard. Reject pending prose after object/array bodies, including prose followed by another container, while preserving nested JSON5 bodies and explicit SDK retry tails.

Recognize LF, CR, and Unicode line/paragraph separators as JSON5 line-comment terminators in body scanning, trailing-comment validation, and undefined-value normalization so comments cannot hide pending prose.
