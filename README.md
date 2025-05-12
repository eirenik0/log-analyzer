# Log Analyzer

A command-line tool for analyzing and comparing JSON logs from the Applitools testing framework.

## Overview

This tool helps you extract, filter, and compare structured log data from Applitools log files. It's especially useful for:

- Finding differences between test runs
- Debugging test failures
- Analyzing log patterns

## Usage

### Basic Commands

**Compare two log files:**
```bash
log-analyzer compare path/to/first.log path/to/second.log
```

**Get information about a log file:**
```bash
log-analyzer info path/to/logfile.log
```

### Command Aliases and Shortcuts

For convenience, the following command aliases and shortcuts are available:

| Command | Aliases | Description |
|---------|---------|-------------|
| `compare` | `cmp` | Compare logs with all options |
| `diff` | | Compare logs showing only differences (shortcut for `compare --diff-only`) |
| `llm` | | Generate LLM-friendly JSON output (shortcut for `compare --diff-only -F json -c`) |
| `info` | `i`, `inspect` | Display log file information |

### Global Options

The following options can be used with any command:

| Option | Description |
|--------|-------------|
| `-F, --format <format>` | Output format: `text` (default) or `json` |
| `-c, --compact` | Use compact mode for output (shorter keys, optimized structure) |
| `-o, --output <path>` | Path to output file for the results |
| `--color <mode>` | Control color output: `auto` (default), `always`, or `never` |
| `-v, --verbose` | Increase output verbosity (can be used multiple times) |
| `-q, --quiet` | Suppress all output except errors |

### Compare Options

The `compare` command supports several options to fine-tune your analysis:

| Option | Description |
|--------|-------------|
| `-C, --component <name>` | Filter logs by component (e.g., "core-universal", "socket") |
| `--exclude-component <name>` | Exclude logs with specific component |
| `-l, --level <level>` | Filter logs by log level (e.g., "INFO", "ERROR") |
| `--exclude-level <level>` | Exclude logs with specific level (e.g., "DEBUG") |
| `-t, --contains <text>` | Filter logs containing specific text |
| `--exclude-text <text>` | Exclude logs containing specific text |
| `-d, --direction <direction>` | Filter logs by communication direction (Incoming/Outgoing) |
| `-D, --diff-only` | Show only differences, skip matching objects |
| `-f, --full` | Show full JSON objects, not just the differences |
| `-s, --sort-by <field>` | Sort results by: `time` (default), `component`, `level`, `type`, or `diffcount` |

### Examples

**Compare logs from two test runs showing only differences:**
```bash
log-analyzer compare universal-1.log universal-2.log -D
```

**Compare logs filtering by component and level:**
```bash
log-analyzer compare universal-1.log universal-2.log -C core-universal -l ERROR
```

**Save comparison results to a file:**
```bash
log-analyzer -o diff.log compare universal-1.log universal-2.log
```

**Filter logs by direction and output as JSON:**
```bash
log-analyzer -F json compare universal-1.log universal-2.log -d Outgoing
```

**Use compact mode with JSON output:**
```bash
log-analyzer -F json -c compare universal-1.log universal-2.log
```

**Sort results by component and exclude DEBUG level logs:**
```bash
log-analyzer compare universal-1.log universal-2.log -s component --exclude-level DEBUG
```

**Use environment variables for common settings:**
```bash
# Set in .env file or export in shell
export LOG_ANALYZER_FORMAT=json
export LOG_ANALYZER_EXCLUDE_LEVEL=DEBUG
log-analyzer compare universal-1.log universal-2.log
```

**Use diff command with increased verbosity:**
```bash
log-analyzer diff universal-1.log universal-2.log -v -s diffcount
```

**The diff command is a shortcut for compare with --diff-only:**
```bash
# These commands are equivalent:
log-analyzer diff universal-1.log universal-2.log
log-analyzer compare universal-1.log universal-2.log -D
```

**Use the llm command for machine-readable JSON output:**
```bash
# Generate compact JSON diff for LLM consumption:
log-analyzer llm universal-1.log universal-2.log > diff.json

# Equivalent to:
log-analyzer compare universal-1.log universal-2.log -D -F json -c > diff.json
```

## Features

- **Structured log parsing**: Automatically extracts and parses JSON payloads
- **Intelligent comparison**: Semantically compares JSON objects, regardless of property order
- **Advanced filtering**: Include/exclude logs by component, level, content, or direction
- **Log categorization**: Automatically categorizes logs into events, commands, and requests
- **Multiple output formats**: Choose between human-readable text or JSON for programmatic processing
- **Detailed output**: Clear, readable comparison output with highlighting for differences
- **Flexible output options**: Save results to a file, use compact mode for more concise output
- **Optimized CLI**: Well-organized command structure with aliases and global options
- **Environment configuration**: Set defaults via environment variables or `.env` file
- **Output customization**: Control colors, verbosity, and sorting for better analysis

## CLI Enhancements

This tool includes several advanced CLI features to improve usability:

### Command Shortcuts and Aliases

- **Command aliases**: Use shorthand commands like `cmp` for `compare` and `i` for `info`
- **Specialized commands**: Built-in shortcut commands that combine commonly-used options:
  - `diff` command: Automatically shows only differences (equivalent to `compare --diff-only`)
  - `llm` command: Optimized for LLM consumption (equivalent to `compare --diff-only -F json -c`)

### Advanced Filtering

- **Include filters**: Filter logs by component, level, or text content
- **Exclude filters**: Skip logs with specified component, level, or text
- **Directional filtering**: Filter by communication direction (incoming or outgoing)
- **Combined filtering**: Mix and match filters for precise log selection

### Output Control

- **Format selection**: Choose between human-readable text or JSON output
- **Compact mode**: More concise output with shortened keys for space efficiency
- **Color control**: Configure when colors are used (`auto`, `always`, or `never`)
- **Verbosity levels**: Control detail level from quiet (errors only) to verbose (all details)
- **Custom sorting**: Sort results by time, component, level, type, or difference count

### Environment Configuration

- **Environment variables**: Configure options via environment variables (prefixed with `LOG_ANALYZER_`)
- **.env support**: Store common configurations in a `.env` file for persistence
- **Example configuration**: See `.env.example` for available environment options

## Complete Command Reference

### Global Options

| Option | Environment Variable | Description |
|--------|---------------------|-------------|
| `-F, --format <text\|json>` | `LOG_ANALYZER_FORMAT` | Output format (text or JSON) |
| `-c, --compact` | `LOG_ANALYZER_COMPACT` | Use compact mode for output |
| `-o, --output <path>` | `LOG_ANALYZER_OUTPUT_FILE` | Path to output file for results |
| `--color <auto\|always\|never>` | `LOG_ANALYZER_COLOR` | Control color output |
| `-v, --verbose` | `LOG_ANALYZER_VERBOSE` | Increase verbosity (can be used multiple times) |
| `-q, --quiet` | `LOG_ANALYZER_QUIET` | Be quiet, show only errors |

### Commands

#### `compare` (aliases: `cmp`)
Compare two log files with all options

```bash
log-analyzer compare [OPTIONS] <FILE1> <FILE2>
```

| Option | Environment Variable | Description |
|--------|---------------------|-------------|
| `-C, --component <name>` | `LOG_ANALYZER_COMPONENT` | Filter logs by component |
| `--exclude-component <name>` | `LOG_ANALYZER_EXCLUDE_COMPONENT` | Exclude logs with specific component |
| `-l, --level <level>` | `LOG_ANALYZER_LEVEL` | Filter logs by level |
| `--exclude-level <level>` | `LOG_ANALYZER_EXCLUDE_LEVEL` | Exclude logs with specific level |
| `-t, --contains <text>` | `LOG_ANALYZER_CONTAINS` | Filter logs containing specific text |
| `--exclude-text <text>` | `LOG_ANALYZER_EXCLUDE_TEXT` | Exclude logs containing specific text |
| `-d, --direction <Incoming\|Outgoing>` | `LOG_ANALYZER_DIRECTION` | Filter logs by direction |
| `-D, --diff-only` | | Show only differences, skip matching objects |
| `-f, --full` | | Show full JSON objects, not just differences |
| `-s, --sort-by <field>` | `LOG_ANALYZER_SORT_BY` | Sort by: time, component, level, type, diffcount |

#### `diff`
Compare logs showing only differences (shortcut for `compare --diff-only`)

```bash
log-analyzer diff [OPTIONS] <FILE1> <FILE2>
```

Supports all options as `compare` except `--diff-only` which is always enabled.

#### `llm`
Generate LLM-friendly compact JSON output (shortcut for `compare --diff-only -F json -c`)

```bash
log-analyzer llm [OPTIONS] <FILE1> <FILE2>
```

Supports filtering options from `compare` but always uses JSON format, compact mode, and shows only differences.

#### `info` (aliases: `i`, `inspect`)
List all components, event types, log levels, and detailed statistics in a log file

```bash
log-analyzer info [OPTIONS] <FILE>
```

| Option | Description |
|--------|-------------|
| `-s, --samples` | Show sample log messages for each component |
| `-j, --json-schema` | Display detailed JSON schema information for event payloads |
| `-C, --component <n>` | Filter logs by component |
| `-l, --level <level>` | Filter logs by log level |
| `-p, --payloads` | Show payload statistics for each event/command/request type |
| `-t, --timeline` | Show detailed timeline analysis with event distribution |

The enhanced `info` command provides deep insights into log structure and content:

```bash
# Show sample log messages for each component
log-analyzer info --samples universal-1.log

# Display detailed timeline analysis with event distribution
log-analyzer info --timeline universal-1.log

# Show JSON schema information and payload statistics
log-analyzer info --json-schema --payloads universal-1.log

# Filter to analyze only specific component
log-analyzer info --component core-ufg --timeline universal-1.log

# Comprehensive analysis with all options
log-analyzer info --samples --json-schema --payloads --timeline universal-1.log
```

