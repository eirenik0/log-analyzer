---
"log-analyzer": minor
---

Complete shared command/request/event classification and correlation. Shipped profiles and synchronized skill templates now use version-2 global event rules with explicit direction/phase, endpoint and ordered correlation-field mappings; version-1 and legacy custom contracts remain supported. Cache normalized IDs/scopes before analysis and retain pair/orphan provenance. Expose classification coverage independent of operation-type/display selection and parse coverage, with consistent text/JSON redaction and capability metadata. Preserve JSON5 string identities during undefined-value compatibility conversion, support unknown-direction filters, and preserve generic extraction and existing scoped ambiguity/timestamp/ID-reuse safeguards.

Pre-1.0 breaking behavior/API: shipped request/event recognition now follows documented exact grammar; manually constructed operations require cached explicit or legacy evidence, and missing direction remains Unknown. Global rules cannot mix legacy lifecycle markers. Additive report fields retain schema version 1; the former command unknown counter remains a deprecated compatibility alias. See README and docs/design/event-classification.md for migration.
