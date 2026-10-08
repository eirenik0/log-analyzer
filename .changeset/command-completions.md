---
"log-analyzer": patch
---

Recognize quoted command completion lines across shipped profiles and pair them with starts. Parse command names independently of lifecycle markers, excluding names and embedded JSON from boundary matching while preserving legacy unquoted start-delimited names.
