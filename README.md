# Log Analyzer

A command-line tool for analyzing and comparing JSON logs from the Applitools testing framework.

## Overview

This tool helps you extract, filter, and compare structured log data from Applitools log files. It's especially useful for:

- Finding differences between test runs
- Debugging test failures
- Analyzing log patterns

## Installation

### Quick Install (macOS/Linux)

```bash
# Clone the repository
git clone https://github.com/eirenik0/log-analyzer.git
cd log-analyzer

# Run the install script
./install.sh
```

This will install the `log-analyzer` command to your PATH, allowing you to run it from anywhere.

### From Source

```bash
# Clone the repository
git clone https://github.com/eirenik0/log-analyzer.git
cd log-analyzer

# Build the project
cargo build --release

# The binary will be available at target/release/log-analyzer
```

### Cross-Platform Builds

To build for multiple platforms:

```bash
# Run the build script
./build.sh
```

This creates binaries for Linux and Windows in the `builds` directory.

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

### Global Options

The following options can be used with any command:

| Option | Description |
|--------|-------------|
| `-F, --format <format>` | Output format: `text` (default) or `json` |
| `-c, --compact` | Use compact mode for output (shorter keys, optimized structure) |
| `-o, --output <path>` | Path to output file for the results |

### Compare Options

The `compare` command supports several options to fine-tune your analysis:

| Option | Description |
|--------|-------------|
| `-C, --component <name>` | Filter logs by component (e.g., "core-universal", "socket") |
| `-l, --level <level>` | Filter logs by log level (e.g., "INFO", "ERROR") |
| `-t, --contains <text>` | Filter logs containing specific text |
| `-d, --direction <direction>` | Filter logs by communication direction (Incoming/Outgoing) |
| `-D, --diff-only` | Show only differences, skip matching objects |
| `-f, --full` | Show full JSON objects, not just the differences |

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

## Features

- **Structured log parsing**: Automatically extracts and parses JSON payloads
- **Intelligent comparison**: Semantically compares JSON objects, regardless of property order
- **Filtering capabilities**: Filter logs by component, level, content, or direction
- **Log categorization**: Automatically categorizes logs into events, commands, and requests
- **Multiple output formats**: Choose between human-readable text or JSON for programmatic processing
- **Detailed output**: Clear, readable comparison output with highlighting for differences
- **Flexible output options**: Save results to a file, use compact mode for more concise output
- **Optimized CLI**: Well-organized command structure with global and command-specific options

## License

[MIT](LICENSE)