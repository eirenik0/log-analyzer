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

### Compare Options

The `compare` command supports several options to fine-tune your analysis:

| Option | Description |
|--------|-------------|
| `--component <name>` | Filter logs by component (e.g., "core-universal", "socket") |
| `--level <level>` | Filter logs by log level (e.g., "INFO", "ERROR") |
| `--contains <text>` | Filter logs containing specific text |
| `--direction <direction>` | Filter logs by communication direction (Incoming/Outgoing) |
| `-d, --diff-only` | Show only differences, skip matching objects |
| `-o, --output <path>` | Path to output file for the differences |
| `-f, --full` | Show full JSON objects, not just the differences |

### Examples

**Compare logs from two test runs showing only differences:**
```bash
log-analyzer compare universal-1.log universal-2.log --diff-only
```

**Compare logs filtering by component and level:**
```bash
log-analyzer compare universal-1.log universal-2.log --component core-universal --level ERROR
```

**Save comparison results to a file:**
```bash
log-analyzer compare universal-1.log universal-2.log --output diff.log
```

**Filter logs by direction:**
```bash
log-analyzer compare universal-1.log universal-2.log --direction Outgoing
```

## Features

- **Structured log parsing**: Automatically extracts and parses JSON payloads
- **Intelligent comparison**: Semantically compares JSON objects, regardless of property order
- **Filtering capabilities**: Filter logs by component, level, content, or direction
- **Log categorization**: Automatically categorizes logs into events, commands, and requests
- **Detailed output**: Clear, readable comparison output with highlighting for differences
- **File output**: Option to save comparison results to a file

## License

[MIT](LICENSE)