# Log Analyzer

[![CI](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml)
[![Release](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml)

A CLI tool for analyzing and comparing structured logs.

The default `base` profile is intentionally generic. Use a built-in preset such as `--preset eyes` or a repo-specific `--config` file when you need log-family-specific parsing and lifecycle semantics.

## Supported Log Format (Quick Check)

`log-analyzer` works best with structured text logs where each entry looks like:

```text
component | timestamp [LEVEL] message
```

Example:

```text
socket | 2025-04-03T21:07:27.668Z [INFO ] Emit event of type "Logger.log" with payload {...}
core-universal | 2025-04-03T21:07:27.652Z [INFO ] Core universal is started on port 21077
```

It also auto-detects a few other common formats:

- Rust tracing: `timestamp level module::path: message key=value ...`
- Syslog/journald-style lines: `timestamp host process[pid]: message`
- JSON lines: `{"timestamp":...,"level":...,"message":...}`

Profiles can force a parser with `[parser] format = "rust-tracing"` (or `classic`, `syslog`, `json-lines`) and tune Rust target mapping with `module_depth` / `module_strip_prefix`. Structured `key=value` fields become filterable and extractable via `--filter "trace_id:abc123"` or `extract --field restream_name`.

## Installation

```bash
# Auto-detect platform and install latest release
curl -fsSL https://raw.githubusercontent.com/eirenik0/log-analyzer/main/scripts/install.sh | bash

# Or build from source
cargo install --path .

# Verify installation
log-analyzer --version
```

## Quick Start

```bash
# Compare two log files
log-analyzer compare file1.log file2.log

# Show only differences
log-analyzer diff file1.log file2.log

# Get log overview (single file or multiple files)
log-analyzer info logs/*.log

# Opt into the built-in Eyes/Applitools preset when analyzing that log family
log-analyzer --preset eyes info logs/*.log

# Structured grep-style search with log-aware filtering
log-analyzer search file.log -f "t:retryTimeout" --context 2

# Search Rust tracing fields directly
log-analyzer search file.log -f "actor_kind:switch" --payloads

# Extract a payload field and aggregate occurrences
log-analyzer extract file.log -f "t:makeManager" --field concurrency

# Extract a structured tracing field
log-analyzer extract file.log -f "trace_id:fabb5aa4" --field restream_name

# Diagnose clustered errors and affected sessions across related logs
log-analyzer --preset eyes errors logs/*.log --warn --sessions

# Analyze performance bottlenecks across one or more files
log-analyzer --preset eyes perf logs/*.log

# Trace one operation lifecycle by correlation/request ID or session path
log-analyzer trace logs/*.log --id f227f11e

# Generate LLM-friendly output
log-analyzer llm file.log

# Generate a starter profile from one or more related logs
log-analyzer generate-config logs/*.log --template custom-start --profile-name my-team

# Generate a profile starting from the Eyes preset
log-analyzer generate-config logs/*.log --template eyes --profile-name my-eyes-team
```

## Configuration is Essential

> **Every analysis command depends on a well-tuned profile config.** Without one, the tool falls back to generic heuristics that will miss domain-specific commands, requests, session hierarchies, and lifecycle boundaries. The difference between a useful diagnosis and a misleading one is almost always the config.

**Why this matters:**

- **Session completion tracking** (`info` profile insights, `errors --sessions`) requires `[[sessions.levels]]` to know which `component_id` prefixes map to runners, tests, checks, and environments - and which commands create or complete them. Without this, incomplete/orphaned sessions go undetected.
- **Performance pairing** (`perf`) uses `command_start_markers` and `command_completion_markers` from `[perf]` to match operation starts with their completions. Wrong markers = wrong latencies and false orphans.
- **Payload extraction** (`extract`, `search --payloads`) relies on `json_indicators` and `command_payload_markers` from `[parser]` to locate and parse embedded JSON. If these don't match your log format, payloads are invisible.
- **Request lifecycle tracing** (`trace`, `perf --orphans-only`) depends on `request_send_markers`, `request_receive_markers`, and `request_endpoint_marker` to pair outgoing requests with their responses.

**How to get started:**

```bash
# 1. Start from the right built-in preset/template for your log family
#    Eyes / Applitools-style logs:
log-analyzer generate-config logs/*.log --template eyes --profile-name my-team

#    Other structured logs:
log-analyzer generate-config logs/*.log --template custom-start --profile-name my-team

# 2. Review and refine the generated TOML - add session levels, fix markers
#    The generator infers what it can, but domain knowledge is yours to add

# 3. Always pass --config when running analysis
log-analyzer --config my-team.toml errors logs/*.log --sessions
```

If your log directory path contains spaces, quote the directory part but not the wildcard (for example `"/path with spaces"/logs/*.log`).

See [Profile Configuration](#profile-configuration) for the full reference and examples. Investing 10 minutes in a good config pays back on every analysis run.

## 5-Minute First Success

Use this sequence to confirm the parser works on your logs before deeper analysis:

```bash
# 1. Sanity-check that entries parse and timestamps/components look right
#    Use a preset if your logs already match one of the built-ins
log-analyzer --preset eyes info logs/*.log

# 2. Generate a starter profile from the same related log set
log-analyzer generate-config logs/*.log --template eyes --profile-name my-team

# 3. Re-run with the generated profile and inspect payload extraction
log-analyzer --config my-team.toml info logs/*.log --payloads --samples

# 4. Pick the next command by goal
#    Failure triage:
log-analyzer --config my-team.toml errors logs/*.log --warn --sessions

#    Performance triage:
log-analyzer --config my-team.toml perf logs/*.log --threshold-ms 1000

#    One request/session trace:
log-analyzer --config my-team.toml trace logs/*.log --id <id-fragment>
```

If step 3 shows missing payloads or obviously wrong command/request names, tune the profile markers before trusting `errors`, `perf`, or `trace`.

## What Counts as "Related Logs"?

Use multiple files together only when they belong to the same run/session (for example rotated/split chunks from one test run or one service execution window).

Good signs they are related:

- overlapping or contiguous timestamps for one investigation window
- same environment/test run/build context
- shared `component_id` hierarchy or correlation/request IDs
- files were split/rotated from one process/run

Avoid mixing unrelated runs, retries from different executions, or logs from different environments in the same command. That can distort counts, traces, session impact, and latency/orphan analysis.

## Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `compare` | `cmp` | Compare two log files |
| `diff` | | Compare showing only differences |
| `info` | `i`, `inspect` | Display statistics for one or more log files |
| `search` | | Structured grep-style search for matching log entries |
| `errors` | | Cluster ERROR/WARN patterns and summarize affected sessions |
| `extract` | | Extract and aggregate a JSON payload/settings field from matching entries |
| `perf` | | Analyze operation timing across one or more log files |
| `trace` | | Trace one operation/session across one or more log files |
| `process` | `llm` | Generate LLM-friendly JSON output |
| `llm-diff` | | Generate LLM-friendly diff output |
| `generate-config` | `gen-config` | Generate a profile TOML from logs |

## Global Options

| Option | Env Variable | Description |
|--------|--------------|-------------|
| `-F, --format <text\|json>` | `LOG_ANALYZER_FORMAT` | Output format |
| `-j, --json` | `LOG_ANALYZER_JSON` | JSON output (shorthand for `-F json -c`) |
| `-c, --compact` | `LOG_ANALYZER_COMPACT` | Compact output mode |
| `-f, --filter <expr>` | `LOG_ANALYZER_FILTER` | Filter expression (see below) |
| `-o, --output <path>` | `LOG_ANALYZER_OUTPUT` | Output file path |
| `--config <path>` | `LOG_ANALYZER_CONFIG` | Load parser/perf/profile rules from TOML |
| `--color <auto\|always\|never>` | `LOG_ANALYZER_COLOR` | Color output control |
| `-v, --verbose` | `LOG_ANALYZER_VERBOSE` | Increase verbosity |
| `-q, --quiet` | `LOG_ANALYZER_QUIET` | Show only errors |

## Filter Expression Syntax

Use `-f, --filter` with a unified expression syntax:

```bash
--filter "type:value [!type:value] ..."
```

**Filter types (with aliases):**

| Type | Aliases | Description |
|------|---------|-------------|
| `component` | `comp`, `c` | Filter by component name |
| `level` | `lvl`, `l` | Filter by log level (INFO, ERROR, etc.) |
| `text` | `t` | Filter by text in message |
| `direction` | `dir`, `d` | Filter by direction (incoming/outgoing) |

**Prefix with `!` to exclude.**  
Different filter types are combined with AND. Multiple values of the same type are OR-ed.

```bash
# Only core-universal component
-f "c:core-universal"

# Only ERROR level logs
-f "l:ERROR"

# Core component, exclude DEBUG level
-f "c:core !l:DEBUG"

# Contains 'timeout', incoming direction only
-f "t:timeout d:incoming"
```

## Command-Specific Options

### compare / diff

| Option | Description |
|--------|-------------|
| `-D, --diff-only` | Show only differences (always on for `diff`) |
| `--full` | Show full JSON objects |
| `-s, --sort-by` | Sort by: `time`, `component`, `level`, `type`, `diff-count` |

### info

Accepts one or more log files. When multiple files are provided, entries are merged and analyzed together.
Use this only for related logs (for example, split files from the same run/session). Mixing unrelated runs can make counts and timelines misleading.

| Option | Description |
|--------|-------------|
| `-s, --samples` | Show sample messages per component |
| `--json-schema` | Display JSON schema information |
| `-p, --payloads` | Show payload statistics |
| `-t, --timeline` | Show timeline analysis |

### search

Searches one log file and prints matching entries using the same structured filter expression used by other commands.

| Option | Description |
|--------|-------------|
| `--context <n>` | Show `n` entries before/after each match |
| `--payloads` | Show parsed payload/settings JSON for displayed entries |
| `--count-by <field>` | Count/group matches by: `matches`, `component`, `level`, `type`, `payload` |

`--count-by` switches the command into count mode (grouped counts instead of entry output).

### errors

Diagnoses ERROR entries (and optionally WARN entries) across one or more related log files by clustering normalized message patterns and estimating session impact from `component_id` + orphan detection heuristics.

Accepts one or more log files. Entries are merged and sorted by timestamp before analysis.
Use this only for related logs from the same run/session. Combining unrelated logs can make affected-session counts and orphan outcomes misleading.

| Option | Description |
|--------|-------------|
| `--top-n <number>` | Number of clusters to show (default: 10, `0` = all) |
| `--warn` | Include WARN entries (default: ERROR only) |
| `--sessions` | Show affected sessions per cluster (cross-references `component_id`) |
| `-s, --sort-by <field>` | Sort by: `count` (default), `time`, `impact` |

### extract

Extracts a named field from parsed payload/settings JSON for matching log entries and aggregates counts by value.

| Option | Description |
|--------|-------------|
| `--field <path>` | Field name/path to extract (supports dot paths like `settings.retryTimeout`) |

### perf

Accepts one or more log files. Entries are merged and sorted by timestamp before analysis, which enables cross-file pairing (for example, orphan resolution when an operation starts in one file and completes in another).
Use this only for related logs from the same run/session. Combining unrelated logs can produce meaningless latency/orphan results.

| Option | Description |
|--------|-------------|
| `--threshold-ms <ms>` | Slow operation threshold (default: 1000) |
| `--top-n <number>` | Number of slowest operations (default: 20) |
| `--orphans-only` | Show only unfinished operations |
| `--op-type <request\|event\|command>` | Filter by operation type |

Sort options: `duration`, `count`, `name`

### trace

Accepts one or more log files. Entries are merged and sorted by timestamp, then filtered by one selector:
- `--id <substring>` matches correlation/request IDs by substring in the raw log line (useful for truncated IDs from grep output)
- `--session <substring>` matches the `component_id` hierarchy/path (for example `manager-ufg-3nl`)

This is intended for tracing a single run/session across split logs. Mixing unrelated files may produce noisy traces.

| Option | Description |
|--------|-------------|
| `--id <substring>` | Trace by correlation/request ID substring |
| `--session <substring>` | Trace by `component_id` / session path substring |

### llm / llm-diff

| Option | Description |
|--------|-------------|
| `-s, --sort-by` | Sort by: `time`, `component`, `level`, `type`, `diff-count` |
| `--no-sanitize` | Disable sensitive field hiding |

`llm` (`process`) also supports:
- `--limit <number>` - Max entries (default: 100, `0` = unlimited)

### generate-config

Generate a profile from one or more related log files (for example, split/rotated logs from the same run/session).

| Option | Description |
|--------|-------------|
| `--profile-name <name>` | Name for the generated profile (defaults to file stem for a single input, otherwise `generated-profile`) |
| `--template <path-or-name>` | Base template path or built-in: `base`, `eyes`, `custom-start`, `service-api`, `event-pipeline` |

## Examples

```bash
# Compare logs filtering by component and level
log-analyzer diff file1.log file2.log -f "c:core-universal l:ERROR"

# Exclude DEBUG logs from comparison
log-analyzer diff file1.log file2.log -f "!l:DEBUG"

# Save JSON diff to file
log-analyzer -j -o diff.json diff file1.log file2.log

# Show operations slower than 500ms across a session split into files (not unrelated runs)
log-analyzer --preset eyes perf logs/*.log --threshold-ms 500

# Trace one operation across split files using a request/correlation ID fragment
log-analyzer trace logs/*.log --id f227f11e

# Trace a whole session subtree by component_id path prefix/substring
log-analyzer trace logs/*.log --session manager-ufg-3nl

# Comprehensive analysis across multiple files from the same run/session
log-analyzer info logs/*.log --samples --timeline --payloads

# Structured search with payload display
log-analyzer search file.log -f "t:makeManager c:core" --payloads

# Search with context (entry-based, not raw text lines)
log-analyzer search file.log -f "t:retryTimeout" --context 2

# Group counts by parsed payload JSON
log-analyzer search file.log -f "t:concurrency" --count-by payload

# Cluster recurring failures and include per-session outcomes
log-analyzer --preset eyes errors logs/*.log --warn --sessions --sort-by impact

# Extract a specific payload field and aggregate values
log-analyzer extract file.log -f "t:makeManager" --field concurrency

# Extract retry timeout values from matching payloads
log-analyzer extract file.log -f "t:retryTimeout" --field retryTimeout

# LLM-friendly output with a custom limit
log-analyzer llm file.log --limit 50

# LLM-friendly diff sorted by highest-severity levels first
log-analyzer llm-diff file1.log file2.log --sort-by level

# Generate a profile from related split logs (merged before inference)
log-analyzer generate-config logs/run-*.log --template custom-start --profile-name my-team
```

## Environment Configuration

Set defaults via environment variables (prefix `LOG_ANALYZER_`):

```bash
export LOG_ANALYZER_FORMAT=json
export LOG_ANALYZER_FILTER="!l:DEBUG"
export LOG_ANALYZER_COMPACT=true
export LOG_ANALYZER_PRESET="eyes"
```

## Profile Configuration

Use profile TOML files to keep the binary generic and push case-specific knowledge into config.

Included built-ins:

- `config/profiles/base.toml` - minimal reusable defaults
- `config/profiles/eyes.toml` - Eyes/Applitools-specific preset
- `config/templates/custom-start.toml` - starter template for any project
- `config/templates/service-api.toml` - service/API wording template
- `config/templates/event-pipeline.toml` - event-driven wording template

These profiles/templates are also embedded in the binary and can be referenced by name in
`generate-config --template`:

- `base`
- `eyes`
- `custom-start`
- `service-api`
- `event-pipeline`

Examples:

```bash
# Generic base profile
log-analyzer info logs/app.log

# Built-in Eyes preset
log-analyzer --preset eyes info logs/app.log
```

Create your own profile from templates:

```bash
# In this repository
cp config/templates/custom-start.toml config/profiles/my-team.toml

# If only the skill is installed globally
cp ~/.claude/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml

# Then run with your custom profile
log-analyzer --config config/profiles/my-team.toml info logs/app.log

# Or generate a profile using an embedded built-in template
log-analyzer generate-config logs/app.log --template service-api --profile-name my-team

# Generate a profile from multiple related log chunks (merged before inference)
log-analyzer generate-config logs/run-1.log logs/run-2.log --template custom-start --profile-name my-team
```

Only combine related logs from the same run/session when using `generate-config`; mixing unrelated runs can pollute inferred commands/requests/session levels.

For consumer repositories, prefer a tiny wrapper script or Make target that pins either `--preset <name>` or `--config <repo-profile.toml>`. That keeps the binary generic while making repo workflows explicit and repeatable.

### Validate Your Profile (Quick Checklist)

Before relying on analysis results, verify the generated/custom profile with a few quick checks:

- `info --payloads --samples` shows expected command/request names and parsed payloads (not mostly missing payloads)
- `search --payloads` for a known message displays JSON payload/settings content you expect
- `extract --field <path>` returns real values for a known field (not only empty/missing)
- `perf` does not show obviously impossible orphan counts/latencies for known-complete runs
- `trace --id` or `trace --session` finds a known request/session path from your raw logs
- `errors --sessions` reports sensible affected sessions after `[[sessions.levels]]` is tuned

If these checks fail, adjust `[parser]`, `[perf]`, or `[[sessions.levels]]` markers before interpreting the output.

Optional session hierarchy/lifecycle hints (used by `info` profile insights):

```toml
[[sessions.levels]]
name = "runner"
segment_prefix = "manager-"
create_command = "makeManager"
complete_commands = ["getResults", "closeBatch"]
summary_fields = ["concurrency", "batch.id"]

[[sessions.levels]]
name = "test"
segment_prefix = "eyes-"
create_command = "openEyes"
complete_commands = ["close", "abort"]

[[sessions.levels]]
name = "environment"
segment_prefix = "environment-"
```

When `sessions.levels` is configured, `info` automatically summarizes session counts/completion health per level and can surface common create-time fields (for example `concurrency`).

`generate-config` also detects session-like prefixes from `component_id` paths and embeds them as generic `[[sessions.levels]]` entries (`level-1`, `level-2`, ...).

## Claude Code Integration

### Installation

Install the Claude Code skill to use interactive log analysis in any project:

```bash
/plugin marketplace add https://github.com/eirenik0/log-analyzer
/plugin install log-analyzer
```

### Usage

Use the `/analyze-logs` command in [Claude Code](https://claude.ai/code) for interactive analysis:

```bash
/analyze-logs diff file1.log file2.log          # Compare and explain differences
/analyze-logs perf logs/*.log --threshold-ms 500  # Find bottlenecks across files
/analyze-logs trace logs/*.log --id f227f11e      # Follow one operation lifecycle
/analyze-logs info logs/*.log --samples           # Cross-file log structure overview
/analyze-logs llm test.log                      # Generate LLM-friendly output
```

## Features

- **Structured parsing** - Extracts and parses JSON payloads automatically
- **Semantic comparison** - Compares JSON objects regardless of property order
- **Diff context improvements** - Tracks source line numbers and marks changes as added/removed/modified
- **Advanced filtering** - Include/exclude by component, level, content, or direction
- **Operation lifecycle tracing** - Follow a single correlation ID or session path across files with per-step timing
- **Multi-file session analysis** - Merge and analyze `info`/`perf` inputs across multiple log files
- **Session lifecycle insights** - Profile-driven session tree/completion tracking in `info` (with legacy prefix compatibility)
- **Performance analysis** - Identify slow and orphan operations
- **LLM-friendly output** - Sanitized, compact JSON for AI consumption
- **Profile-driven customization** - Override parser/perf markers via TOML config or generated templates
- **Flexible output** - Text or JSON format with color and verbosity control
