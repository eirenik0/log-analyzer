# Changelog

User-visible changes, newest first. See the [README](README.md) for current usage
and [GitHub Releases](https://github.com/eirenik0/log-analyzer/releases) for published downloads.

## Unreleased

See [pending release notes](.changeset/) for changes awaiting the next version.

## 0.3.0 (2026-10-09)

Investigate failures and slow runs with verifiable source references, bounded
reports, and explicit limits on what the logs can establish.

### Upgrading

- **Eyes/Applitools logs:** select `--preset eyes`. The default `base` profile is
  now generic.
- **Custom profiles:** shipped profiles now use version-2 `event_rules` with
  whole-message recognition. Review the [migration guide](docs/design/event-classification.md);
  legacy-only profiles remain supported.
- **Rust integrations:** constructed operation records must include classification
  evidence. Timing requires valid, scoped start/end boundaries.

### Added

- An opt-in crates.io publishing workflow with release validation and package dry runs.
- Source citations and JSON schemas that let agents verify findings against the
  original records, with input, profile and query identities.
- Paged JSON reports with byte, character and item budgets, resumable cursors,
  and explicit omitted-detail counts. See [bounded output](README.md#bounded-investigation-output).
- `validate-profile` to check parsing, recognition and timing against sample logs
  and optional known facts before using a custom profile.
- Profile inheritance, configurable timing boundaries and intentional start-only
  events; structured-export normalization and correlated field extraction with
  source line/row references.
- Rust tracing, syslog and JSON-lines detection, plus browser-console prefixes
  on classic logs.
- Failure, slow-run and lifecycle investigation workflows with synthetic checks.
  [Evaluation results](evals/README.md) validate the scripted harness; model-quality
  improvements and token savings remain unmeasured.

### Fixed

- Standalone skill installation, plugin invocation and portable references; documented
  investigation examples are checked against executable workflow fixtures.
- Nonempty unparsed input now fails with coverage diagnostics. Empty selections,
  zero errors and unavailable analysis remain distinguishable.
- Correlation respects scope and reused IDs, preserves timestamp offsets, and
  reports missing, conflicting or ambiguous lifecycle evidence.
- Filters, sorting, limits and saved output agree across text and JSON reports.
- Unicode compaction is safe; optional redaction retains analytic counts and
  typed evidence metadata.

## 0.2.0 (2026-02-25)

Inspect related logs together and follow failures, fields and session activity.

### Upgrading

Replace `[profile.session_prefixes]` with `[[sessions.levels]]`. Generated profiles
now use generic level names such as `level-1`; template-defined levels are preserved.

### Added

- `info`, `perf` and `generate-config` accept multiple related files, including
  split or rotated logs from one run. `perf` can pair boundaries across files.
- `search` supports filters, surrounding-entry context, parsed payloads and grouped counts.
- `extract` aggregates payload/settings fields, including nested dot paths.
- `errors` groups ERROR messages and optional WARNs, with affected-session details,
  impact sorting and estimated blocking spans.
- `trace` follows ID or session-path substrings across files in chronological
  order, with per-step deltas. A substring match can include multiple lifecycles.
- Profile-defined session levels let `info` show completion summaries, hierarchy
  and configured create-time fields.

## 0.1.3 (2026-02-19)

### Added

- Custom TOML profiles through `--config`, embedded starter templates, and
  `generate-config` to create an editable profile from sample logs.
- Unified `--filter` expressions for component, level, text and direction,
  including exclusions. Different fields use AND; values of the same field use OR.
- `-j` as shorthand for compact JSON output (`-F json -c`).
- Clearer diffs with added/removed/modified labels, summary counts and source lines.

### Fixed

- Repeated shared keys pair one-to-one; unmatched entries remain visible in both
  text and JSON diffs.
- Sorting, full-payload output and saved reports work consistently across the
  comparison and processing commands.
- Empty `command_payload_markers` no longer cause a parser panic.
- Summary tables adapt to output width and work in saved reports.

## 0.1.2 (2026-01-22)

### Added

- Claude Code marketplace/plugin installation for use across projects.

### Fixed

- The CLI installer defaults to `~/bin`, avoiding a `sudo` requirement, and
  provides PATH setup guidance for zsh, Bash and fish.
- Installation documentation includes WSL guidance for Windows users.

## 0.1.1 (2026-01-21)

- Added installation scripts and the Claude Code analysis skill.
- Expanded filtering, sorting and `info` inspection options.
- Improved request name, ID and direction parsing.
- Added individual-log preparation with default sanitization and the `perf`
  command for operation timing analysis.

## 0.1.0

Initial release: compare JSON payloads across logs, inspect components and log
levels, prepare compact sanitized output, analyze operation timing, and filter
results by component, level, text or direction. Supports text and JSON output.
