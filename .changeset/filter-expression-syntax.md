---
default: minor
---

Add unified filter expression syntax:

- Added expression-based filtering via `-f, --filter`.
- Supports `component`, `level`, `text`, and `direction` terms (with short aliases like `c:`, `l:`, `t:`, `d:`).
- Supports exclusions with `!` (for example: `!l:DEBUG`).
- Multiple terms are combined with AND semantics.
- Matching for `level` and `direction` values is case-insensitive.
- Unknown filter values now produce warnings to catch typos.
