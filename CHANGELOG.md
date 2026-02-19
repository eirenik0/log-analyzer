# Changelog

## 0.1.3 (2026-02-19)

### Features

- add source line tracking for log parsing and validation
- add unified filter module with expression-based log filtering
- enhance comparison output with table formatting and JSON shorthand support
- introduce CLI enhancements and output improvements
- add support for config file via `--config` flag in CLI
- improve filtering logic
- add `generate-config` command with embedded templates for profile generation
- ensure unique/unpaired entries are included in JSON and text diff outputs

#### Improve CLI usability with new global flags:

- Added `-j, --json` as shorthand for `-F json -c` for compact machine-readable output.
- Added `-f, --filter` for unified filter expressions (for example: `c:core l:ERROR !t:timeout`).
- `-f, --filter` can also be set with `LOG_ANALYZER_FILTER`.
- `--json` conflicts with explicit `-F, --format` to avoid ambiguous output settings.

#### Add configurable profiles and starter templates for custom log formats:

- Added runtime profile loading via `--config <path>` or `LOG_ANALYZER_CONFIG`.
- Parser markers, pairing markers, and correlation keys are now configurable through profiles.
- Added profile-aware `info` insights for unknown components, commands, requests, and session prefixes.
- Added reusable templates: `base`, `custom-start`, `service-api`, and `event-pipeline`.

#### Improve `diff` output readability and diagnostics:

- Differences are now classified as added, removed, or modified (`+`, `-`, `~`).
- Summary output now includes counts of additions, removals, and modifications.
- Diff entries now include source line numbers for faster navigation to original logs.
- JSON diff output now includes `change_type`, and text diffs are split into `text1`/`text2`.

#### Add profile generation command and built-in template support:

- Added `generate-config` (`gen-config`) to create a TOML profile from a log file.
- Generated profiles include discovered components, commands, requests, and session prefix hints.
- Supports `--profile-name`, `--template`, and `-o, --output`.
- `--template` now accepts either a file path or built-in template name (`base`, `custom-start`, `service-api`, `event-pipeline`).
- Embedded built-in templates into the binary and use embedded `base` as default for better portability.

#### Add unified filter expression syntax:

- Added expression-based filtering via `-f, --filter`.
- Supports `component`, `level`, `text`, and `direction` terms (with short aliases like `c:`, `l:`, `t:`, `d:`).
- Supports exclusions with `!` (for example: `!l:DEBUG`).
- Multiple terms are combined with AND semantics.
- Matching for `level` and `direction` values is case-insensitive.
- Unknown filter values now produce warnings to catch typos.

### Fixes

#### Fix regressions in compare/diff filtering and output:

- Repeated shared keys are now paired one-to-one; unmatched occurrences are kept as unique entries.
- `--sort-by time/component/level/type` now sorts correctly.
- `--full` now prints full payload JSON in comparison output.
- `-o, --output` now writes the correct output for `compare`, `diff`, `llm-diff`, `process`, and `perf` (text and JSON).
- Filter logic is now consistent: different filter types are AND-ed, multiple values of the same type are OR-ed.
- `diff` output now includes unpaired unique entries in both text and JSON modes.
- Parser no longer panics when profile config has empty `command_payload_markers`.

#### Improve output formatting for summaries:

- Summary statistics now render as styled, width-aware tables.
- Table formatting is applied consistently across console and file output.
- Improves readability of command output for large result sets.

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
