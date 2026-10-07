---
"log-analyzer": patch
---

Fix #2: recognize browser-console source prefixes on classic log entries and multiline continuations. Preserve original raw text and physical source lines, retain locations in the console_source structured field, and support filename/path/URL locations with line and optional column numbers.
