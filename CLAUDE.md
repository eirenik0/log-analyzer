# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Log-analyzer is a Rust command-line tool designed to analyze and compare JSON logs from the Applitools testing framework. It can parse complex log files, extract JSON payloads, and identify differences between log files.

## Commands

### Building the Project

```bash
# Build the project
cargo build

# Run in development mode
cargo run -- <arguments>

# Build for release
cargo build --release

# Cross-compile for multiple platforms using the build script
./build.sh
```

### Running the Application

```bash
# Compare two log files
cargo run -- compare <file1> <file2> [options]

# Show only differences between log files (shortcut for compare --diff-only)
cargo run -- diff <file1> <file2> [options]

# Display information about a log file
cargo run -- info <file> [options]

# Generate LLM-friendly compact JSON output from a single log file
cargo run -- llm <file> [options]

# Generate LLM-friendly compact JSON output of differences
cargo run -- llm-diff <file1> <file2> [options]

# Analyze operation timing and identify performance bottlenecks
cargo run -- perf <file> [options]
```

### Global Options

These options can be used with any command:
- `-F, --format <text|json>` - Output format (default: text)
- `-c, --compact` - Use compact mode for output (shorter keys, optimized structure)
- `-o, --output <path>` - Path to output file for results
- `--color <auto|always|never>` - Control color output (default: auto)
- `-v, --verbose` - Increase verbosity level (can be used multiple times)
- `-q, --quiet` - Be quiet, show only errors

### Compare Command (alias: `cmp`)

Compare two log files and show differences between JSON objects.

Options:
- `-C, --component <name>` - Filter logs by component name
- `--exclude-component <name>` - Exclude logs by component name
- `-l, --level <level>` - Filter logs by log level (INFO, ERROR, etc.)
- `--exclude-level <level>` - Exclude logs by log level
- `-t, --contains <text>` - Filter logs containing specific text
- `--exclude-text <text>` - Exclude logs containing specific text
- `-d, --direction <Incoming|Outgoing>` - Filter logs by communication direction
- `-D, --diff-only` - Show only differences between logs
- `-f, --full` - Show full JSON objects, not just differences
- `-s, --sort-by <field>` - Sort output by field (time, component, level, type, diff-count)

### Diff Command

Shortcut for `compare --diff-only`. Same options as Compare command (except `--diff-only`).

### Info Command (aliases: `i`, `inspect`)

List all components, event types, log levels, and detailed statistics in a log file.

Options:
- `-C, --component <name>` - Filter logs by component name
- `-l, --level <level>` - Filter logs by log level
- `-s, --samples` - Show sample log messages for each component
- `-j, --json-schema` - Display detailed JSON schema information for event payloads
- `-p, --payloads` - Show payload statistics for each event/command/request type
- `-t, --timeline` - Show detailed timeline analysis with event distribution

### LLM Command (alias for `process`)

Generate LLM-friendly compact JSON output of a single log file with sanitized content.

Options:
- `-C, --component <name>` - Filter logs by component name
- `--exclude-component <name>` - Exclude logs by component name
- `-l, --level <level>` - Filter logs by log level
- `--exclude-level <level>` - Exclude logs by log level
- `-t, --contains <text>` - Filter logs containing specific text
- `--exclude-text <text>` - Exclude logs containing specific text
- `-d, --direction <Incoming|Outgoing>` - Filter logs by communication direction
- `-s, --sort-by <field>` - Sort output by field (time, component, level, type)
- `--limit <number>` - Maximum number of log entries to include (default: 100, 0 = unlimited)
- `--no-sanitize` - Disable hiding of sensitive fields from JSON payloads (sanitization is enabled by default)

### LLM-Diff Command

Generate LLM-friendly compact JSON output of differences (shortcut for `compare --diff-only -F json -c`).

Options: Same filtering options as LLM command, plus `--no-sanitize`.

### Perf Command

Analyze operation timing and identify performance bottlenecks.

Options:
- `-C, --component <name>` - Filter logs by component name
- `--exclude-component <name>` - Exclude logs by component name
- `-l, --level <level>` - Filter logs by log level
- `--exclude-level <level>` - Exclude logs by log level
- `-t, --contains <text>` - Filter logs containing specific text
- `--exclude-text <text>` - Exclude logs containing specific text
- `-d, --direction <Incoming|Outgoing>` - Filter logs by communication direction
- `--threshold-ms <ms>` - Duration threshold in milliseconds for highlighting slow operations (default: 1000)
- `--top-n <number>` - Number of slowest operations to display (default: 20)
- `--orphans-only` - Show only orphan operations (started but never finished)
- `--op-type <Request|Event|Command>` - Filter by operation type
- `-s, --sort-by <field>` - Sort results by field (duration, count, name)

### Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name -- --nocapture
```

## Code Architecture

### Core Modules

1. **Parser (`src/parser.rs`)** - Parses log files into structured data
   - Extracts components, timestamps, log levels, and messages
   - Identifies and parses JSON payloads from log messages
   - Categorizes logs into different types (events, commands, requests)

2. **Comparator (`src/comparator.rs`)** - Compares logs and identifies differences
   - Compares JSON payloads while respecting object structure
   - Finds differences regardless of object property order
   - Generates human-readable difference reports

3. **CLI (`src/cli.rs`)** - Command-line interface using Clap
   - Defines the CLI commands and arguments
   - Handles parameter parsing and validation

4. **LLM Processor (`src/llm_processor.rs`)** - Generates LLM-friendly output
   - Sanitizes sensitive data from log payloads
   - Produces compact JSON output optimized for LLM consumption

5. **Performance Analyzer (`src/perf_analyzer/`)** - Analyzes operation timing
   - Tracks operation durations (requests, events, commands)
   - Identifies orphan operations (started but never finished)
   - Calculates performance statistics and identifies bottlenecks

6. **Library (`src/lib.rs`)** - Core library exposing public API
   - Orchestrates command execution
   - Provides filtering and output formatting

### Key Data Types

- `LogEntry` - Represents a single log entry with all metadata
- `LogEntryKind` - Enum for different types of logs (Generic, Event, Command, Request)
- `ComparisonResults` - Contains comparison results between two log files
- `LogFilter` - Used to filter logs by component, level, text, or direction
- `PerfStats` - Contains performance analysis results (durations, orphans, slow operations)

### Flow of Execution

1. User runs the application with a command
2. CLI arguments are parsed using `cli::cli_parse()`
3. Log files are parsed with `parser::parse_log_file()`
4. Command-specific processing:
   - **compare/diff**: Filters are applied, logs are compared with `comparator::compare_logs()`
   - **info**: Statistics and metadata are extracted and displayed
   - **llm/process**: Logs are sanitized and output as compact JSON
   - **perf**: Operation timings are analyzed and bottlenecks identified

## Development Notes

- The codebase uses Rust 2024 edition
- Dependencies include clap (for CLI), serde (for JSON), and chrono (for timestamps)
- Tests use the tempfile crate for testing file output
- JSON comparison is semantic, not just a string comparison
- The code handles complex log structures, including nested JSON objects and arrays