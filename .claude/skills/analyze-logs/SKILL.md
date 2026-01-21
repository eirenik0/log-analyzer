---
name: analyze-logs
description: Analyze, compare, and debug Applitools test logs. Use when comparing log files, finding test failures, identifying performance bottlenecks, or preparing logs for analysis.
argument-hint: <command> [files...] [options]
allowed-tools: Bash(cargo run:*), Bash(./target/release/log-analyzer:*), Bash(log-analyzer:*), Read, Glob, Grep
context: fork
---

# Log Analyzer Skill

Analyze JSON logs from Applitools testing framework using the log-analyzer CLI tool.

## Commands Overview

| Command | Purpose | Example |
|---------|---------|---------|
| `diff` | Show differences between two log files | `/analyze-logs diff file1.log file2.log` |
| `compare` | Full comparison with all matches | `/analyze-logs compare file1.log file2.log` |
| `info` | Analyze structure of a single log | `/analyze-logs info test.log --samples` |
| `perf` | Find performance bottlenecks | `/analyze-logs perf test.log --threshold-ms 500` |
| `llm` | Generate LLM-friendly output | `/analyze-logs llm test.log` |
| `llm-diff` | LLM-friendly diff output | `/analyze-logs llm-diff file1.log file2.log` |

See [reference.md](reference.md) for complete command documentation.

## Quick Start

First, ensure the tool is built:
```bash
cargo build --release
```

Then run analysis:
```bash
./target/release/log-analyzer <command> [options]
```

Or during development:
```bash
cargo run -- <command> [options]
```

## Task Instructions

When the user invokes this skill:

1. **Parse the request** to determine:
   - Which command is needed (diff, compare, info, perf, llm)
   - Which log files to analyze
   - Any filtering options (component, level, text)

2. **Find log files** if not specified:
   ```bash
   # Look for .log files in the project
   find . -name "*.log" -type f 2>/dev/null | head -10
   ```

3. **Build the command** with appropriate options:
   - For debugging failures: use `diff` with `--diff-only`
   - For performance issues: use `perf` with appropriate threshold
   - For understanding logs: use `info` with `--samples --payloads`

4. **Execute and interpret**:
   - Run the log-analyzer command
   - Summarize key findings in plain language
   - Highlight actionable items (errors, slow operations, differences)
   - Suggest next steps if issues are found

## Common Workflows

### Debug Test Failure
```bash
# Quick diff to see what changed
./target/release/log-analyzer diff passing.log failing.log

# Focus on errors only
./target/release/log-analyzer diff passing.log failing.log -l ERROR

# Focus on specific component
./target/release/log-analyzer diff passing.log failing.log -C core-universal
```

### Performance Investigation
```bash
# Find operations taking > 2 seconds
./target/release/log-analyzer perf test.log --threshold-ms 2000

# Find orphan operations (started but never finished)
./target/release/log-analyzer perf test.log --orphans-only

# Focus on requests only
./target/release/log-analyzer perf test.log --op-type Request --top-n 30
```

### Log Exploration
```bash
# Full overview with samples
./target/release/log-analyzer info test.log --samples --payloads --timeline

# JSON output for further processing
./target/release/log-analyzer info test.log -F json
```

### Prepare for LLM Analysis
```bash
# Sanitized, compact output
./target/release/log-analyzer llm test.log --limit 100 -o context.json

# Diff for LLM
./target/release/log-analyzer llm-diff file1.log file2.log -o diff.json
```

## Filtering Options

All commands support these filters:

**Include filters:**
- `-C, --component <name>` - Filter by component (socket, core-universal, etc.)
- `-l, --level <level>` - Filter by log level (ERROR, WARN, INFO, DEBUG)
- `-t, --contains <text>` - Filter by text content
- `-d, --direction <Incoming|Outgoing>` - Filter by direction

**Exclude filters:**
- `--exclude-component <name>`
- `--exclude-level <level>`
- `--exclude-text <text>`

## Output Formats

- `-F text` - Human-readable colored output (default)
- `-F json` - Structured JSON output
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
- **Components**: All log sources in the file
- **Event Types**: Categorized operations
- **Timeline**: Distribution of events over time
