---
name: analyze-logs
description: Analyze, compare, and debug structured logs. Use when comparing log files, finding failures, identifying performance bottlenecks, or preparing logs for analysis.
argument-hint: <command> [files...] [options]
allowed-tools: Bash(cargo run:*), Bash(./target/release/log-analyzer:*), Bash(log-analyzer:*), Read, Glob, Grep
context: fork
---

# Log Analyzer Skill

Analyze structured logs using the `log-analyzer` CLI tool.

The default `base` profile is intentionally generic. When behavior needs to be case-specific, prefer a built-in preset or pass a profile config:

```bash
log-analyzer --preset eyes <command> ...
log-analyzer --config config/profiles/custom.toml <command> ...
```

Shipped profiles use version-2 whole-message/structured `event_rules`; customize the explicit
rules when your producer's messages differ. Legacy-only custom marker profiles
retain their semantics. Mixed legacy/global explicit lifecycle settings fail loading.
Check unmatched/ambiguous/invalid evidence and `operation_coverage.classification`
before interpreting empty timing results. `generate-config` preserves template mode.
For producers that intentionally emit no end, version-2 start rules can set
`end_expected = false`. `perf` counts these as `start_only_events` without orphans
or measured durations; configured session completion commands can complete on
them. Operation-type filters count excluded start-only records as suppressed.
Record-field scopes over 4096 bytes use bounded prefix/length/digest keys rather
than failing scope lookup; summaries containing the reserved digest marker are
also encoded. Treat these keys as summaries, not the original scope. Explicit
event-rule scope mappings use the same encoding after validation and retain their
input limit. See [reference.md](reference.md)
for correlation details and digest limitations.

Profiles can also define session hierarchy/lifecycle hints with `[[sessions.levels]]` (for example runner/test/environment prefixes plus create/complete commands). `info` will then report session completion health per level.

For small customizations, create a TOML profile with top-level `extends = "eyes"`
or another built-in name, then specify only the overrides. File parents resolve
relative to the child. Tables merge; arrays and scalars replace inherited values.
Chains allow at most eight profiles including the child and any built-in parent.
Keep inherited version-2 `event_rules` separate from legacy lifecycle markers.
See [reference.md](reference.md#profile-templates) for an example and resolution rules.

Create a custom profile from a template:

```bash
# If this repo is available
cp config/templates/custom-start.toml config/profiles/my-team.toml

# If only the skill is installed globally
cp ~/.claude/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml
```

Built-in templates/presets are also available directly in the binary:

```bash
log-analyzer --preset eyes info ./logs/*.log
log-analyzer generate-config ./logs/*.log --template service-api --profile-name my-team
```

`generate-config` auto-detects session-like `component_id` prefixes and writes generic `[[sessions.levels]]` entries. It accepts one or more related logs (merged before inference).

## Commands Overview

| Command | Purpose | Example |
|---------|---------|---------|
| `diff` | Show differences between two log files | `/analyze-logs diff file1.log file2.log` |
| `compare` | Full comparison with all matches | `/analyze-logs compare file1.log file2.log` |
| `info` | Analyze structure across one or more logs | `/analyze-logs info ./logs/*.log --samples` |
| `search` | Structured grep-style search for matching entries | `/analyze-logs search test.log -f "t:timeout" --context 2` |
| `errors` | Cluster recurring ERROR/WARN patterns and session impact | `/analyze-logs errors ./logs/*.log --warn --sessions` |
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
curl -fsSL https://raw.githubusercontent.com/eirenik0/log-analyzer/main/scripts/install.sh | bash
```

If you are working inside a cloned `log-analyzer` repo, this also works:
```bash
./scripts/install.sh
```

Or build from source:
```bash
cargo build --release
```

Verify installation:
```bash
log-analyzer --version
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
     curl -fsSL https://raw.githubusercontent.com/eirenik0/log-analyzer/main/scripts/install.sh | bash

   If you're inside a cloned log-analyzer repo, this also works:
     ./scripts/install.sh

   Or build from source:
     cargo build --release
   ```

2. **Parse the request** to determine:
   - Which command is needed (diff, compare, info, search, errors, extract, perf, trace, process/llm, llm-diff, generate-config)
   - Which log file(s) to analyze (one file or multiple files/globs)
   - Any filtering options (component, level, text)

3. **Find log files** if not specified:
   ```bash
   # Look for .log files in the project
   find . -name "*.log" -type f 2>/dev/null | head -10
   ```

4. **Build the command** with appropriate options:
   - Use `log-analyzer` if installed, otherwise `./target/release/log-analyzer` or `cargo run --`
   - If a path contains spaces, quote the directory/path but keep the wildcard outside quotes (for example `"/path with spaces"/*.log`, not `"/path with spaces/*.log"`)
   - Do not pass expanded globs to single-file commands (`search`, `extract`, `llm`/`process`); choose one file or switch to a multi-file command
   - Check parse coverage before trusting findings: nonempty unparsed input exits 1; empty input and zero filter matches have distinct statuses. Browser console source prefixes on classic entries are supported and preserved as `console_source`.
   - For long stacks or a fixed reading budget, use `errors --bounded --max-output-chars 2400`; inspect omission counts. Scope/totals/impact remain first. JSON keeps totals and bounded details, while the total character budget applies to text.
   - For debugging failures, run `errors` first (or one of the first commands) to get a structured error inventory before deeper investigation
   - After the initial `errors` pass, build the causal chain with targeted follow-up queries: manager creation patterns (`search`), concurrency config extraction (`extract --field concurrency` or `search --count-by payload`), and SDK path tracing (`trace --id` / `--session`)
   - Use `diff` with `--diff-only` when comparing expected vs failing logs after the initial `errors` pass
   - For grep-like inspection with structured filters: use `search` (optionally `--context`, `--payloads`, or `--count-by payload`)
   - Structured tracing/json fields can be filtered directly with `-f "trace_id:abc123"` or `-f "actor_kind:switch"`
   - For "what went wrong?" diagnosis: use `errors` (optionally `--warn`, `--sessions`, `--sort-by impact`, `--top-n 0` for all clusters)
   - For aggregating one payload/settings field across matches: use `extract --field <path>` (for example `--field retryTimeout` or a tracing field like `--field restream_name`)
   - For performance issues: use `perf` with appropriate threshold (pass multiple files only when they belong to the same run/session for meaningful timing/orphan analysis)
   - For tracing one operation/session: use `trace --id <id-fragment>` or `trace --session <component_id-fragment>` (multiple files are fine when they are from the same run/session)
   - For understanding logs: use `info` with `--samples --payloads` (pass multiple files only when they are related, e.g. split output from one run)
   - If a profile includes `[[sessions.levels]]`, mention the per-level session completion summary from `info` in your findings
   - If the logs match a known built-in grammar (for example Eyes/Applitools logs), prefer `--preset eyes` for analysis commands and `--template eyes` for profile generation
   - For profile generation: use `generate-config`; it will infer parser/profile hints and generic session levels from one or more related logs (merged before inference), and default `-o` to `.claude/skills/analyze-logs/profiles/<name>.toml` if not provided
   - `--template` can be either a file path or built-in name: `base`, `eyes`, `custom-start`, `service-api`, `event-pipeline`

5. **Execute and interpret**:
   - Run the log-analyzer command
   - Summarize key findings in plain language
   - Highlight actionable items (errors, slow operations, differences)
   - Suggest next steps if issues are found

## Common Workflows

### Debug Test Failure
```bash
# First pass: structured error inventory ("what went wrong?")
log-analyzer --preset eyes errors failing.log --warn --sessions

# Follow-up: inspect manager creation / setup patterns
log-analyzer search failing.log -f "t:makeManager" --payloads

# Follow-up: verify concurrency (or similar config) values across matches
log-analyzer extract failing.log -f "t:makeManager" --field concurrency

# Follow-up: trace one failing request/session to reconstruct SDK path
log-analyzer trace failing.log --session manager-

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
log-analyzer --preset eyes perf ./logs/*.log --threshold-ms 2000

# Find orphan operations (can pair across files)
log-analyzer --preset eyes perf ./logs/*.log --orphans-only

# Focus on requests only
log-analyzer --preset eyes perf ./logs/*.log --op-type request --top-n 30
```

Only combine files from the same session/run. Mixing unrelated logs can make latency stats and orphan results meaningless.

### Trace One Operation / Session
```bash
# Trace by correlation/request ID fragment across split logs
log-analyzer trace ./logs/*.log --id f227f11e

# Trace by component_id hierarchy/session path
log-analyzer --preset eyes trace ./logs/*.log --session manager-ufg-3nl
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

# Cluster recurring failures and include per-session outcomes (completed vs orphaned)
log-analyzer errors ./logs/*.log --warn --sessions --sort-by impact

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

### Evidence identity

Use the shared [evidence contract](../../../docs/design/evidence-contract.md)
and published schemas. Verify input/profile identities before reusing citations.
Source references survive selection, sorting and compaction; aggregate counts
need record retrieval. Describe trace spans and legacy error span estimates with
their declared semantics. Keep observations, measurements, hypotheses, contrary
evidence and unknowns distinct in investigation findings.

For common bounded JSON investigation output, use the report budget controls
advertised by capabilities. Follow snapshot-bound cursors without changing
selection/redaction and inspect stop/omission states before concluding. Use
`--complete-output` when needed; see the common-budget reference for compatibility.
