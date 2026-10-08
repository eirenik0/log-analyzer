---
"log-analyzer": patch
---

Correlate record-field scope values longer than 4096 bytes instead of reporting them as `missing_scope_field`. Such a value becomes a readable prefix plus its byte length and a 128-bit FNV-1a digest, so equal long scopes pair. Encode short values containing the reserved digest marker as well, preventing raw summaries from impersonating long scopes. Explicit event-rule scope mappings use the same reserved-marker encoding after validation and retain their existing input limit. Document the bounded keys and digest limitations in the README, CLI help, and analysis skill.
