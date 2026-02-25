---
name: analyze-logs
description: Analyze, compare, and debug structured logs. Use when comparing log files, finding failures, identifying performance bottlenecks, or preparing logs for analysis.
argument-hint: <command> [files...] [options]
allowed-tools: Bash(cargo run:*), Bash(./target/release/log-analyzer:*), Bash(log-analyzer:*), Read, Glob, Grep
context: fork
---

# Log Analyzer Skill

Analyze structured logs using the `log-analyzer` CLI tool.

When behavior needs to be case-specific, pass a profile config:

```bash
log-analyzer --config config/profiles/base.toml <command> ...
log-analyzer --config config/profiles/custom.toml <command> ...
```

Profiles can also define session hierarchy/lifecycle hints with `[[sessions.levels]]` (for example runner/test/environment prefixes plus create/complete commands). `info` will then report session completion health per level. Legacy `[profile.session_prefixes]` still works.

Create a custom profile from a template:

```bash
# If this repo is available
cp config/templates/custom-start.toml config/profiles/my-team.toml

# If only the skill is installed globally
cp ~/.claude/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml
```

Built-in templates are also available directly in the binary:

```bash
log-analyzer generate-config ./logs/*.log --template service-api --profile-name my-team
```

`generate-config` auto-detects session-like `component_id` prefixes and writes generic `[[sessions.levels]]` entries. It accepts one or more related logs (merged before inference). Use `config/profiles/eyes.toml` when you want Eyes-specific session lifecycle metadata (create/complete commands, summary fields).

## Commands Overview

| Command | Purpose | Example |
|---------|---------|---------|
| `diff` | Show differences between two log files | `/analyze-logs diff file1.log file2.log` |
| `compare` | Full comparison with all matches | `/analyze-logs compare file1.log file2.log` |
| `info` | Analyze structure across one or more logs | `/analyze-logs info ./logs/*.log --samples` |
| `search` | Structured grep-style search for matching entries | `/analyze-logs search test.log -f "t:timeout" --context 2` |
| `extract` | Extract and aggregate a payload field from matching entries | `/analyze-logs extract test.log -f "t:makeManager" --field concurrency` |
| `perf` | Find performance bottlenecks across one or more logs | `/analyze-logs perf ./logs/*.log --threshold-ms 500` |
| `trace` | Trace one operation/session lifecycle across one or more logs | `/analyze-logs trace ./logs/*.log --id f227f11e` |
| `llm` | Generate LLM-friendly output | `/analyze-logs llm test.log` |
| `llm-diff` | LLM-friendly diff output | `/analyze-logs llm-diff file1.log file2.log` |
| `generate-config` | Generate a profile TOML from one or more related logs | `/analyze-logs generate-config ./logs/*.log --profile-name my-team` |

See [reference.md](reference.md) for complete command documentation.

## Quick Start

### Installation

Install the latest release binary (recommended):
```bash
./scripts/install.sh
```

Or build from source:
```bash
cargo build --release
```

### Running Commands

If installed via `scripts/install.sh`:
```bash
log-analyzer <command> [options]
```

If built from source:
```bash
./target/release/log-analyzer <command> [options]
```

Or during development:
```bash
cargo run -- <command> [options]
```

## Task Instructions

When the user invokes this skill:

1. **Check if log-analyzer is available**:
   ```bash
   # Check if binary is installed
   command -v log-analyzer || which log-analyzer || [ -f ./target/release/log-analyzer ]
   ```

   If not available, inform the user:
   ```
   The log-analyzer tool is not installed. Install it with:
     ./scripts/install.sh

   Or build from source:
     cargo build --release
   ```

2. **Parse the request** to determine:
   - Which command is needed (diff, compare, info, search, extract, perf, trace, llm, generate-config)
   - Which log file(s) to analyze (one file or multiple files/globs)
   - Any filtering options (component, level, text)

3. **Find log files** if not specified:
   ```bash
   # Look for .log files in the project
   find . -name "*.log" -type f 2>/dev/null | head -10
   ```

4. **Build the command** with appropriate options:
   - Use `log-analyzer` if installed, otherwise `./target/release/log-analyzer` or `cargo run --`
   - For debugging failures: use `diff` with `--diff-only`
   - For grep-like inspection with structured filters: use `search` (optionally `--context`, `--payloads`, or `--count-by payload`)
   - For aggregating one payload/settings field across matches: use `extract --field <path>` (for example `--field retryTimeout`)
   - For performance issues: use `perf` with appropriate threshold (pass multiple files only when they belong to the same run/session for meaningful timing/orphan analysis)
   - For tracing one operation/session: use `trace --id <id-fragment>` or `trace --session <component_id-fragment>` (multiple files are fine when they are from the same run/session)
   - For understanding logs: use `info` with `--samples --payloads` (pass multiple files only when they are related, e.g. split output from one run)
   - If a profile includes `[[sessions.levels]]`, mention the per-level session completion summary from `info` in your findings
  - For profile generation: use `generate-config`; it will infer parser/profile hints and generic session levels from one or more related logs (merged before inference), and default `-o` to `.claude/skills/analyze-logs/profiles/<name>.toml` if not provided
   - `--template` can be either a file path or built-in name: `base`, `custom-start`, `service-api`, `event-pipeline`

5. **Execute and interpret**:
   - Run the log-analyzer command
   - Summarize key findings in plain language
   - Highlight actionable items (errors, slow operations, differences)
   - Suggest next steps if issues are found

## Common Workflows

### Debug Test Failure
```bash
# Quick diff to see what changed
log-analyzer diff passing.log failing.log

# Focus on errors only
log-analyzer diff passing.log failing.log -f "l:ERROR"

# Focus on specific component
log-analyzer diff passing.log failing.log -f "c:core-universal"

# Combined filters
log-analyzer diff passing.log failing.log -f "c:core l:ERROR !t:timeout"
```

### Performance Investigation
```bash
# Find operations taking > 2 seconds across split session logs
log-analyzer perf ./logs/*.log --threshold-ms 2000

# Find orphan operations (can pair across files)
log-analyzer perf ./logs/*.log --orphans-only

# Focus on requests only
log-analyzer perf ./logs/*.log --op-type Request --top-n 30
```

Only combine files from the same session/run. Mixing unrelated logs can make latency stats and orphan results meaningless.

### Trace One Operation / Session
```bash
# Trace by correlation/request ID fragment across split logs
log-analyzer trace ./logs/*.log --id f227f11e

# Trace by component_id hierarchy/session path
log-analyzer trace ./logs/*.log --session manager-ufg-3nl
```

Only combine related files from the same run/session so the trace timeline stays meaningful.

### Log Exploration
```bash
# Full overview with samples across multiple files
log-analyzer info ./logs/*.log --samples --payloads --timeline

# Structured grep replacement with context
log-analyzer search test.log -f "t:retryTimeout" --context 2

# Count/group matching entries by parsed payload
log-analyzer search test.log -f "t:concurrency" --count-by payload

# Extract and aggregate a specific payload field
log-analyzer extract test.log -f "t:makeManager" --field concurrency

# JSON output for further processing
log-analyzer info ./logs/*.log -j
```

Only combine related files (for example, rotated chunks of the same run). Otherwise counts and timeline distributions may not be useful.

### Prepare for LLM Analysis
```bash
# Sanitized, compact output
log-analyzer llm test.log --limit 100 -o context.json

# Diff for LLM
log-analyzer llm-diff file1.log file2.log -o diff.json
```

### Generate Config Profile
```bash
# Generate from related split logs and save to skill-local profiles directory
log-analyzer generate-config ./logs/*.log --profile-name cypress \
  -o .claude/skills/analyze-logs/profiles/cypress.toml

# Inherit parser/perf rules from a template while generating profile hints
log-analyzer generate-config ./logs/*.log \
  --template service-api \
  --profile-name service-api \
  -o .claude/skills/analyze-logs/profiles/service-api.toml
```

Only combine related logs from the same run/session to avoid polluting inferred profile hints.

## Filter Expression Syntax

Use `-f, --filter` with unified expression syntax:

```bash
-f "type:value [!type:value] ..."
```

**Filter types (with aliases):**
| Type | Aliases | Description |
|------|---------|-------------|
| `component` | `comp`, `c` | Filter by component name |
| `level` | `lvl`, `l` | Filter by log level |
| `text` | `t` | Filter by text in message |
| `direction` | `dir`, `d` | Filter by direction |

**Prefix with `!` to exclude.** Examples:
```bash
-f "c:core-universal"           # Only core-universal component
-f "l:ERROR"                    # Only ERROR level logs
-f "c:core !l:DEBUG"            # Core component, exclude DEBUG
-f "t:timeout d:incoming"       # Contains 'timeout', incoming only
```

Filter semantics:
- Different filter types combine with AND
- Multiple values of the same filter type combine with OR

## Output Formats

- `-F text` - Human-readable colored output (default)
- `-F json` - Structured JSON output
- `-j, --json` - JSON output shorthand (implies `-F json -c`)
- `-c, --compact` - Shortened keys for compact output
- `-o, --output <path>` - Save to file

## Interpreting Results

### Diff Output
- **unique_to_log1** / **unique_to_log2**: Events only in one file
- **shared_comparisons**: Matching events with field differences
- Focus on configuration changes, error status changes, and timing differences

### Perf Output
- **Slowest Operations**: Operations exceeding threshold
- **Orphan Operations**: Started but never completed (potential hangs)
- **Statistics**: P50, P95, P99 latencies per operation type

### Info Output
- **Components**: All log sources in the input log file(s)
- **Event Types**: Categorized operations
- **Timeline**: Distribution of events over time across the merged input timeline
