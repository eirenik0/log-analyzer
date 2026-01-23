---
default: minor
---

Add unified filter expression syntax for log filtering:

- New `src/filter/` module providing a simple expression language for filtering logs
- Filter types supported:
  - `component:` / `comp:` / `c:` - Filter by component name
  - `level:` / `lvl:` / `l:` - Filter by log level
  - `text:` / `t:` - Filter by text in message
  - `direction:` / `dir:` / `d:` - Filter by direction (incoming/outgoing)
- Exclusion filters with `!` prefix (e.g., `!level:DEBUG`)
- Multiple terms combined with AND logic
- Case-insensitive matching for level and direction values
- Warnings for unknown filter values to help catch typos

Example usage:
```
# Filter by component and level
-f "component:core level:ERROR"

# Exclude debug logs from socket component
-f "comp:socket !lvl:DEBUG"

# Filter incoming requests containing timeout
-f "direction:incoming text:timeout"

# Using short aliases
-f "c:core l:ERROR !t:retry"
```
