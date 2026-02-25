# Log Analyzer Command Reference

Complete documentation of all log-analyzer commands and options.

## Installation

### Using Installation Script (Recommended)

The easiest way to install log-analyzer is using the installation script, which auto-detects your platform and downloads the appropriate binary:

```bash
./scripts/install.sh
```

This will:
- Detect your OS and architecture automatically
- Download the latest release binary
- Install to `~/bin/log-analyzer` by default
- Provide instructions for adding to PATH if needed

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

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `-F, --format` | `text`, `json` | `text` | Output format |
| `-j, --json` | flag | off | JSON output (shorthand for `-F json -c`) |
| `-c, --compact` | flag | off | Use compact mode (shorter keys) |
| `-f, --filter` | expression | none | Filter expression (see below) |
| `-o, --output` | path | stdout | Save results to file |
| `--config` | path | none | Load parser/perf/profile rules from TOML |
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
- `config/profiles/base.toml` - default parser/perf/profile baseline
- `config/templates/custom-start.toml` - generic starter with placeholders
- `config/templates/service-api.toml` - service/API oriented wording
- `config/templates/event-pipeline.toml` - event-driven pipeline wording

Built-in template names (for `generate-config --template`):
- `base`
- `custom-start`
- `service-api`
- `event-pipeline`

Use a profile with any command:

```bash
log-analyzer --config config/profiles/my-team.toml info ./logs/test.log
log-analyzer --config config/profiles/my-team.toml diff ./logs/a.log ./logs/b.log
```

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

### llm (alias: process)

Generate LLM-friendly compact JSON output.

```bash
log-analyzer llm <file> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `-s, --sort-by <field>` | Sort by: time, component, level, type |
| `--limit <n>` | Max entries (default: 100, 0 = unlimited) |
| `--no-sanitize` | Disable sensitive field redaction |

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
      "ts": "21:07:27.621",
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

Same options as `llm` command.

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
| `--top-n <n>` | Number of slowest ops (default: 20) |
| `--orphans-only` | Show only orphan operations |
| `--op-type <type>` | Filter: Request, Event, Command |
| `-s, --sort-by <field>` | Sort by: duration, count, name |

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
log-analyzer perf ./logs/*.log --op-type Request --top-n 50

# Sort by occurrence count
log-analyzer perf ./logs/*.log -s count
```

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

Analyze a log file and generate a TOML config profile.

```bash
log-analyzer generate-config <file> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `--profile-name <name>` | Name to set in `profile_name` (defaults to input filename stem) |
| `--template <path\|name>` | Base profile to inherit parser/perf rules from (`base`, `custom-start`, `service-api`, `event-pipeline`) |

Output is always TOML. Use global `-o, --output` to write generated profile files.

**Examples:**
```bash
# Print generated profile TOML to stdout
log-analyzer generate-config ./logs/test.log --profile-name test-run

# Save generated profile for skill reuse
log-analyzer generate-config ./logs/test.log --profile-name eyes-cypress \
  -o .claude/skills/analyze-logs/profiles/eyes-cypress.toml

# Generate using parser/perf rules from a built-in template
log-analyzer generate-config ./logs/test.log \
  --template service-api \
  --profile-name service-api \
  -o .claude/skills/analyze-logs/profiles/service-api.toml
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
| `LOG_ANALYZER_SORT_BY` | Default sort order |

## Log Format

The tool expects logs in this format:
```
component | timestamp [LEVEL] message
```

Example:
```
socket | 2025-04-03T21:07:27.668Z [INFO ] Emit event of type "Logger.log" with payload {...}
core-universal | 2025-04-03T21:07:27.652Z [INFO ] Core universal is started on port 21077
```

Supports multi-line JSON payloads embedded in messages.
