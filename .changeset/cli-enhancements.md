---
default: minor
---

Improve CLI usability with new global flags:

- Added `-j, --json` as shorthand for `-F json -c` for compact machine-readable output.
- Added `-f, --filter` for unified filter expressions (for example: `c:core l:ERROR !t:timeout`).
- `-f, --filter` can also be set with `LOG_ANALYZER_FILTER`.
- `--json` conflicts with explicit `-F, --format` to avoid ambiguous output settings.
