# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Log-analyzer is a Rust command-line tool for analyzing and comparing JSON logs. The parser/comparator core is generic, while domain-specific parsing/perf rules can be injected via TOML profiles.

## Claude Code Skill

This project includes a Claude Code skill for interactive log analysis.

### Installation

**Option 1: Plugin Installation (Recommended for use in other projects)**

Install the skill globally using the plugin system:

```bash
/plugin marketplace add https://github.com/eirenik0/log-analyzer
/plugin install log-analyzer
```

**Option 2: Project-Level (Automatic when cloning this repo)**

The skill is automatically available when working in this repository. It's defined in `.claude/skills/analyze-logs/`.

### Usage

Use the skill in Claude Code with:

```
/analyze-logs diff file1.log file2.log     # Compare two logs
/analyze-logs perf test.log                 # Performance analysis
/analyze-logs info test.log --samples       # Log structure overview
```

## Commands

### Building the Project

```bash
# Build the project
cargo build

# Run in development mode
cargo run -- <arguments>

# Build for release
cargo build --release

# Install locally
cargo install --path .
```

### Installation from Release

```bash
# Auto-detect platform and install latest release
./scripts/install.sh

# Or manually download from GitHub Releases
curl -LO https://github.com/eirenik0/log-analyzer/releases/latest/download/log-analyzer-VERSION-TARGET.tar.gz
```

### Running the Application

```bash
# Compare two log files
cargo run -- compare <file1> <file2> [options]

# Show only differences between log files (shortcut for compare --diff-only)
cargo run -- diff <file1> <file2> [options]

# Display information about one or more related log files
cargo run -- info <file> [file...] [options]

# Structured grep-style search in one log file
cargo run -- search <file> [options]

# Diagnose clustered errors/warnings across one or more related logs
cargo run -- errors <file> [file...] [options]

# Extract and aggregate a payload/settings field from matching entries
cargo run -- extract <file> --field <path> [options]

# Generate LLM-friendly compact JSON output from a single log file (canonical: process; alias: llm)
cargo run -- process <file> [options]

# Generate LLM-friendly compact JSON output of differences
cargo run -- llm-diff <file1> <file2> [options]

# Analyze operation timing and identify performance bottlenecks across one or more related logs
cargo run -- perf <file> [file...] [options]

# Trace a single operation/session across one or more related logs
cargo run -- trace <file> [file...] (--id <substring> | --session <substring>) [options]

# Generate a profile TOML by analyzing one or more related log files
cargo run -- generate-config <file> [file...] [options]

# Use an embedded template by name (no local file path needed)
cargo run -- generate-config <file> --template service-api --profile-name my-profile
```

### Global Options

These options can be used with any command:
- `-F, --format <text|json>` - Output format (default: text)
- `-j, --json` - JSON output shorthand for `-F json -c`
- `-c, --compact` - Use compact mode for output (shorter keys, optimized structure)
- `-f, --filter <expr>` - Filter expression (see Filter Syntax below)
- `-o, --output <path>` - Path to output file for results
- `--config <path>` - Analyzer profile config path (TOML)
- `--color <auto|always|never>` - Control color output (default: auto)
- `-v, --verbose` - Increase verbosity level (can be used multiple times)
- `-q, --quiet` - Be quiet, show only errors

### Filter Expression Syntax

The `-f, --filter` option accepts a unified filter expression:

```
--filter "type:value [!type:value] ..."
```

**Filter types (with aliases):**
- `component`, `comp`, `c` - Filter by component name
- `level`, `lvl`, `l` - Filter by log level (INFO, ERROR, etc.)
- `text`, `t` - Filter by text in message
- `direction`, `dir`, `d` - Filter by direction (incoming/outgoing)

**Prefix with `!` to exclude.** Examples:
```bash
# Only core-universal component
--filter "c:core-universal"

# Only ERROR level logs
--filter "l:ERROR"

# Core component, exclude DEBUG level
--filter "c:core !l:DEBUG"

# Contains 'timeout', incoming direction only
--filter "t:timeout d:incoming"

# Different filter types combine with AND;
# multiple values of the same type combine with OR
--filter "c:core-requests l:INFO !t:health"
# equivalent OR-within-type example:
--filter "c:core-requests c:socket l:ERROR"
```

### Compare Command (alias: `cmp`)

Compare two log files and show differences between JSON objects.

Options:
- `-D, --diff-only` - Show only differences between logs
- `--full` - Show full JSON objects, not just differences
- `-s, --sort-by <field>` - Sort output by field (time, component, level, type, diff-count)

Comparison output now also surfaces source line numbers for each paired entry, marks JSON changes as added/removed/modified, and preserves unpaired repeated entries in unique sections.

### Diff Command

Shortcut for `compare --diff-only`. Same options as Compare command (except `--diff-only`).

### Info Command (aliases: `i`, `inspect`)

List all components, event types, log levels, and detailed statistics in a log file.

Options:
- `-s, --samples` - Show sample log messages for each component
- `--json-schema` - Display detailed JSON schema information for event payloads
- `-p, --payloads` - Show payload statistics for each event/command/request type
- `-t, --timeline` - Show detailed timeline analysis with event distribution

### Search Command

Search a log file and print matching entries (structured grep replacement).

Options:
- `--context <number>` - Show N matching context entries before/after each match (default: 0)
- `--payloads` - Show parsed payload/settings JSON for each displayed entry
- `--count-by <matches|component|level|type|payload>` - Count matches grouped by a structured field instead of printing entries

### Errors Command

Diagnose clustered errors/warnings and affected sessions across one or more logs.

Options:
- `--top-n <number>` - Number of clusters to show (default: 10, `0` = all)
- `--warn` - Include WARN entries (default: ERROR only)
- `--sessions` - Show affected sessions for each cluster (cross-reference by `component_id`)
- `-s, --sort-by <count|time|impact>` - Sort clusters by field (default: `count`)

### Extract Command

Extract and aggregate a JSON payload/settings field from matching log entries.

Options:
- `--field <path>` - Field name/path to extract from payload JSON (supports dot paths like `settings.retryTimeout`)

### Trace Command

Trace a single operation lifecycle by correlation/request ID or session path across one or more log files.

Options:
- `--id <substring>` - Correlation/request ID substring to trace (matches raw log lines)
- `--session <substring>` - `component_id`/session path substring to trace (matches hierarchy)

### Process Command (alias: `llm`)

Generate LLM-friendly compact JSON output of a single log file with sanitized content.

Options:
- `-s, --sort-by <field>` - Sort output by field (time, component, level, type, diff-count)
- `--limit <number>` - Maximum number of log entries to include (default: 100, 0 = unlimited)
- `--no-sanitize` - Disable hiding of sensitive fields from JSON payloads (sanitization is enabled by default)

### LLM-Diff Command

Generate LLM-friendly compact JSON output of differences (shortcut for `compare --diff-only -F json -c`).

Options:
- `-s, --sort-by <field>` - Sort output by field (time, component, level, type, diff-count)
- `--no-sanitize` - Disable sanitization of sensitive fields

### Perf Command

Analyze operation timing and identify performance bottlenecks.

Options:
- `--threshold-ms <ms>` - Duration threshold in milliseconds for highlighting slow operations (default: 1000)
- `--top-n <number>` - Number of slowest operations to display (default: 20)
- `--orphans-only` - Show only orphan operations (started but never finished)
- `--op-type <request|event|command>` - Filter by operation type
- `-s, --sort-by <field>` - Sort results by field (duration, count, name)

### Generate-Config Command (alias: `gen-config`)

Analyze one or more log files and generate a TOML profile.

Options:
- `--profile-name <name>` - Name for the generated profile (defaults to input filename stem for one file, otherwise `generated-profile`)
- `--template <path-or-name>` - Base template path or built-in name (`base`, `custom-start`, `service-api`, `event-pipeline`)

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
   - Tracks source line numbers for each parsed entry

2. **Config (`src/config.rs`)** - Loads analyzer profiles and built-in templates
   - Loads `--config` TOML from disk
   - Provides embedded defaults/templates when no external file is used
   - Produces profile insights (unknown components/commands/requests, session prefixes)

3. **Comparator (`src/comparator.rs`)** - Compares logs and identifies differences
   - Compares JSON payloads while respecting object structure
   - Finds differences regardless of object property order
   - Classifies changes as Added/Removed/Modified
   - Generates human-readable difference reports with styled tables

4. **Filter (`src/filter/`)** - Unified filter expression parsing
   - Parses filter expressions like `"c:core l:ERROR !t:timeout"`
   - Supports include/exclude semantics with `!` prefix
   - Converts expressions to LogFilter for log matching

5. **CLI (`src/cli.rs`)** - Command-line interface using Clap
   - Defines the CLI commands and arguments
   - Handles parameter parsing and validation

6. **LLM Processor (`src/llm_processor.rs`)** - Generates LLM-friendly output
   - Sanitizes sensitive data from log payloads
   - Produces compact JSON output optimized for LLM consumption

7. **Performance Analyzer (`src/perf_analyzer/`)** - Analyzes operation timing
   - Tracks operation durations (requests, events, commands)
   - Identifies orphan operations (started but never finished)
   - Calculates performance statistics and identifies bottlenecks

8. **Library (`src/lib.rs`)** - Core library exposing public API
   - Orchestrates command execution
   - Provides filtering and output formatting

9. **Config Generator (`src/config_generator.rs`)** - Builds profile TOML data from parsed logs
   - Collects known components, commands, and requests
   - Detects high-frequency session prefixes from component IDs
   - Preserves parser/perf rules from a base config

### Key Data Types

- `LogEntry` - Represents a single log entry with all metadata
- `LogEntryKind` - Enum for different types of logs (Generic, Event, Command, Request)
- `ComparisonResults` - Contains comparison results between two log files
- `AnalyzerConfig` - Runtime parser/perf/profile rules loaded from TOML or built-ins
- `FilterExpression` - Parsed unified filter AST for include/exclude matching
- `LogFilter` - Used to filter logs by component, level, text, or direction
- `PerfStats` - Contains performance analysis results (durations, orphans, slow operations)

### Flow of Execution

1. User runs the application with a command
2. CLI arguments are parsed using `cli::cli_parse()`
3. Analyzer profile config is loaded with `config::load_config()`
4. Log files are parsed with `parser::parse_log_file_with_config()`
5. Command-specific processing:
   - **compare/diff**: Filters are applied, logs are compared with `comparator::compare_logs()`
   - **info**: Statistics and metadata are extracted and displayed
   - **llm/process**: Logs are sanitized and output as compact JSON
   - **perf**: Operation timings are analyzed and bottlenecks identified
   - **generate-config**: Profile hints are generated from parsed logs and output as TOML

## Development Notes

- The codebase uses Rust 2024 edition
- Dependencies include clap (CLI), serde (JSON), chrono (timestamps), comfy-table (tables), and thiserror (error types)
- Tests use the tempfile crate for testing file output
- JSON comparison is semantic, not just a string comparison
- The code handles complex log structures, including nested JSON objects and arrays

## Change Tracking with Knope

This project uses [Knope](https://knope.tech) for automated changelog generation and release management. Knope tracks changes via conventional commits and changeset files.

### Conventional Commits

Use conventional commit messages for automatic changelog generation:

```bash
# Feature (minor version bump)
git commit -m "feat: add new parsing capability"

# Bug fix (patch version bump)
git commit -m "fix: correct off-by-one error"

# Breaking change (major version bump)
git commit -m "feat!: redesign API interface"

# With scope
git commit -m "feat(parser): improve JSON detection"
```

### Changeset Files

For detailed change documentation, use the interactive command:

```bash
knope document-change
```

This prompts for version bump type (`patch`/`minor`/`major`) and change description, then creates a properly formatted file in `.changeset/`.

**Format notes** (if editing manually):
- Frontmatter: `default: patch` (or `minor`/`major`)
- Body: Plain text or simple bullet list (no headers like `### Fixed`)
- Body content is appended directly to CHANGELOG.md

### Local Knope Commands

```bash
# Install Knope
cargo install knope

# Preview what the next release would look like
knope prepare-release --dry-run

# Get current version
knope get-version
```

## Release Process

Releases are automated via GitHub Actions using Knope. To create a new release:

1. **Make changes** using conventional commits (feat:, fix:, etc.)
2. **Trigger release** from GitHub Actions → Release → Run workflow
3. **GitHub Actions will**:
   - Run `knope prepare-release` to update CHANGELOG.md and bump version
   - Run tests on all platforms
   - Build binaries for Linux (x86_64, aarch64, musl), macOS (x86_64, arm64), Windows
   - Run `knope release` to create a GitHub Release with changelog
   - Upload all binaries and checksums

### Dry Run

Test the release process without creating a release:
- Go to GitHub Actions → Release → Run workflow
- Check "Run without creating release (for testing)"

### Requirements

The release workflow requires a `PAT` secret (Personal Access Token) with write permissions to push version bumps back to the repository.

### Supported Platforms

| Platform | Target | Archive |
|----------|--------|---------|
| Linux x86_64 | x86_64-unknown-linux-gnu | tar.gz |
| Linux x86_64 (musl) | x86_64-unknown-linux-musl | tar.gz |
| Linux ARM64 | aarch64-unknown-linux-gnu | tar.gz |
| macOS Intel | x86_64-apple-darwin | tar.gz |
| macOS Apple Silicon | aarch64-apple-darwin | tar.gz |
| Windows x86_64 | x86_64-pc-windows-msvc | zip |
