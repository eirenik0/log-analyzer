# CLI reference

Reference for supported input, output, filters and command options. Start with [[usage]]; consult the installed command help for authoritative syntax.

## Supported Log Format (Quick Check)

`log-analyzer` works best with structured text logs where each entry looks like:

```text
component | timestamp [LEVEL] message
```

Example:

```text
socket | 2025-04-03T21:07:27.668Z [INFO ] Emit event of type "Logger.log" with payload {...}
core-universal | 2025-04-03T21:07:27.652Z [INFO ] Core universal is started on port 21077
```

Classic component names support word characters, hyphens and slashes, including
`worker/io (run-1)`. Timestamp-shaped headers with unsupported component
punctuation are rejected candidates rather than multiline continuations.

Classic logs copied from a browser console may start with a source location:
`background.js:123 worker | timestamp [LEVEL] message`. Supported locations are
filenames, paths, or URLs followed by `:line` or `:line:column`, then whitespace
and a complete classic entry header. Detection and parsing normalize that prefix;
`raw_logline` and physical source line numbers remain unchanged. The location is
available as `structured_fields.console_source` (also filterable/extractable).
When the entry header is prefixed, source prefixes on its continuation lines are
removed for message/payload parsing. Text that lacks a complete classic header
remains continuation text.

It also auto-detects a few other common formats:

- Rust tracing: `timestamp level module::path: message key=value ...`
- Syslog/journald-style lines: `timestamp host process[pid]: message`
- JSON lines: `{"timestamp":...,"level":...,"message":...}`

Profiles can force a parser with `[parser] format = "rust-tracing"` (or `classic`, `syslog`, `json-lines`) and tune Rust target mapping with `module_depth` / `module_strip_prefix`. Structured `key=value` fields become filterable and extractable via `--filter "trace_id:abc123"` or `extract --field restream_name`.

These are supported parser grammars, not universal format recognition. Arbitrary
unstructured text and access-log formats may remain unparsed. JSON exports can
need explicit field mappings or normalization in a profile; a JSON object alone
does not supply lifecycle meaning. Parser coverage and rejected candidates must
be checked on the actual capture. Generic `base` behavior remains separate from
`eyes`, `custom-start`, `service-api` and `event-pipeline` lifecycle profiles.

## Parse Coverage and Exit Status

`info`, `errors`, and `perf` report each file's selected parser, active profile,
input size in bytes, parsed entry count, and rejected candidate count before
filtering.

JSON reports expose these under `coverage.files`, with aggregate
`parsed_entries`, `filter_matches`, and `status`. `info -F json` includes entry,
component, and level totals. It also provides advisory `info.next_steps`, mirrored
in text output: run an initial bounded investigation using automatic detection or
an explicit override, inspect support, and resolve/validate remaining semantic gaps.
The structural disclaimer means lifecycle semantics and upstream capture
completeness are **not assessed by structure**, not that investigation is unavailable.
If no semantic profile is justified, explicit `--profile base` still provides generic
facts and unavailable-goal diagnostics. The skill requires this initial investigation
before custom parsing; scripts then address a documented gap in the selected
profile, supported relationships or processed evidence. Bounded input inspection
for profile assertions can precede it.

An input file with non-whitespace content but no recognized entries fails with
exit status **1**, even if other input files parse successfully. Its coverage
report is still printed and saved by `-o`; no success summary is printed.
Empty/whitespace-only files succeed (`empty_input`). Parsed input with no filter
matches succeeds (`zero_filter_matches`). Parsed input without selected
ERROR/WARN entries succeeds with zero error totals (`parsed`). Other runtime
failures also exit 1; invalid CLI arguments exit 2.

Rejected candidates count attempted entries, including malformed JSON lines and
unrecognized leading blocks. Multiline payloads and stack frames stay attached
to their entry and do not count as separate rejections. Partially recognized
files succeed but expose rejected candidates; review coverage before trusting
a diagnosis. Format auto-detection samples the first ten nonempty lines from the
same consumed input stream used for parsing. Selection retains the existing
score/tie-break behavior; it is not a validation verdict.

Each coverage object adds `structural_diagnostics` version 1: selection provenance,
sampled format-match counts and no-match/tied/mixed status, whole-input format
observations, physical candidate blocks, attached nonempty lines, blank lines,
and bounded rejection source lines/reasons (first 20, with omitted/total counts).
Text coverage shows these observations and limits, including clean and blank
inputs. Bounded error text explicitly labels a compact clean-input structure
summary to preserve its existing budget. It retains sample/full-input status and
unverified attached-line counts, while omitting clean-only selection provenance,
exact format-match vectors, physical-block, blank-line and zero unsupported-header
counts. Unbounded text and JSON expose those counters. Ambiguity,
unsupported structure and rejection details remain visible in bounded text,
with the existing mandatory-metadata budget exception when necessary. Physical blocks count
attempted source blocks; normalized parsed/rejected rows remain separate populations.
Attached lines are not automatically verified continuations. Format observations
can overlap (for example Rust tracing/syslog or JSON-looking payload lines).
Neither format recognition nor capture coverage proves lifecycle meaning or
upstream capture completeness. Existing report schema 1 retains additive fields;
older retained reports without these diagnostics remain valid.

A documented unsupported Python-style shape is an unindented date/time followed
by a plain or bracketed level, then logger/message text (for example
`2026-01-01 10:00:00,123 ERROR service - failed`). When it does not match a supported
Rust tracing/syslog header, it starts a rejected block rather than being swallowed
into a previous record. Its traceback lines stay in that block. This is explicit
unsupported-structure reporting, not universal Python logging support. Indented
header-like text remains attached. Use supported input formats or explicit
structured-export normalization; generating lifecycle rules cannot repair an
unsupported structural grammar.

## Evidence contract for agents

JSON reports add snapshot-scoped `evidence_ref` citations and shared input,
profile, query, coverage, redaction and omission semantics under the existing
`report_metadata.evidence`.

Inspect `log-analyzer capabilities` for embedded schema documents
and contract versions. Published [JSON Schemas](../schemas/) and the
[evidence contract](contracts/evidence-contract.md) document report variants,
citation resolution, measurement boundaries, changed-input behavior and limits.

Source IDs survive selection and presentation. Recheck input/profile identities
before reusing citations. Keep observed facts, measurements, hypotheses,
contrary evidence and unknowns distinct using the investigation-result schema.
Nonempty unparsed input returns a coverage-only JSON document with exit status 1,
including all declared inputs and the same document in `--output`.
Aggregate inventories require record retrieval before citing an occurrence;
legacy error blocking spans are explicitly estimates.

Investigation report contract v1 and retained artifact contract v1 are published in
[the investigation contract](contracts/investigation-contract.md),
with [synthetic examples](../examples/investigation-contract/). `capabilities` embeds
these under `report_schemas.investigation` and `evidence_artifact`, and separately
advertises implementation availability. Schema 1 accepts both the existing agent-result shape and the retained report
shape. These contracts separate assessment, processing completion, presentation
omissions, occurrence/population identity and verification after redaction. `investigate` and `evidence` implement the retained contract;
[the unified workflow](usage.md#unified-investigation-and-retained-evidence) explains their limits.
Applied redaction in the retained contract omits record payloads and exposes only
source omission markers in excerpts; arithmetic can remain independently checkable.

## Bounded investigation output

Use `--report-max-chars`, `--report-max-bytes`, or `--report-max-items` for compact
JSON pages across investigation commands.

Limits include the final serialized
metadata and newline; character counts are not model-token counts. Keep input,
profile, selection and redaction unchanged and pass `retrieval.next_cursor` to
`--report-cursor` for the next page. Use `--complete-output` for every collection.

Coverage, totals, applicability, ambiguity and omission counts remain visible.
`retrieval.collections` separates prior, displayed and remaining items;
`evidence_records` returns each selected source record once. Tiny budgets or an
atomic oversized item produce an explicit stop state. Increase the budget or
use complete output instead of truncating JSON. Changed-input cursors return a
structured error and exit 1. Returned output is bounded; parsing memory/CPU are
not. See [the common budget and retrieval contract](contracts/bounded-reports.md)
for exact units, ordering, compatibility and measured synthetic resource behavior.

## Other CLI tasks

Use these commands for comparisons, focused searches and specialized analysis.

```bash
# Compare two log files
log-analyzer compare file1.log file2.log

# Show only differences
log-analyzer diff file1.log file2.log

# Get log overview (single file or multiple files)
log-analyzer info logs/*.log

# Opt into the built-in Eyes/Applitools profile when analyzing that log family
log-analyzer --profile eyes info logs/*.log

# Structured grep-style search with log-aware filtering
log-analyzer search file.log -f "t:retryTimeout" --context 2

# Search Rust tracing fields directly
log-analyzer search file.log -f "actor_kind:switch" --payloads

# Extract a payload field and aggregate occurrences
log-analyzer extract file.log -f "t:makeManager" --field concurrency

# Extract a structured tracing field
log-analyzer extract file.log -f "trace_id:fabb5aa4" --field restream_name

# Diagnose clustered errors and affected sessions across related logs
log-analyzer --profile eyes errors logs/*.log --warn --sessions

# Analyze performance bottlenecks across one or more files
log-analyzer --profile eyes perf logs/*.log

# Trace one operation lifecycle by correlation/request ID or session path
log-analyzer trace logs/*.log --id f227f11e

# Generate LLM-friendly output
log-analyzer llm file.log

# Prepare a separate request-profile candidate; inspect its validation gaps
log-analyzer profile prepare logs/*.log --template custom-start --profile-name my-team \
  --candidate-output my-team.toml --kind request --summary

# Alternatively, start from Eyes rules (choose a fresh candidate path)
log-analyzer profile prepare logs/*.log --template eyes --profile-name my-eyes-team \
  --candidate-output my-eyes-team.toml --kind request --summary
```

## What Counts as "Related Logs"?

Use multiple files together only when they belong to the same run/session (for example rotated/split chunks from one test run or one service execution window).

Good signs they are related:

- overlapping or contiguous timestamps for one investigation window
- same environment/test run/build context
- shared `component_id` hierarchy or correlation/request IDs
- files were split/rotated from one process/run

Avoid mixing unrelated runs, retries from different executions, or logs from different environments in the same command. That can distort counts, traces, session impact, and latency/orphan analysis.

## Specialized and compatibility commands

The six primary commands are listed above. `--help-advanced` reveals specialized
commands; compatibility shortcuts remain callable but hidden from primary help.

| Command | Aliases | Description |
|---------|---------|-------------|
| `compare` | `cmp` | Compare two log files |
| `diff` | | Compare showing only differences |
| `info` | `i`, `inspect` | Display statistics for one or more log files |
| `search` | | Structured grep-style search for matching log entries |
| `errors` | | Cluster ERROR/WARN patterns and summarize affected sessions |
| `extract` | | Extract and aggregate a JSON payload/settings field from matching entries |
| `perf` | | Analyze operation timing across one or more log files |
| `trace` | | Trace one operation/session across one or more log files |
| `process` | `llm` | Generate LLM-friendly JSON output |
| `schema` | | Preview structured-export JSON paths and types |
| `llm-diff` | | Generate LLM-friendly diff output |
| `generate-config` | `gen-config` | Return generated TOML inside JSON; raw export with `--format text` |

## Output for models and people

Command results default to JSON. Use `--summary` for concise JSON without losing
coverage, uncertainty, provenance, or recovery instructions:

```bash
log-analyzer investigate capture.jsonl --artifact evidence.json --summary
log-analyzer evidence evidence.json --summary
log-analyzer profile resolve capture.jsonl --kind request --summary
log-analyzer info capture.jsonl --summary
```

`investigate --summary` uses the versioned investigation brief contract and includes
up to five findings. The older hidden `--brief` spelling remains a navigation-only
brief. `capabilities --summary` omits embedded schemas. Paginated analysis and evidence
commands return at most five presentation items and preserve exact continuation cursors.
Profile preparation limits representative witnesses to five. Schema previews, mapping
reports, and generated-profile output use a `summary_version: 1` envelope with a
`report`, explicit JSON-pointer `omissions`, `limitations`, and `next_steps`. Coverage,
diagnostics and provenance remain exact. Scalars are not truncated; summary mode is
not a byte limit. Mapping mutations must not be rerun to expand a report; use inspect.

`--summary` conflicts with `--complete-output` and `--format text`. Use JSON page
budgets where supported for precise size limits. Existing text renderers remain
available explicitly through `--format text`. Saved
reports use the same presentation as stdout; retained artifacts always remain JSON.
This changes the previous default text output: scripts expecting text must select
`--format text`, and scripts consuming generated TOML must select the raw export.

## Global Options

Global flags control profiles, output, filtering and evidence limits across commands.

| Option | Env Variable | Description |
|--------|--------------|-------------|
| `--summary` | | Concise JSON with explicit omissions and next steps |
| `-F, --format <text\|json>` | `LOG_ANALYZER_FORMAT` | Compatibility output selector; defaults to JSON |
| `-j, --json` | `LOG_ANALYZER_JSON` | JSON output (shorthand for `-F json -c`) |
| `-c, --compact` | `LOG_ANALYZER_COMPACT` | Compact output mode |
| `-f, --filter <expr>` | `LOG_ANALYZER_FILTER` | Filter expression (see below) |
| `-o, --output <path>` | `LOG_ANALYZER_OUTPUT` | Output file path |
| `--profile <name-or-file>` | `LOG_ANALYZER_PROFILE` | Select a built-in or TOML profile |
| `--color <auto\|always\|never>` | `LOG_ANALYZER_COLOR` | Color output control |
| `-v, --verbose` | `LOG_ANALYZER_VERBOSE` | Increase verbosity |
| `-q, --quiet` | `LOG_ANALYZER_QUIET` | Show only errors |

## Filter Expression Syntax

Use `-f, --filter` with a unified expression syntax:

```bash
--filter "type:value [!type:value] ..."
```

**Filter types (with aliases):**

| Type | Aliases | Description |
|------|---------|-------------|
| `component` | `comp`, `c` | Filter by component name |
| `level` | `lvl`, `l` | Filter by log level (INFO, ERROR, etc.) |
| `text` | `t` | Filter by text in message |
| `direction` | `dir`, `d` | Filter by direction (incoming/outgoing/unknown) |

**Prefix with `!` to exclude.**
Different filter types are combined with AND. Multiple values of the same type are OR-ed.

```bash
# Only core-universal component
-f "c:core-universal"

# Only ERROR level logs
-f "l:ERROR"

# Core component, exclude DEBUG level
-f "c:core !l:DEBUG"

# Contains 'timeout', incoming direction only
-f "t:timeout d:incoming"
```

## Command-Specific Options

Each command adds options for its analysis and output needs.

### compare / diff

Compare log inputs with context and optional semantic JSON comparison.

| Option | Description |
|--------|-------------|
| `-D, --diff-only` | Show only differences (always on for `diff`) |
| `--full` | Show full JSON objects |
| `-s, --sort-by` | Sort by: `time`, `component`, `level`, `type`, `diff-count` |

### info

Accepts one or more log files.

When multiple files are provided, entries are merged and analyzed together.
Use this only for related logs (for example, split files from the same run/session). Mixing unrelated runs can make counts and timelines misleading.

| Option | Description |
|--------|-------------|
| `-s, --samples` | Show sample messages per component |
| `--json-schema` | Display JSON schema information |
| `-p, --payloads` | Show payload statistics |
| `-t, --timeline` | Show timeline analysis |

### search

Searches one log file and prints matching entries using the same structured filter expression used by other commands.

| Option | Description |
|--------|-------------|
| `--context <n>` | Show `n` entries before/after each match |
| `--payloads` | Show parsed payload/settings JSON for displayed entries |
| `--count-by <field>` | Count/group matches by: `matches`, `component`, `level`, `type`, `payload` |

`--count-by` switches the command into count mode (grouped counts instead of entry output).

### errors

Diagnoses ERROR entries (and optionally WARN entries) across one or more related log files by clustering normalized message patterns and estimating session impact from `component_id` + orphan detection heuristics.

Accepts one or more log files. Entries are merged and sorted by timestamp before analysis.
Use this only for related logs from the same run/session. Combining unrelated logs can make affected-session counts and orphan outcomes misleading.

| Option | Description |
|--------|-------------|
| `--top-n <number>` | Number of clusters to show (default: 10, `0` = all) |
| `--warn` | Include WARN entries (default: ERROR only) |
| `--sessions` | Show affected sessions per cluster (cross-references `component_id`) |
| `-s, --sort-by <field>` | Sort by: `count` (default), `time`, `impact` |

Use `errors --bounded` for reports with long stacks. It defaults to 600 Unicode
characters per sample/pattern, 5 stack-frame lines per sample, and 12,000 Unicode
characters for the entire text report (including coverage, newlines, and omission
counts). Override with `--max-sample-chars`, `--max-stack-frames`, and
`--max-output-chars`; specifying any limit enables bounded mode. Zero omits that
detail or requests a zero text budget. Recognized stack-frame lines start with
`at `, `at` followed by a tab, Python's `File "`, or a Rust frame number and colon.
All sample content still counts against the character limits.

Coverage, overall totals, and session impact precede cluster overviews and
samples. Overviews receive space before sample details; remaining space is shared
across displayed clusters. Reports state omitted cluster, sample-character,
stack-frame, pattern-character, and requested session-detail counts. Counts
include details in clusters hidden by `--top-n` or the budget. Patterns in text
overviews are capped at 120 characters (or the smaller sample limit).

If mandatory scope/totals/impact metadata and omission counts exceed the text
budget, they are printed in full with an explicit warning, and cluster details
are omitted. This is the only exception to the text budget. JSON preserves full
coverage and totals, limits samples/patterns/frames, and reports omissions;
`--max-output-chars` applies only to text so JSON remains valid. Numeric analysis
totals are calculated before any presentation limits.

Complete details remain the default. Use `--complete --top-n 0` for every cluster
without detail limits; `--complete` conflicts with bounded mode and limit flags.

```bash
log-analyzer errors stacks.log --sessions --bounded --max-output-chars 2400
log-analyzer -F json errors stacks.log --bounded --max-stack-frames 2
log-analyzer errors stacks.log --complete --top-n 0
```

### Report redaction

Add `--redact` to any investigation command to redact text, JSON, stdout, and saved
reports.

This happens after parsing, filtering, comparison, and correlation, so
counts and evidence selection use original values. Text begins with
`[REDACTED OUTPUT]`; JSON includes `redaction.applied: true`. Raw evidence strings
inside reports are redacted too; input files are never modified.

```sh
log-analyzer search file.log --payloads --redact -F json
log-analyzer extract file.log --field token --redact
log-analyzer trace file.log --id request-123 --redact --mask-id request_id
```

Recognized sensitive fields are password/passwd/pwd, secret, token, access_token,
refresh_token, api_key/apikey, client_secret, authorization/auth, cookie/set_cookie,
credential/credentials, signature, and private_key. Matching ignores case and separators, including
camelCase. Compact JSON (`-j`, `-c -F json`, and `llm-diff`) stays compact after redaction.
Nested objects, JSON embedded in strings, quoted or unquoted log
assignments, and URL query names/values (including percent encoding) are handled.
Authorization and Cookie header values (including folded continuations) are fully redacted
for every scheme. Redacted text retains actual message line breaks so separate
headers remain distinct. Endpoints and ordinary correlation IDs are retained
regardless of ID length or identifier field-name style.
Use repeatable `--mask-id <field>` with `--redact` to mask selected identifier fields
as `[MASKED_ID:N]`; equal values share a replacement within one invocation across
stdout and files, including known ID occurrences in prose. Trace selectors are
masked even when no named input field or matching entry exists. Replacements are
local to each invocation. Numeric IDs in source prose are masked while report counts,
physical line numbers, and timestamps retain their original meaning. ID collection
and masking use a shared multi-pattern index for logs with many distinct IDs. Bounded errors are redacted before sample/output budgets
are applied; the redaction marker counts against the text budget. Omission counts
for strings describe the redacted presentation; numeric analysis totals retain
their original values.

Without `--redact`, investigation reports can contain raw sensitive data.
`process` and `llm-diff` retain their legacy payload-only sanitization defaults;
`--no-sanitize` disables that legacy behavior. `--redact` instead applies the shared
policy, including when `--no-sanitize` is present. Redaction covers recognized
fields and assignments, not arbitrary unlabeled prose. Generated profiles and
schema previews use the same report marking when redaction is requested.

### extract

Extracts fields from parsed payload/settings JSON for matching log entries.

A single
`--field` keeps the existing aggregate counts. Repeat `--field` for correlated rows,
or use `--rows` for one field. Each row includes the source file, physical line,
expanded source row path, full timestamp (including source offset), and available
request/trace/span/correlation IDs.

```sh
log-analyzer extract file.log --field name --field width --field status -F json
log-analyzer extract file.log --field name --field width --expand-array cases
```

Inputs are parsed once. Without expansion, arrays remain values and numeric dot
segments select individual array elements. `--expand-array <dot-path>` selects one
array in each payload and emits one row per item; fields then refer to that item.
There is no implicit expansion or Cartesian product. An empty array emits no rows;
a missing or non-array expansion is reported in `rejected_expansions` (also in text).
Rows retain JSON nulls; absent fields also have null values but are distinguished
by `missing_fields`. A missing payload retains a row with `payload_present: false`.
Embedded JSON parsing respects braces and escaped quotes inside strings.

| Option | Description |
|--------|-------------|
| `--field <path>` | Field name/path to extract (supports dot paths like `settings.retryTimeout`) |

### perf

Accepts one or more log files.

Entries are merged and sorted by timestamp before analysis, which enables cross-file pairing (for example, orphan resolution when an operation starts in one file and completes in another).
Use this only for related logs from the same run/session.

| Option | Description |
|--------|-------------|
| `--threshold-ms <ms>` | Slow operation threshold (default: 1000) |
| `--top-n <number>` | Maximum rows per section (default: 20, `0` = unlimited) |
| `--orphans-only` | Show only unfinished operations |
| `--op-type <request\|event\|command>` | Filter by operation type |

Sort options: `duration`, `count`, `name`

Text and JSON apply the same selection before output. `--top-n` limits each
operations, statistics, orphans, and threshold-violations section separately;
`0` includes all rows. Duration sorts operations by elapsed time and statistics
by average duration, descending. Count sorts by the full operation-group count;
name sorts alphabetically. Orphans are chronological. Full `totals` and `omitted`
counts describe the analysis before selection, including with `--orphans-only`.
The threshold highlights slow completed operations; it does not filter statistics.
Explicit `--sort-by` or `--threshold-ms` conflicts with `--orphans-only`, since
unfinished operations have no measured duration or completed-operation count.



Performance correlation uses operation kind, name, ID, and `component_id` scope.
Configure `[perf].correlation_scope_fields` for a composite scope (for example
`["component_id", "tenant"]`); `component`, structured fields, and top-level payload
fields are supported. A missing, null, or empty configured scope field is
reported instead of matched. A record-field scope value over 4096 bytes is represented
by a UTF-8-safe prefix, its byte length, and a 128-bit FNV-1a digest, so equal long
values still pair. Short values containing the reserved ` bytes, fnv1a128:` marker
are also encoded to prevent them from impersonating generated summaries. These
keys are bounded summaries, not the original scope; digest collisions remain possible.
Explicit event-rule scope mappings use the same reserved-marker encoding after
validation and retain their 4096-byte input limit.
Logs without session IDs need an explicit known
scope (for example `["component"]`) or an intentionally empty scope list; an empty
list makes IDs global and overlapping starts remain ambiguous. JSON envelope
`payload`/`fields` are retained separately from embedded message payloads and
consulted for scope lookup.
Related files can pair across file boundaries because filenames are provenance,
not correlation scope. Overlapping starts for the same composite key are ambiguous:
all group events are preserved, starts remain orphans, and no duration is invented.
Unmatched ends and missing keys are retained in `unmatched_events`; JSON includes
`ambiguous_groups` and start/end source file and line references. Diagnostic
sections also obey `--top-n`, with full totals and omitted counts; `0` preserves all.

Some producers log only the start of an operation. Mark such a start rule with
`end_expected = false` in its version-2 `mapping`; `perf` then counts it as
`start_only_events` instead of an orphan, and session levels can complete on it.
Other rules keep expecting an end, so a missing end there is still reported.
These records do not produce measured durations or completed timing operations.
An operation-type filter counts excluded start-only records as suppressed events.

`perf` reports `operation_coverage` separately from parse coverage in text and JSON.
Relevant events are parsed request, event, and command candidates; each is paired,
unmatched, suppressed, or explicitly start-only. Suppressions identify operation-type filters, missing
recognized boundaries. Command starts no longer depend on finding a completion
elsewhere; starts, ends and identity-only evidence remain diagnostic when unpaired.
Conflicting rule interpretations and invalid mappings also remain unmatched.
Unknown generic records evaluated by command rules are counted separately in
`unclassified_command_records`, without fabricating command identities.
`no_applicable_events` means no candidates; `insufficient_evidence` means candidates
but no measured pairs; `partial_evidence` includes pairs plus unmatched/suppressed
candidates; `observed_pairs` means there are measured pairs and all remaining
candidates paired or explicitly expect no end, without claiming
capture completeness. Ambiguous/rejected event counts are subsets of unmatched
counts. Ambiguous pair cardinality is `null`/unknown when it cannot be established;
rejected pairs count groups with exactly one start and one end whose timestamp
years are inferred. All boundaries in a group containing inferred years are
rejected before date sorting and retained only in diagnostics, without claiming
a missing completion in the orphan list.
Coverage remains full when display limits or `--orphans-only` hide rows.

The observed capture window uses filtered parsed timestamps and preserves source
offsets. Bounds and elapsed time are unavailable for empty selections or inferred
years. Boundaries outside the input or filters remain unknown; upstream export
completeness is always `unknown`. Equal timestamps across files or duplicated
physical rows with identical row paths have no established boundary order and cannot create durations or assert a missing completion in the orphan list.
Ties are isolated at closed lifecycle boundaries; established sequential pairs
before and after the uncertain segment remain measured.

Substring `trace` searches can span multiple lifecycles and do not establish pairing.

### Configurable event timelines

Custom profiles can add event regexes and named start/end pairs.

`perf` includes
`event_timeline`; `trace` applies the same rules to selected matches. Patterns
match original raw entries and may expose named captures used in composite keys.
Correlation fields also accept `component_id`, `component`, structured fields,
and top-level JSON envelope/payload fields. Each pair must use identical key fields.

```toml
[[timeline.events]]
name = "fetch_begin"
pattern = 'fetch begin id=(?P<id>\w+)'
correlation_fields = ["component_id", "id"]

[[timeline.events]]
name = "fetch_response"
pattern = 'fetch response id=(?P<id>\w+)'
correlation_fields = ["component_id", "id"]

[[timeline.pairs]]
name = "fetch"
start_event = "fetch_begin"
end_event = "fetch_response"
timing = "measured"
```

Pair timing is `measured`, `inferred_sleep`, or `unknown`. Only measured boundaries
produce `measured_duration_ms`; all pairs retain their observed timestamp gap.
A retry logged after sleep should end an `inferred_sleep` pair, not a response-time
measurement. Missing boundaries/keys and overlapping starts remain explicit;
ambiguity stays within the affected lifecycle segment, preserving independent
completed intervals. Events retain source lines, full timestamps,
sample counts, and gaps since the previous matched event. Explicit source offsets
are retained in timeline timestamps; naive times are marked `host_assumed`.
Yearless syslog dates are marked `inferred_year` and cannot produce measured
intervals or capture spans until the missing year is resolved. All cross-event
gaps remain unavailable when mixed year provenance prevents overall ordering. Equal
timestamps across files cannot establish boundary order and remain ambiguous. Summed measured work
can exceed elapsed capture time when work overlaps. The capture window describes
observed entries; upstream capture completeness remains `unknown`.
Per-pair coverage reports unavailable measurements even when another pair succeeds;
configured event types with no matches have explicit zero sample counts.
Timeline evidence is retained in full independently of the performance row limit.

### trace

Accepts one or more log files.

Entries are merged and sorted by timestamp, then filtered by one selector:
- `--id <substring>` matches correlation/request IDs by substring in the raw log line (useful for truncated IDs from grep output)
- `--session <substring>` matches the `component_id` hierarchy/path (for example `manager-ufg-3nl`)

This is intended for tracing a single run/session across split logs. Mixing unrelated files may produce noisy traces.

| Option | Description |
|--------|-------------|
| `--id <substring>` | Trace by correlation/request ID substring |
| `--session <substring>` | Trace by `component_id` / session path substring |

### llm / llm-diff

These commands produce bounded summaries or comparisons for model consumption.

| Option | Description |
|--------|-------------|
| `-s, --sort-by` | Sort by: `time`, `component`, `level`, `type`, `diff-count` |
| `--no-sanitize` | Disable sensitive field hiding |

`llm` (`process`) supports `time` (default, earliest first), `component`,
`level` (highest severity first), and `type` sorting. `diff-count` applies only
to `llm-diff`. Filtering and sorting happen before `--limit` (default: 100,
`0` = unlimited). Ties retain input order after a timestamp tie-breaker.
The reported time range is the minimum/maximum timestamp among returned entries.
All `process` timestamps (entry `ts` and time-range bounds) use full RFC 3339
UTC dates and times with milliseconds and `Z`, independent of the host timezone.

### generate-config

This compatibility command returns `generated_profile.toml` inside JSON.

Use
`--format text` for raw TOML, or prefer `profile prepare --candidate-output PATH`
to save a candidate separately from its validation report.
The JSON report includes source parse/filter `coverage`; generation does not
validate lifecycle semantics or activate the candidate.

Generate a profile from one or more related log files (for example, split/rotated logs from the same run/session).

| Option | Description |
|--------|-------------|
| `--profile-name <name>` | Name for the generated profile (defaults to file stem for a single input, otherwise `generated-profile`) |
| `--template <path-or-name>` | Base template path or built-in: `base`, `eyes`, `custom-start`, `service-api`, `event-pipeline` |

## Examples

These examples combine common commands, filters and output options.

```bash
# Compare logs filtering by component and level
log-analyzer diff file1.log file2.log -f "c:core-universal l:ERROR"

# Exclude DEBUG logs from comparison
log-analyzer diff file1.log file2.log -f "!l:DEBUG"

# Save JSON diff to file
log-analyzer -j -o diff.json diff file1.log file2.log

# Show operations slower than 500ms across a session split into files (not unrelated runs)
log-analyzer --profile eyes perf logs/*.log --threshold-ms 500

# Trace one operation across split files using a request/correlation ID fragment
log-analyzer trace logs/*.log --id f227f11e

# Trace a whole session subtree by component_id path prefix/substring
log-analyzer trace logs/*.log --session manager-ufg-3nl

# Comprehensive analysis across multiple files from the same run/session
log-analyzer info logs/*.log --samples --timeline --payloads

# Structured search with payload display
log-analyzer search file.log -f "t:makeManager c:core" --payloads

# Search with context (entry-based, not raw text lines)
log-analyzer search file.log -f "t:retryTimeout" --context 2

# Group counts by parsed payload JSON
log-analyzer search file.log -f "t:concurrency" --count-by payload

# Cluster recurring failures and include per-session outcomes
log-analyzer --profile eyes errors logs/*.log --warn --sessions --sort-by impact

# Extract a specific payload field and aggregate values
log-analyzer extract file.log -f "t:makeManager" --field concurrency

# Extract retry timeout values from matching payloads
log-analyzer extract file.log -f "t:retryTimeout" --field retryTimeout

# LLM-friendly output with a custom limit
log-analyzer llm file.log --limit 50

# LLM-friendly diff sorted by highest-severity levels first
log-analyzer llm-diff file1.log file2.log --sort-by level

# Prepare a candidate from related split logs and report request-timing gaps
log-analyzer profile prepare logs/run-*.log --template custom-start --profile-name my-team \
  --candidate-output my-team.toml --kind request --summary
```
