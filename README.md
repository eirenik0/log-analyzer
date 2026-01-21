# Log Analyzer

[![CI](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml)
[![Release](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml)

A CLI tool for analyzing and comparing JSON logs from the Applitools testing framework.

## Installation

```bash
# Auto-detect platform and install latest release
curl -fsSL https://raw.githubusercontent.com/eirenik0/log-analyzer/main/scripts/install.sh | bash

# Or build from source
cargo install --path .
```

## Quick Start

```bash
# Compare two log files
log-analyzer compare file1.log file2.log

# Show only differences
log-analyzer diff file1.log file2.log

# Get log file overview
log-analyzer info file.log

# Analyze performance bottlenecks
log-analyzer perf file.log

# Generate LLM-friendly output
log-analyzer llm file.log
```

## Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `compare` | `cmp` | Compare two log files |
| `diff` | | Compare showing only differences |
| `info` | `i`, `inspect` | Display log file statistics |
| `perf` | | Analyze operation timing |
| `process` | `llm` | Generate LLM-friendly JSON output |
| `llm-diff` | | Generate LLM-friendly diff output |

## Global Options

| Option | Env Variable | Description |
|--------|--------------|-------------|
| `-F, --format <text\|json>` | `LOG_ANALYZER_FORMAT` | Output format |
| `-c, --compact` | `LOG_ANALYZER_COMPACT` | Compact output mode |
| `-o, --output <path>` | `LOG_ANALYZER_OUTPUT_FILE` | Output file path |
| `--color <auto\|always\|never>` | `LOG_ANALYZER_COLOR` | Color output control |
| `-v, --verbose` | `LOG_ANALYZER_VERBOSE` | Increase verbosity |
| `-q, --quiet` | `LOG_ANALYZER_QUIET` | Show only errors |

## Filtering Options

Available for `compare`, `diff`, `llm`, `llm-diff`, and `perf` commands:

| Option | Env Variable | Description |
|--------|--------------|-------------|
| `-C, --component <name>` | `LOG_ANALYZER_COMPONENT` | Filter by component |
| `--exclude-component <name>` | `LOG_ANALYZER_EXCLUDE_COMPONENT` | Exclude component |
| `-l, --level <level>` | `LOG_ANALYZER_LEVEL` | Filter by log level |
| `--exclude-level <level>` | `LOG_ANALYZER_EXCLUDE_LEVEL` | Exclude log level |
| `-t, --contains <text>` | `LOG_ANALYZER_CONTAINS` | Filter by text content |
| `--exclude-text <text>` | `LOG_ANALYZER_EXCLUDE_TEXT` | Exclude by text |
| `-d, --direction <Incoming\|Outgoing>` | `LOG_ANALYZER_DIRECTION` | Filter by direction |
| `-s, --sort-by <field>` | `LOG_ANALYZER_SORT_BY` | Sort results |

## Command-Specific Options

### compare / diff

| Option | Description |
|--------|-------------|
| `-D, --diff-only` | Show only differences (always on for `diff`) |
| `-f, --full` | Show full JSON objects |

Sort options: `time`, `component`, `level`, `type`, `diffcount`

### info

| Option | Description |
|--------|-------------|
| `-s, --samples` | Show sample messages per component |
| `-j, --json-schema` | Display JSON schema information |
| `-p, --payloads` | Show payload statistics |
| `-t, --timeline` | Show timeline analysis |

### perf

| Option | Description |
|--------|-------------|
| `--threshold-ms <ms>` | Slow operation threshold (default: 1000) |
| `--top-n <number>` | Number of slowest operations (default: 20) |
| `--orphans-only` | Show only unfinished operations |
| `--op-type <Request\|Event\|Command>` | Filter by operation type |

Sort options: `duration`, `count`, `name`

### llm / llm-diff

| Option | Description |
|--------|-------------|
| `--limit <number>` | Max entries (default: 100, 0 = unlimited) |
| `--no-sanitize` | Disable sensitive field hiding |

## Examples

```bash
# Compare logs filtering by component and level
log-analyzer compare file1.log file2.log -C core-universal -l ERROR

# Save JSON diff to file
log-analyzer -F json -o diff.json diff file1.log file2.log

# Show operations slower than 500ms
log-analyzer perf file.log --threshold-ms 500

# Comprehensive log analysis
log-analyzer info file.log --samples --timeline --payloads

# LLM-friendly diff with custom limit
log-analyzer llm-diff file1.log file2.log --limit 50
```

## Environment Configuration

Set defaults via environment variables or `.env` file:

```bash
export LOG_ANALYZER_FORMAT=json
export LOG_ANALYZER_EXCLUDE_LEVEL=DEBUG
export LOG_ANALYZER_COMPACT=true
```

See `.env.example` for all available options.

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
/analyze-logs perf test.log --threshold-ms 500  # Find performance bottlenecks
/analyze-logs info test.log --samples           # Log structure overview
/analyze-logs llm test.log                      # Generate LLM-friendly output
```

## Features

- **Structured parsing** - Extracts and parses JSON payloads automatically
- **Semantic comparison** - Compares JSON objects regardless of property order
- **Advanced filtering** - Include/exclude by component, level, content, or direction
- **Performance analysis** - Identify slow and orphan operations
- **LLM-friendly output** - Sanitized, compact JSON for AI consumption
- **Flexible output** - Text or JSON format with color and verbosity control
