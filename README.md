# Log Analyzer

[![CI](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml)
[![Release](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml)

A CLI tool for analyzing and comparing JSON logs.

Core parser/comparison logic stays generic; domain-specific details can be supplied via profile config files.

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

| Option | Description |
|--------|-------------|
| `-s, --samples` | Show sample messages per component |
| `--json-schema` | Display JSON schema information |
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
log-analyzer diff file1.log file2.log -f "c:core-universal l:ERROR"

# Exclude DEBUG logs from comparison
log-analyzer diff file1.log file2.log -f "!l:DEBUG"

# Save JSON diff to file
log-analyzer -j -o diff.json diff file1.log file2.log

# Show operations slower than 500ms
log-analyzer perf file.log --threshold-ms 500

# Comprehensive log analysis
log-analyzer info file.log --samples --timeline --payloads

# LLM-friendly diff with custom limit
log-analyzer llm-diff file1.log file2.log --limit 50
```

## Environment Configuration

Set defaults via environment variables (prefix `LOG_ANALYZER_`):

```bash
export LOG_ANALYZER_FORMAT=json
export LOG_ANALYZER_FILTER="!l:DEBUG"
export LOG_ANALYZER_COMPACT=true
export LOG_ANALYZER_CONFIG="./config/profiles/base.toml"
```

## Profile Configuration

Use profile TOML files to keep the binary generic and push case-specific knowledge into config.

Included examples:

- `config/profiles/base.toml` - minimal reusable defaults
- `config/templates/custom-start.toml` - starter template for any project
- `config/templates/service-api.toml` - service/API wording template
- `config/templates/event-pipeline.toml` - event-driven wording template

Examples:

```bash
# Generic base profile
log-analyzer --config config/profiles/base.toml info logs/app.log
```

Create your own profile from templates:

```bash
# In this repository
cp config/templates/custom-start.toml config/profiles/my-team.toml

# If only the skill is installed globally
cp ~/.claude/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml

# Then run with your custom profile
log-analyzer --config config/profiles/my-team.toml info logs/app.log
```

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
