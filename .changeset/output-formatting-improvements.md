---
default: patch
---

Improved output formatting with table support:

- Added `comfy-table` dependency for better table rendering with dynamic width and styling
- Added `thiserror` dependency for improved error handling
- Extended `OutputFormatter` trait with `write_table()` method for table output
- Added `create_styled_table()` helper function for consistent table styling
- Implemented table support in both `ConsoleFormatter` and `FileFormatter`
- Refactored `console_summary.rs` to use styled tables for statistics display
