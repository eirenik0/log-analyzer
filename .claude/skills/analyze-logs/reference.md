# Log Analyzer Command Reference

Complete documentation of all log-analyzer commands and options.

## Installation

### Using Installation Script (Recommended)

The easiest way to install log-analyzer is using the installation script, which auto-detects your platform and downloads the appropriate binary:

```bash
curl -fsSL https://raw.githubusercontent.com/eirenik0/log-analyzer/main/scripts/install.sh | bash
```

This will:
- Detect your OS and architecture automatically
- Download the latest release binary
- Install to `~/bin/log-analyzer` by default
- Provide instructions for adding to PATH if needed

If you are inside a cloned `log-analyzer` repository, `./scripts/install.sh` works too.

Verify installation:

```bash
log-analyzer --version
log-analyzer --help
```

### Manual Download from Release Binary

Download the appropriate binary for your platform from [GitHub Releases](https://github.com/eirenik0/log-analyzer/releases):

```bash
# macOS (Apple Silicon)
curl -LO https://github.com/eirenik0/log-analyzer/releases/latest/download/log-analyzer-VERSION-aarch64-apple-darwin.tar.gz
tar xzf log-analyzer-*.tar.gz
sudo mv log-analyzer /usr/local/bin/

# macOS (Intel)
curl -LO https://github.com/eirenik0/log-analyzer/releases/latest/download/log-analyzer-VERSION-x86_64-apple-darwin.tar.gz

# Linux (x86_64)
curl -LO https://github.com/eirenik0/log-analyzer/releases/latest/download/log-analyzer-VERSION-x86_64-unknown-linux-gnu.tar.gz

# Linux (musl/Alpine)
curl -LO https://github.com/eirenik0/log-analyzer/releases/latest/download/log-analyzer-VERSION-x86_64-unknown-linux-musl.tar.gz
```

### From Source

```bash
cargo install --path .
# or
cargo build --release
```

## Global Options

These work with any command:

Path/glob note:
- Multi-file commands rely on shell-expanded globs (for example `./logs/*.log`)
- If a path contains spaces, quote the directory part but keep the wildcard outside quotes (for example `"/path with spaces"/logs/*.log`)

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `-F, --format` | `text`, `json` | `text` | Output format |
| `-j, --json` | flag | off | JSON output (shorthand for `-F json -c`) |
| `-c, --compact` | flag | off | Use compact mode (shorter keys) |
| `-f, --filter` | expression | none | Filter expression (see below) |
| `-o, --output` | path | stdout | Save results to file |
| `--config` | path | none | Load parser/perf/profile rules from TOML |
| `--preset` | name | none | Use a built-in preset/profile (`base`, `eyes`, `custom-start`, `service-api`, `event-pipeline`) |
| `--color` | `auto`, `always`, `never` | `auto` | Control color output |
| `-v, --verbose` | count | 0 | Increase verbosity (repeatable) |
| `-q, --quiet` | flag | off | Show only errors |

## Profile Templates

Start from a template and customize your log format:

```bash
# Local repo templates
cp config/templates/custom-start.toml config/profiles/my-team.toml

# Skill-installed templates (global install)
cp ~/.claude/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml
```

Available files:
- `config/profiles/base.toml` - generic parser/perf/profile baseline
- `config/profiles/eyes.toml` - Eyes/Applitools-specific preset
- `config/templates/custom-start.toml` - generic starter with placeholders
- `config/templates/service-api.toml` - service/API oriented wording
- `config/templates/event-pipeline.toml` - event-driven pipeline wording

Built-in template names (for `generate-config --template`):
- `base`
- `eyes`
- `custom-start`
- `service-api`
- `event-pipeline`

Use a profile with any command:

```bash
log-analyzer --preset eyes info ./logs/test.log
log-analyzer --config config/profiles/my-team.toml info ./logs/test.log
log-analyzer --config config/profiles/my-team.toml diff ./logs/a.log ./logs/b.log
```

Optional session lifecycle hints can be defined with `[[sessions.levels]]` in the profile (for example `runner`/`test` levels with `segment_prefix`, `create_command`, and `complete_commands`).
`generate-config` now auto-detects session-like prefixes from `component_id` paths and emits generic `[[sessions.levels]]` entries (`level-1`, `level-2`, ...);

## Filter Expression Syntax

Use `-f, --filter` with unified expression syntax:

```bash
-f "type:value [!type:value] ..."
```

**Filter types (with aliases):**
| Type | Aliases | Description |
|------|---------|-------------|
| `component` | `comp`, `c` | Filter by component name |
| `level` | `lvl`, `l` | Filter by log level (INFO, ERROR, etc.) |
| `text` | `t` | Filter by text in message |
| `direction` | `dir`, `d` | Filter by direction (incoming/outgoing) |

**Prefix with `!` to exclude.**
Different filter types combine with AND, while multiple values of the same type combine with OR.

```bash
-f "c:core-universal"           # Only core-universal component
-f "l:ERROR"                    # Only ERROR level logs
-f "c:core !l:DEBUG"            # Core component, exclude DEBUG
-f "t:timeout d:incoming"       # Contains 'timeout', incoming only
```

## Commands

### compare (alias: cmp)

Compare two log files and show differences.

```bash
log-analyzer compare <file1> <file2> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `-D, --diff-only` | Show only differences |
| `--full` | Show full JSON objects |
| `-s, --sort-by <field>` | Sort by: time, component, level, type, diff-count |

**Examples:**
```bash
# Basic comparison
log-analyzer compare test1.log test2.log

# Show only differences
log-analyzer compare test1.log test2.log --diff-only

# Filter to errors in core component
log-analyzer compare test1.log test2.log -f "c:core-universal l:ERROR"

# JSON output sorted by difference count
log-analyzer compare test1.log test2.log -D -j -s diff-count
```

### diff

Shortcut for `compare --diff-only`.

```bash
log-analyzer diff <file1> <file2> [options]
```

Same options as `compare` except `--diff-only` is implicit.

### info (aliases: i, inspect)

Display information about one or more log files.

```bash
log-analyzer info <file> [file...] [options]
```

When multiple files are provided, entries are merged and analyzed together.
Only combine related files (for example, split logs from the same run/session), otherwise aggregated counts/timelines may not be meaningful.

**Options:**
| Option | Description |
|--------|-------------|
| `-s, --samples` | Show sample log messages |
| `--json-schema` | Display JSON schema information |
| `-p, --payloads` | Show payload statistics |
| `-t, --timeline` | Show timeline analysis |

**Examples:**
```bash
# Full analysis across multiple files
log-analyzer info ./logs/*.log --samples --payloads --timeline

# JSON schema for a specific component
log-analyzer info ./logs/*.log -f "c:socket" --json-schema

# Quick overview
log-analyzer info test.log
```

If the loaded profile defines `[[sessions.levels]]`, `info` also prints a per-level session completion summary (completed vs incomplete) and common configured create-time summary fields.

### search

Structured grep-style search for matching log entries in a single file, using the same `-f/--filter` expression syntax as other commands.

```bash
log-analyzer search <file> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--context <n>` | Show `n` entries before/after each match |
| `--payloads` | Show parsed payload/settings JSON for displayed entries |
| `--count-by <field>` | Count/group matches by: matches, component, level, type, payload |

`--count-by` switches output from entry listing to grouped counts.

**Examples:**
```bash
# Structured search with log-aware filtering
log-analyzer search test.log -f "t:makeManager c:core" --payloads

# Show matching entries with 2 entries of context
log-analyzer search test.log -f "t:retryTimeout" --context 2

# Count/group matches by parsed payload JSON
log-analyzer search test.log -f "t:concurrency" --count-by payload
```

### errors

Diagnose recurring failures across one or more related logs by clustering normalized ERROR messages (and optionally WARNs), listing affected `component_id` sessions, and estimating impact using orphan-operation detection.

```bash
log-analyzer errors <file> [file...] [options]
```

When multiple files are provided, entries are merged and analyzed together.
Only combine related files from the same run/session, otherwise cluster counts and session outcomes may be misleading.

**Options:**
| Option | Description |
|--------|-------------|
| `--top-n <n>` | Number of clusters to show (default: 10, `0` = all) |
| `--warn` | Include WARN entries (default: ERROR only) |
| `--sessions` | Show affected sessions per cluster |
| `-s, --sort-by <field>` | Sort by: `count` (default), `time`, `impact` |

**Examples:**
```bash
# Quick "what went wrong?" summary across split logs
log-analyzer errors ./logs/*.log

# Include warnings and show impacted sessions with outcome labels
log-analyzer errors ./logs/*.log --warn --sessions

# Prioritize clusters affecting the most sessions
log-analyzer errors ./logs/*.log --warn --sort-by impact --top-n 20
```

For bug triage, use `errors` as an early first pass, then follow with targeted `search`/`extract`/`trace` queries (for example manager creation patterns, concurrency config values, and SDK request/session traces) to build the full causal chain.

### extract

Repeat `--field` to keep related values in rows, or select `--rows`. For explicit
array expansion and missing/null semantics, see the README extract section.
Rows retain timestamps, correlation IDs, and source file/line/row paths.

Extract and aggregate a specific field from parsed payload/settings JSON in matching entries.

```bash
log-analyzer extract <file> --field <path> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--field <path>` | Field name/path to extract (supports dot paths like `settings.retryTimeout`) |

Uses the same global `-f/--filter` expression syntax to scope which entries contribute to the aggregation.

**Examples:**
```bash
# Extract concurrency values from makeManager calls
log-analyzer extract test.log -f "t:makeManager" --field concurrency

# Extract retryTimeout values
log-analyzer extract test.log -f "t:retryTimeout" --field retryTimeout

# Nested field path
log-analyzer extract test.log -f "c:core" --field settings.retries.0.timeout
```

### process (alias: llm)

Generate LLM-friendly compact JSON output.

```bash
log-analyzer process <file> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `-s, --sort-by <field>` | Sort by: time (earliest first), component, level (highest severity first), type |
| `--limit <n>` | Max entries (default: 100, 0 = unlimited) |
| `--no-sanitize` | Disable sensitive field redaction |

Filtering and sorting precede the limit. Timestamp breaks ties; exact ties keep input order.
The time range describes the returned entries. `diff-count` is only valid for diffs.
Entry `ts` and time-range bounds are full RFC 3339 UTC timestamps with milliseconds
and `Z`; they preserve the instant across host timezones, DST, and midnight.

**Output format:**
```json
{
  "metadata": {
    "total_entries": 500,
    "filtered_entries": 100,
    "components": ["socket", "core-universal"],
    "levels": ["INFO", "ERROR"],
    "time_range": { "start": "...", "end": "..." }
  },
  "logs": [
    {
      "idx": 1,
      "ts": "2026-01-01T21:07:27.621Z",
      "comp": "core-universal",
      "lvl": "INFO",
      "typ": "E:Emit:Logger.log",
      "msg": "...",
      "data": {}
    }
  ]
}
```

**Examples:**
```bash
# Standard LLM preparation
log-analyzer llm test.log --limit 100 -o context.json

# Include sensitive data (for debugging only)
log-analyzer llm test.log --no-sanitize

# Errors only
log-analyzer llm test.log -f "l:ERROR" --limit 50
```

### llm-diff

Generate LLM-friendly diff output. Equivalent to `compare --diff-only -F json -c`.

```bash
log-analyzer llm-diff <file1> <file2> [options]
```

Same options as `process` (or `llm`) command.

### perf

Analyze operation timing and identify bottlenecks across one or more log files.

```bash
log-analyzer perf <file> [file...] [options]
```

When multiple files are provided, entries are merged and sorted by timestamp before analysis. This allows cross-file pairing, including orphan detection when operations start in one file and complete in another.
Only combine related files from the same run/session. Mixing unrelated logs can produce misleading or meaningless timing/orphan results.

**Options:**
| Option | Description |
|--------|-------------|
| `--threshold-ms <ms>` | Duration threshold (default: 1000ms) |
| `--top-n <n>` | Maximum rows per section (default: 20, `0` = unlimited) |
| `--orphans-only` | Show only orphan operations |
| `--op-type <type>` | Filter: `request`, `event`, `command` |
| `-s, --sort-by <field>` | Sort by: duration, count, name |

Text and JSON apply the same selection before output. `--top-n` limits each
operations, statistics, orphans, and threshold-violations section separately;
`0` includes all rows. Duration sorts operations by elapsed time and statistics
by average duration, descending. Count sorts by the full operation-group count;
name sorts alphabetically. Orphans are chronological. Full `totals` and `omitted`
counts describe the analysis before selection, including with `--orphans-only`.
The threshold highlights slow completed operations; it does not filter statistics.
Explicit `--sort-by` or `--threshold-ms` conflicts with `--orphans-only`, since
unfinished operations have no measured duration or completed-operation count.

**Output includes:**
- Slowest operations with timing details
- Orphan operations (started but never finished)
- Statistics per operation type (count, avg, p50, p95, p99)

**Examples:**
```bash
# Find slow operations (>2s) across split session logs
log-analyzer perf ./logs/*.log --threshold-ms 2000

# Find incomplete operations (cross-file pairing supported)
log-analyzer perf ./logs/*.log --orphans-only

# Request analysis only
log-analyzer perf ./logs/*.log --op-type request --top-n 50

# Sort by occurrence count
log-analyzer perf ./logs/*.log -s count
```

Performance correlation uses operation kind, name, ID, and `component_id` scope.
Configure `[perf].correlation_scope_fields` for a composite scope (for example
`["component_id", "tenant"]`); `component`, structured fields, and top-level payload
fields are supported. A missing, null, or empty configured scope field is
reported instead of matched. Logs without session IDs need an explicit known
scope (for example `["component"]`) or an intentionally empty scope list; an empty
list makes IDs global and overlapping starts remain ambiguous. JSON envelope
`payload`/`fields` are retained separately from embedded message payloads and
consulted for scope lookup.
Related files can pair across file boundaries because filenames are provenance,
not correlation scope. Overlapping starts for the same composite key are ambiguous:
all group events are preserved, starts remain orphans, and no duration is invented.
Unmatched ends and missing keys are retained in `unmatched_events`; JSON includes
`ambiguous_groups` and start/end source file and line references. Diagnostic
sections also obey `--top-n`, with full totals and omitted counts; `0` preserves all.

Substring `trace` searches can span multiple lifecycles and do not establish pairing.

### Configurable event timelines

See [README: Configurable event timelines](../../../README.md#configurable-event-timelines)
for the profile syntax. For retries, identify the response boundary separately
from the marker written after sleep. Inspect incomplete/ambiguous groups before
using durations; use `trace` to narrow the evidence to a related context.

### trace

Trace a single operation lifecycle by correlation/request ID or by `component_id` session path across one or more log files.

```bash
log-analyzer trace <file> [file...] (--id <substring> | --session <substring>) [options]
```

When multiple files are provided, entries are merged and sorted by timestamp before tracing.
Only combine related files from the same run/session, otherwise the timeline may include unrelated noise.

**Options:**
| Option | Description |
|--------|-------------|
| `--id <substring>` | Match correlation/request ID substring in raw log lines |
| `--session <substring>` | Match `component_id` hierarchy/session path substring |

Uses the global output options (`-F`, `-j`, `-o`) and prints per-step timing deltas in text mode.

**Examples:**
```bash
# Trace by correlation/request ID fragment
log-analyzer trace ./logs/*.log --id f227f11e

# Trace by session path / component_id hierarchy
log-analyzer trace ./logs/*.log --session manager-ufg-3nl

# JSON trace output
log-analyzer -j trace ./logs/*.log --id f227f11e -o trace.json
```

### generate-config (alias: gen-config)

Analyze one or more related log files and generate a TOML config profile.

```bash
log-analyzer generate-config <file> [file...] [options]
```

When multiple files are provided, entries are merged before profile inference. Only combine related logs from the same run/session.

**Options:**
| Option | Description |
|--------|-------------|
| `--profile-name <name>` | Name to set in `profile_name` (defaults to input filename stem for one file, otherwise `generated-profile`) |
| `--template <path\|name>` | Base profile to inherit parser/perf rules from (`base`, `eyes`, `custom-start`, `service-api`, `event-pipeline`) |

Output is always TOML. Use global `-o, --output` to write generated profile files.

**Examples:**
```bash
# Print generated profile TOML to stdout
log-analyzer generate-config ./logs/test.log --profile-name test-run

# Merge split logs before inferring profile hints
log-analyzer generate-config ./logs/run-1.log ./logs/run-2.log --profile-name run-profile

# Save generated profile for skill reuse
log-analyzer generate-config ./logs/*.log --profile-name cypress \
  -o .claude/skills/analyze-logs/profiles/cypress.toml

# Generate using parser/perf rules from a built-in template
log-analyzer generate-config ./logs/*.log \
  --template service-api \
  --profile-name service-api \
  -o .claude/skills/analyze-logs/profiles/service-api.toml

# Generate a repo profile starting from the Eyes preset
log-analyzer generate-config ./logs/*.log \
  --template eyes \
  --profile-name eyes \
  -o .claude/skills/analyze-logs/profiles/eyes.toml
```

## Environment Variables

All variables use `LOG_ANALYZER_` prefix:

| Variable | Description |
|----------|-------------|
| `LOG_ANALYZER_FORMAT` | Default output format (`text` or `json`) |
| `LOG_ANALYZER_JSON` | Enable JSON output mode |
| `LOG_ANALYZER_COMPACT` | Enable compact mode |
| `LOG_ANALYZER_FILTER` | Default filter expression |
| `LOG_ANALYZER_OUTPUT` | Default output file |
| `LOG_ANALYZER_CONFIG` | Default profile/config file |
| `LOG_ANALYZER_PRESET` | Default built-in preset/profile |
| `LOG_ANALYZER_SORT_BY` | Default sort order |

## Log Format

The classic parser expects logs in this format:
```
component | timestamp [LEVEL] message
```

Example:
```
socket | 2025-04-03T21:07:27.668Z [INFO ] Emit event of type "Logger.log" with payload {...}
core-universal | 2025-04-03T21:07:27.652Z [INFO ] Core universal is started on port 21077
```

Supports multi-line JSON payloads embedded in messages.

The parser can also auto-detect:

- Rust tracing: `timestamp level module::path: message key=value ...`
- Syslog/journald-style lines: `timestamp host process[pid]: message`
- JSON lines: `{"timestamp":...,"level":...,"message":...}`

Profiles can force the format with `[parser] format = "rust-tracing"` (or `classic`, `syslog`, `json-lines`). Rust tracing fields are available to `search` filters and `extract --field ...`, for example `-f "trace_id:abc123"` or `-f "actor_kind:switch"`.

## Parse Coverage

Check `info`, `errors`, or `perf` coverage before interpreting the result. Text
and JSON expose each file's parser/profile, byte size, parsed entry count, and
rejected candidates. Coverage counts precede filters and ERROR/WARN selection.
A nonempty file with no recognized entries exits 1 and reports `unparsed_input`,
even in a mixed file set. Empty/whitespace input (`empty_input`), zero filter
matches (`zero_filter_matches`), and parsed error-free input (`parsed`) succeed.
Multiline continuation lines do not count as rejected candidates.
Use `info -F json` for coverage plus component/level totals. On parsing failure,
JSON stdout and `-o` still contain coverage; inspect stderr for the diagnostic.

## Browser Console Logs

Classic entries support a leading `filename:line`, `path:line:column`, or
`URL:line:column` source location followed by whitespace and a classic header.
No input rewriting is needed. Physical source line numbers and `raw_logline`
remain original; `console_source` is exposed as a structured field. Prefixed
continuation lines stay attached to the entry and are normalized for payload
parsing. Use `search --payloads` or `extract --field console_source` to inspect
source locations.

## Bounded Error Reports

Use `errors --bounded --sessions` for a compact inventory instead of piping to
`head`. Defaults: 600 Unicode characters per sample/pattern, 5 stack-frame lines,
and 12000 Unicode characters for total text output, including newlines/coverage.
Override with `--max-sample-chars`, `--max-stack-frames`, `--max-output-chars`;
any limit enables bounded mode. Zero suppresses that detail or requests a zero
budget. Scope, totals, impact, and cluster overviews appear before samples.
Omission counts cover hidden clusters, sample characters, stack-frame lines,
pattern characters, and requested session detail rows. Metadata is always kept;
if it exceeds the budget, print it with a warning and omit cluster details.
Text overviews limit patterns to 120 characters or the smaller sample limit.
JSON keeps full coverage/totals and bounded details with omission counts; the
total output character budget applies only to text. Complete output is the
default; `--complete --top-n 0` explicitly requests every cluster without detail
limits and conflicts with bounded/limit flags.

### Structured exports

Preview unknown JSON exports with `schema <file> --samples 3` before mapping fields.
Use explicit paths and timestamp units from [README](../../../README.md#structured-export-normalization),
then check normalization diagnostics in `info -j` coverage before investigating.

For reports intended for sharing, use global `--redact`; selected ID fields can
use stable invocation-local masks via `--mask-id`. See README report redaction for
the shared text/JSON policy and raw-output behavior. Redaction does not change
filtering or correlation.

Use `log-analyzer capabilities` to inspect the installed build and supported commands. Reports carry build/profile metadata; see README “Build identity and capabilities” for schema and count-output behavior.

Compact text preserves Unicode graphemes and disambiguates shortened field names. See README “Safe compact text” for omission-marker collisions and existing limits.

Check `perf` operation coverage separately from parse coverage before interpreting empty results. README “perf” describes evidence statuses, suppressions, ambiguous/rejected events, and capture limits.

Shipped command profiles recognize quoted start and completion names independently. For command pairing, tune `[perf]` lifecycle markers; command names, quoted context, assignment metadata, and payload regions (including malformed JSON) are excluded from boundary matching. Negated lifecycle wording does not create boundaries. Embedded JSON must have balanced delimiters and at most 128 nesting levels; deeper payloads stay opaque. See README “Configuration is Essential” for custom unquoted-name compatibility.
