---
default: minor
---

Enhanced diff command output with change type classification, statistics, and line number tracking:

- Added `ChangeType` enum (`Added`, `Removed`, `Modified`) to classify JSON differences
- Added change type indicators in diff output (`+` for additions, `-` for removals, `~` for modifications)
- Implemented change type detection in JSON comparison logic to distinguish between added, removed, and modified fields
- Added change statistics to summary output showing counts of additions, removals, and modifications
- Introduced source line number tracking throughout the parser to enable quick navigation to original log entries
- Updated diff display to show line numbers for each log entry (e.g., `FILE1 #0 (line 410)`)
- Enhanced JSON output formats (standard, compact, readable) to include `change_type` field
- Changed `text_difference` to separate `text1`/`text2` fields for clearer diff representation
- Updated all LogEntry creation paths to track source line numbers from the parsing stage

Example output:
```
File 1: 91 entries, File 2: 100 entries
Changes: 47 additions, 89 removals, 105 modifications

1/3. FILE1 #0 (line 410) ↔ FILE2 #0 (line 540)
  JSON DIFFERENCES:
    [D:1] [+] properties :
      null
      ➔
      [{"name":"browserVersion","value":"135.0"}]
    [D:2] [-] connectionTimeout :
      300000
      ➔
      null
    [D:3] [~] batch.id :
      "b2d1b661-00..."
      ➔
      "6e8afcf5-bc..."
```
