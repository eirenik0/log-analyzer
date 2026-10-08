---
"log-analyzer": patch
---

Correlate record-field scope values longer than 4096 bytes instead of reporting them as `missing_scope_field`. Such a value becomes a readable prefix plus its byte length and a 128-bit FNV-1a digest, so equal long scopes pair and different ones do not.
