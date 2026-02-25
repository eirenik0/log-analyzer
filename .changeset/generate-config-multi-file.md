---
default: minor
---

Add multi-file input support to `generate-config`.

- `log-analyzer generate-config` now accepts one or more log files and merges them before inferring profile hints.
- This improves profile generation for split/rotated logs from the same run by combining observed components, commands, requests, and session prefixes.
- Generated output now includes a multi-source header when multiple files are provided.
