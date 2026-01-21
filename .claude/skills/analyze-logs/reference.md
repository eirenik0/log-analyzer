# Log Analyzer Command Reference

Complete documentation of all log-analyzer commands and options.

## Installation

### From Release Binary

Download the appropriate binary for your platform from GitHub Releases:

```bash
# macOS (Apple Silicon)
curl -LO https://github.com/anthropics/log-analyzer/releases/latest/download/log-analyzer-VERSION-aarch64-apple-darwin.tar.gz
tar xzf log-analyzer-*.tar.gz
sudo mv log-analyzer /usr/local/bin/

# macOS (Intel)
curl -LO https://github.com/anthropics/log-analyzer/releases/latest/download/log-analyzer-VERSION-x86_64-apple-darwin.tar.gz

# Linux (x86_64)
curl -LO https://github.com/anthropics/log-analyzer/releases/latest/download/log-analyzer-VERSION-x86_64-unknown-linux-gnu.tar.gz

# Linux (musl/Alpine)
curl -LO https://github.com/anthropics/log-analyzer/releases/latest/download/log-analyzer-VERSION-x86_64-unknown-linux-musl.tar.gz
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
| `-c, --compact` | flag | off | Use compact mode (shorter keys) |
| `-o, --output` | path | stdout | Save results to file |
| `--color` | `auto`, `always`, `never` | `auto` | Control color output |
| `-v, --verbose` | count | 0 | Increase verbosity (repeatable) |
| `-q, --quiet` | flag | off | Show only errors |

## Commands

### compare (alias: cmp)

Compare two log files and show differences.

```bash
log-analyzer compare <file1> <file2> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `-C, --component <name>` | Filter by component name |
| `--exclude-component <name>` | Exclude by component name |
| `-l, --level <level>` | Filter by log level (INFO, ERROR, WARN, DEBUG, TRACE) |
| `--exclude-level <level>` | Exclude by log level |
| `-t, --contains <text>` | Filter logs containing text |
| `--exclude-text <text>` | Exclude logs containing text |
| `-d, --direction <dir>` | Filter by direction (Incoming, Outgoing) |
| `-D, --diff-only` | Show only differences |
| `-f, --full` | Show full JSON objects |
| `-s, --sort-by <field>` | Sort by: time, component, level, type, diff-count |

**Examples:**
```bash
# Basic comparison
log-analyzer compare test1.log test2.log

# Show only differences
log-analyzer compare test1.log test2.log --diff-only

# Filter to errors in core component
log-analyzer compare test1.log test2.log -C core-universal -l ERROR

# JSON output sorted by difference count
log-analyzer compare test1.log test2.log -D -F json -s diff-count
```

### diff

Shortcut for `compare --diff-only`.

```bash
log-analyzer diff <file1> <file2> [options]
```

Same options as `compare` except `--diff-only` is implicit.

### info (aliases: i, inspect)

Display information about a log file.

```bash
log-analyzer info <file> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `-C, --component <name>` | Filter by component |
| `-l, --level <level>` | Filter by log level |
| `-s, --samples` | Show sample log messages |
| `-j, --json-schema` | Display JSON schema information |
| `-p, --payloads` | Show payload statistics |
| `-t, --timeline` | Show timeline analysis |

**Examples:**
```bash
# Full analysis
log-analyzer info test.log --samples --payloads --timeline

# JSON schema for a specific component
log-analyzer info test.log -C socket --json-schema

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
| `-C, --component <name>` | Filter by component |
| `--exclude-component <name>` | Exclude by component |
| `-l, --level <level>` | Filter by log level |
| `--exclude-level <level>` | Exclude by log level |
| `-t, --contains <text>` | Filter by text |
| `--exclude-text <text>` | Exclude by text |
| `-d, --direction <dir>` | Filter by direction |
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
log-analyzer llm test.log -l ERROR --limit 50
```

### llm-diff

Generate LLM-friendly diff output. Equivalent to `compare --diff-only -F json -c`.

```bash
log-analyzer llm-diff <file1> <file2> [options]
```

Same options as `llm` command.

### perf

Analyze operation timing and identify bottlenecks.

```bash
log-analyzer perf <file> [options]
```

**Options:**
| Option | Description |
|--------|-------------|
| `-C, --component <name>` | Filter by component |
| `--exclude-component <name>` | Exclude by component |
| `-l, --level <level>` | Filter by log level |
| `--exclude-level <level>` | Exclude by log level |
| `-t, --contains <text>` | Filter by text |
| `--exclude-text <text>` | Exclude by text |
| `-d, --direction <dir>` | Filter by direction |
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
# Find slow operations (>2s)
log-analyzer perf test.log --threshold-ms 2000

# Find incomplete operations
log-analyzer perf test.log --orphans-only

# Request analysis only
log-analyzer perf test.log --op-type Request --top-n 50

# Sort by occurrence count
log-analyzer perf test.log -s count
```

## Environment Variables

Configuration via environment variables (prefix: `LOG_ANALYZER_`):

| Variable | Description |
|----------|-------------|
| `LOG_ANALYZER_FORMAT` | Default output format |
| `LOG_ANALYZER_COMPACT` | Enable compact mode |
| `LOG_ANALYZER_OUTPUT_FILE` | Default output file |
| `LOG_ANALYZER_COLOR` | Color mode |
| `LOG_ANALYZER_VERBOSE` | Verbosity level (0-3) |
| `LOG_ANALYZER_QUIET` | Quiet mode |
| `LOG_ANALYZER_COMPONENT` | Default component filter |
| `LOG_ANALYZER_LEVEL` | Default level filter |

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
