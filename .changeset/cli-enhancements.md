---
default: minor
---

CLI enhancements for improved usability:

- Added `-j, --json` flag as shorthand for `-F json -c`
  - Automatically implies compact mode for LLM-friendly output
  - Conflicts with explicit `-F/--format` to avoid ambiguity
- Added `-f, --filter` global flag for unified filter expressions
  - Accepts syntax like `"c:core l:ERROR !t:timeout"`
  - Replaces per-command filter flags (`--component`, `--level`, `--contains`, etc.)
  - Can be set via `LOG_ANALYZER_FILTER` environment variable
- Added `effective_format()` and `effective_compact()` helper methods to `Cli` struct
