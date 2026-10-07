---
"log-analyzer": patch
---

Fix #1: reject nonempty input with no recognized entries (exit 1) and expose per-file parser/profile, byte size, parsed entries, and rejected candidates in info/errors/perf text and JSON reports. Distinguish empty input and zero filter matches from parsing failure, without counting multiline continuations as rejections.
