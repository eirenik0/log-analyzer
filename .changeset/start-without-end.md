---
"log-analyzer": minor
---

Add `end_expected = false` to version-2 event-rule mappings for start records that never get an end record. `perf` counts them as `start_only_events` instead of `missing_end` orphans, without measuring durations, and session levels can create and complete on them. Operation-type filters count excluded start-only records as suppressed while preserving relevant-event totals. The option is rejected in version 1 and on rules without a start phase. Document the behavior in CLI help and the analysis skill.
