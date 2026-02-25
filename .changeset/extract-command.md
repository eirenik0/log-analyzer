---
default: minor
---

Add an `extract` command for aggregating payload field values from matching log entries.

- `log-analyzer extract <file> --field <name>` extracts a JSON payload/settings field and groups by value occurrences.
- Works with the existing global `-f/--filter` expression syntax to scope extraction to specific messages/components.
- Supports JSON output via global `-F json` / `-j` and dot-path field access (for example `settings.retryTimeout`).
