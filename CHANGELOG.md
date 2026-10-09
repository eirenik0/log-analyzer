# Changelog

## Unreleased

## 0.3.0 (2026-10-09)

### Breaking Changes

- integrate explicit lifecycle evidence (#38)
- unify lifecycle classification and coverage (#39)

### Features

- add parser auto-detection for Rust tracing, syslog, and JSON lines
- add `--preset` option for built-in profiles and integrate `eyes` preset across codebase
- add configurable event timing boundaries (#25)
- normalize structured log exports explicitly (#26)
- retain correlated fields and source provenance (#27)
- apply consistent optional redaction (#28)
- expose build identity and capabilities (#29)
- add deterministic event rule module (#37)
- support profile inheritance (#40)
- support starts with no expected end (#42)
- define snapshot-scoped evidence contract
- add bounded evidence retrieval (#53)
- validate profile suitability
- add evidence-backed investigation workflows
- Fix #3: add bounded errors reports with configurable sample-character, stack-frame, and total text-output limits. Keep parse coverage, scope, counts and impact ahead of examples; share detail space across clusters and report explicit omissions. Preserve full output with --complete and keep JSON analysis totals independent of displayed details.
- Expose source revision/state, JSON capabilities, and build/profile metadata in reports. Validate maintained examples against native release binaries.
- Add an opt-in, versioned event-rule configuration and deterministic classification module with whole-message regex and typed structured adapters. Compile rules once at profile loading, retain identity and provenance, and report conflicts, invalid data and unclassified records explicitly. Reject unsupported schemas, invalid mappings and mixed legacy lifecycle markers. Production analysis remains unchanged until command/request integration; document the contract and custom-profile migration policy.
- Add configurable event rules and explicit measured, inferred-sleep, or unknown timing pairs to perf and trace. Preserve source evidence, ambiguous/incomplete groups, sample counts, gaps, and capture windows while separating measured work from elapsed time.
- Add correlated multi-field extraction rows, explicit single-array expansion, missing/null distinctions, and source provenance while preserving single-field aggregates.
- Make the embedded `base` profile genuinely generic and add an explicit built-in `eyes` preset for the current specialized log grammar. The CLI now supports `--preset` for selecting built-in profiles without a repo-local config file, and the docs/skill examples now show pinning a preset or config explicitly in repo workflows.
- Prepare 0.3.0 for the evidence-engine CLI and maintained skill, synchronizing binary/plugin versions and enabling publication of a reviewed prepared version without a second bump.
- Add top-level `extends` to TOML profiles for inheriting built-in profiles or files relative to the child. Tables merge key by key with the child winning; arrays and scalars replace inherited values. Reject cycles, unreadable parents, invalid references, and chains exceeding eight profiles, including the root and any built-in parent. Document inheritance in `--config` help as well as the README and analysis skill.
- Add parser auto-detection for Rust tracing, syslog, and JSON lines, plus structured field extraction for tracing-style `key=value` logs. Structured fields now participate in filtering, extraction, trace matching, and generated parser profiles.
- Add `end_expected = false` to version-2 event-rule mappings for start records that never get an end record. `perf` counts them as `start_only_events` instead of `missing_end` orphans, without measuring durations, and session levels can create and complete on them. Operation-type filters count excluded start-only records as suppressed while preserving relevant-event totals. The option is rejected in version 1 and on rules without a start phase. Document the behavior in CLI help and the analysis skill.
- Add explicit structured-export row expansion, JSON Pointer field mapping, integer timestamp units, and JSON-string decoding with row provenance and skipped-row diagnostics. Add a bounded schema preview command.

#### Add common compact-JSON byte, Unicode-character and item budgets with deterministic

snapshot/profile/query/redaction-bound cursors. Preserve full-scope coverage and
totals, report oversized metadata/items explicitly, and provide complete-output
and canonical source-record retrieval. Keep legacy bounded-error behavior and
add deterministic comparison/source ordering plus synthetic resource measurements.

#### BREAKING CHANGE (pre-1.0): shipped command profiles now use explicit whole-message grammar instead of incidental substring matches. Migrate custom formats by authoring command_rules and removing legacy command identity/phase fields; legacy-only custom marker grammar is retained without automatic translation. LogEntry now exposes cached classification evidence; caller-constructed command records must provide evidence (or explicitly attach legacy compatibility evidence) for timing analysis. All command pairing requires nonempty correlation scope.

Recognize independent command start/completion records across shipped profiles, fixing the service-api 1500 ms reproduction. Cache phases before payload cleanup and consume normalized evidence in the existing correlation engine. Report start-only, end-only, identity-only, conflicting, invalid and unknown evidence honestly. Preserve offsets, scoped ambiguity safeguards and text/JSON selection. Explicit session completion requires a nonfailed end; command names/starts alone do not complete sessions. Bound new command payload decoding with multi-pattern scanning and retain malformed/deep payloads or trailing noncomment content without changing legacy payload parsing. Synchronize profile/skill templates, migration documentation, generated-profile mode and CLI help.

#### Deliver a portable evidence-backed investigation loop and aligned Claude skill,

with capability/coverage/profile preflight, scoped citation retrieval, distinct
findings, and explicit insufficiency/budget stops. Extend the maintained example
runner with synthetic multi-step failure, INFO-only comparison, reused-identity,
truncated/unsupported input, instruction-like context and budget cases.

#### Add sample-scoped profile validation with separate parsing/classification/timing

coverage, rule and source witnesses, identity/scope diagnostics, and strict
positive/negative expected facts and exact pair assertions. Validate inherited or
generated TOML candidates without activation, with shared redaction and retrieval.

#### Add shared presentation redaction for text/JSON reports and files, nested fields and encoded queries, stable optional ID masking, and explicit redaction metadata. Preserve long correlation IDs in compact payloads.

Keep numeric masks out of report metadata, truncate expanded messages on UTF-8 boundaries, and index identifier matching without rescanning the full ID set per entry.

#### Complete shared command/request/event classification and correlation. Shipped profiles and synchronized skill templates now use version-2 global event rules with explicit direction/phase, endpoint and ordered correlation-field mappings; version-1 and legacy custom contracts remain supported. Cache normalized IDs/scopes before analysis and retain pair/orphan provenance. Expose classification coverage independent of operation-type/display selection and parse coverage, with consistent text/JSON redaction and capability metadata. Preserve JSON5 string identities during undefined-value compatibility conversion, support unknown-direction filters, and preserve generic extraction and existing scoped ambiguity/timestamp/ID-reuse safeguards.

Pre-1.0 breaking behavior/API: shipped request/event recognition now follows documented exact grammar; manually constructed operations require cached explicit or legacy evidence, and missing direction remains Unknown. Global rules cannot mix legacy lifecycle markers. Additive report fields retain schema version 1; the former command unknown counter remains a deprecated compatibility alias. See README and docs/design/event-classification.md for migration.

#### Publish CLI report, capabilities and investigation-result JSON Schemas. Add

snapshot-scoped evidence references and effective profile/query identities to
existing report metadata, preserving source locations across selection and
presentation. Add comparison and error estimate boundaries and schema conformance
regressions for redaction, nested rows, partial input and ambiguous timing.

Embed schemas in capability discovery for installed clients. Preserve non-UTF-8
input path identity and exclude output destinations from query serialization.

Emit coverage-only JSON on unparsed-input failures across report commands,
including full mixed-input scope and saved-output parity.

### Fixes

- report nonempty unparsed input and parse coverage (#4)
- parse browser-console prefixes on classic logs (#5)
- bound error reports while preserving coverage and impact summaries (#7)
- sort entries before applying limits (#21)
- preserve complete UTC timestamps (#22)
- apply selection consistently across output formats (#23)
- scope correlation and preserve ambiguous events (#24)
- preserve graphemes and distinct field names (#30)
- report operation evidence and capture limits (#31)
- correlate long scopes without summary impersonation (#41)
- recognize real SDK lifecycle grammar (#43)
- Fix #2: recognize browser-console source prefixes on classic log entries and multiline continuations. Preserve original raw text and physical source lines, retain locations in the console_source structured field, and support filename/path/URL locations with line and optional column numbers.
- Fix Clippy warnings on Rust 1.99 by using key-based sorting, guarded request matches, and direct test configuration initialization. Preserve existing ordering and request filtering behavior.
- Document contribution and code review rules, and configure the standard pre-commit hook for Conventional Commit messages.
- Correlate record-field scope values longer than 4096 bytes instead of reporting them as `missing_scope_field`. Such a value becomes a readable prefix plus its byte length and a 128-bit FNV-1a digest, so equal long scopes pair. Encode short values containing the reserved digest marker as well, preventing raw summaries from impersonating long scopes. Explicit event-rule scope mappings use the same reserved-marker encoding after validation and retain their existing input limit. Document the bounded keys and digest limitations in the README, CLI help, and analysis skill.
- Report performance operation evidence separately from parse coverage, including pairing diagnostics, suppressions, observed capture limits, and unknown export completeness. Reject inferred-year measurements and ambiguous equal-timestamp ordering; preserve analytic metadata under redaction.
- Fix #1: reject nonempty input with no recognized entries (exit 1) and expose per-file parser/profile, byte size, parsed entries, and rejected candidates in info/errors/perf text and JSON reports. Distinguish empty input and zero filter matches from parsing failure, without counting multiline continuations as rejections.
- Apply performance sorting, per-section limits, and orphan-only selection consistently to text and JSON. Preserve full totals and omitted counts, include threshold violations, and reject completed-operation options in orphan-only mode.
- Fix process sorting before entry limits, reject diff-only sorting, and calculate returned timestamp bounds independently of display order.
- Fix process timestamps to emit complete UTC RFC 3339 dates and times rather than labeling local time as UTC or dropping entry dates and offsets.
- Restore the tracked built-in `eyes` preset so clean clones and CI can compile the embedded config and use `--preset eyes` without a missing file error.
- Scope operation correlation by kind, name, ID, and configurable composite context fields. Preserve ambiguous and unmatched events with source provenance without inventing durations, and retain valid cross-file matching.
- Preserve complete Unicode graphemes when shortening report text, and prevent compacted field names or omission metadata from overwriting retained values.

#### Log Analyzer helps AI agents investigate failures and performance problems using

compact, verifiable evidence from logs. Document the local Rust evidence engine,
verified failure/slow-run/lifecycle workflows, profile validation, external-agent
data handling and published synthetic evaluation limits; align skill/plugin copy.

#### Restrict Eyes SDK request completion suffixes to recognized response and retry forms so pending prose cannot create measured operations. Decode default-driver command settings using the same payload rules as regular settings. Keep the analysis skill template synchronized and cover both cases with synthetic regression tests.

Validate standalone response body container boundaries with an opt-in version-2 text capture guard. Reject pending prose after object/array bodies, including prose followed by another container, while preserving nested JSON5 bodies and explicit SDK retry tails.

Recognize LF, CR, and Unicode line/paragraph separators as JSON5 line-comment terminators in body scanning, trailing-comment validation, and undefined-value normalization so comments cannot hide pending prose.

#### Integrate the synthetic CLI corpus and executable typed investigation scorer with

exact source support, abstention/cause/omission checks, bounded paired analyzer and
search-script runs, optional trusted model adapters and sanitized baseline results.

## 0.2.0 (2026-02-25)

### Breaking Changes

#### Remove legacy `[profile.session_prefixes]` configuration in favor of `[[sessions.levels]]` only.

- `AnalyzerConfig` session insights now read only from `sessions.levels`.
- `generate-config` no longer writes `profile.session_prefixes` and uses `level-1`, `level-2`, ... for inferred generic level names.
- Built-in templates, README examples, and Claude skill templates/docs now use `[[sessions.levels]]` exclusively.

### Features

- add multi-file support for `info` and `perf` commands
- add `trace` command to track operation/session lifecycle across log files
- add `search` command for structured log inspection with filtering, context, and grouped counting
- add `extract` command for aggregating JSON field values from matching log entries
- add session lifecycle insights in profiles and `info` command outputs
- allow `generate-config` to process multiple log files and merge entries for profile inference
- add `errors` command for clustering log patterns and session analysis

#### Add an `errors` command for single-command failure diagnosis across one or more log files.

- Clusters ERROR entries (and optionally WARN entries via `--warn`) by normalized message pattern.
- Shows per-cluster severity, counts, emitting components, first/last timestamps, and a sample message.
- Optionally cross-references affected `component_id` sessions via `--sessions`, including `completed` vs `orphaned` outcomes using perf-style orphan detection heuristics.
- Adds impact-oriented cluster sorting (`--sort-by impact`) plus blocking-span estimates in the summary.

#### Add an `extract` command for aggregating payload field values from matching log entries.

- `log-analyzer extract <file> --field <name>` extracts a JSON payload/settings field and groups by value occurrences.
- Works with the existing global `-f/--filter` expression syntax to scope extraction to specific messages/components.
- Supports JSON output via global `-F json` / `-j` and dot-path field access (for example `settings.retryTimeout`).

#### Add multi-file input support to `generate-config`.

- `log-analyzer generate-config` now accepts one or more log files and merges them before inferring profile hints.
- This improves profile generation for split/rotated logs from the same run by combining observed components, commands, requests, and session prefixes.
- Generated output now includes a multi-source header when multiple files are provided.

#### Add multi-file input support for `info` and `perf` commands.

- `log-analyzer info` now accepts one or more log files and aggregates analysis across all inputs.
- `log-analyzer perf` now accepts one or more log files and analyzes them as a single timeline.
- Parsed entries from all provided files are concatenated and sorted by timestamp before analysis, which improves cross-file operation pairing (including orphan detection).

#### Add a `search` command for structured grep-style log inspection.

- `log-analyzer search <file>` prints matching log entries using the existing `-f/--filter` expression syntax.
- Supports entry-based context windows via `--context <n>` and optional parsed payload display with `--payloads`.
- Supports grouped counting mode via `--count-by <matches|component|level|type|payload>` (including payload-based occurrence grouping).

#### Add profile-driven hierarchical session insights for `info` using a new optional `[[sessions.levels]]` config format.

- Supports named session levels with `segment_prefix`, `create_command`, `complete_commands`, and `summary_fields`.
- Upgrades profile analysis to build per-session lifecycle state (created/completed), parent-child links, operation counts, and create-time summary field extraction in a single pass.
- `info` now renders per-level session completion health summaries (completed vs incomplete) and stable configured summary field values when available.
- `generate-config` now emits detected session prefixes as generic `[[sessions.levels]]` entries (`level-1`, `level-2`, ...) while preserving template-defined session levels.

#### Add a `trace` command for following a single operation/session lifecycle across log files.

- `log-analyzer trace` accepts one or more log files and merges/sorts entries by timestamp.
- Supports `--id <substring>` to trace by correlation/request ID fragments and `--session <substring>` to trace by `component_id` hierarchy.
- Text output shows chronological entries with per-step timing deltas; JSON output is also available via global `-F json` / `-j`.

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
