# Changelog

## 0.1.2 (2026-01-22)

### Features

- Add filter for connection direction
- Add advanced filtering, sorting, and CLI enhancements
- Enhance `info` command with detailed analysis options
- Add individual llm log preparation
- Sanitize by default
- Improve request parsing for name, ID, and direction detection
- Add performance analysis command to CLI
- Add installation scripts and Claude Code skill for log analysis
- Improve install script for user-friendliness and compatibility
- Add plugin support for Claude Code and update documentation

#### Add Claude Code plugin support for cross-project skill installation:

- Add `.claude-plugin/plugin.json` manifest to enable plugin distribution
- Add `.claude-plugin/marketplace.json` for plugin marketplace discovery
- Create `skills/` symlink to support both plugin and project-level usage
- Users can install with `/plugin marketplace add` then `/plugin install log-analyzer`
- Update documentation with plugin installation instructions in README.md and CLAUDE.md

### Fixes

#### Improve installation workflow and documentation:

- Fix `scripts/install-skill.sh` to use repository directory instead of current working directory
- Change default install location from `/usr/local/bin` to `$HOME/bin` (no sudo required)
- Add automatic PATH setup instructions for zsh, bash, and fish shells
- Recommend WSL for Windows users instead of native binary
- Rewrite README.md to be more compact and user-friendly (~50% smaller)
- Add Claude Code Integration section with `/analyze-logs` skill examples

## 0.1.1 (2026-01-21)

### Features

- Add filter for connection direction
- Add advanced filtering, sorting, and CLI enhancements
- Enhance `info` command with detailed analysis options
- Add individual llm log preparation
- Sanitize by default
- Improve request parsing for name, ID, and direction detection
- Add performance analysis command to CLI
- Add installation scripts and Claude Code skill for log analysis

## 0.1.0

### Features

- Initial release of log-analyzer
- Compare two log files and show differences between JSON objects
- Display information about log files (components, event types, log levels)
- Generate LLM-friendly compact JSON output with sanitization
- Performance analysis for operation timing and bottleneck identification
- Support for filtering by component, level, text, and direction
- Multiple output formats (text, JSON) with color support
