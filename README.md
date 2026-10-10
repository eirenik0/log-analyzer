# Log Analyzer

[![CI](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml)
[![Release](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml)

Log Analyzer helps AI agents investigate failures and performance problems using
compact, verifiable evidence from logs. It is a local Rust CLI for agents and
people: Rust parses, classifies, correlates and calculates; the consuming agent
chooses queries, tests explanations and communicates findings. The binary does
not call a model or automatically prove a root cause.

Use the [portable investigation workflow](docs/investigation-workflow.md) as the
maintained technical reference. The analysis skill teaches the current
investigate-first workflow with `--profile` as its only profile selector. Generic
parsing is available through `base`; domain lifecycle semantics require a suitable
built-in or validated file-based profile.

## Installation

Building from source requires Rust 1.89 or newer.

```bash
# Auto-detect platform and install latest release
curl -fsSL https://raw.githubusercontent.com/eirenik0/log-analyzer/main/scripts/install.sh | bash

# Or build from source
cargo install --path .

# Verify installation
log-analyzer --version
```

## Unified investigation and retained evidence

Capture each independent input once, detect its profile when needed,
then parse/classify and correlate the captured input once. Inspect
calculated findings, source boundaries, explicit populations and per-goal support:

```bash
log-analyzer --profile examples/investigations/profile.toml investigate \
  examples/investigations/slow.jsonl --artifact evidence.json --report-max-items 5
log-analyzer investigation-evidence evidence.json \
  --expected-sha256 <artifact.stored_sha256> --collection /findings --report-max-items 5
```

Goal support and semantic coverage describe the exact-selected population. Empty
selection has no supported lifecycle assessment; unrelated orphans and scope aliases
do not downgrade selected pairs. Outcome counts require a rule applicable to selected
records. Policy `payload.*` selectors share extraction semantics: decoded payloads
have precedence, with per-field envelope fallback and nested paths.

The main report's `retrieval.next_cursor` resumes `/findings` through
`investigation-evidence --report-cursor ...`. Retrieve `/records`, `/populations`,
`/memberships`, `/memberships/N/members`, `/sequences` or `/sequences/N/events` for
specific evidence; `--id` selects an exact retained ID/reference ID. Retrieval
binds the artifact checksum, input snapshot, effective profile/query, redaction
and collection/ID selector. It never parses or correlates sources again. Optional
`--verify-sources` hashes current files separately and reports unchanged, changed,
missing or unavailable. Prefix verification compares only consumed bytes and reports
`prefix_unchanged`, `prefix_changed` or `prefix_shortened`; retained facts stay about the captured
snapshot. Every declared input receives a verification result; unread sources are
unavailable because no retained snapshot exists. Artifact reads and optional source
verification have byte limits.
Artifacts stay local until deleted, are never overwritten and need caller-managed
retention. Retrieval saves only new `--output` files.

Bare `investigate` automatically checks built-ins and TOML profiles discovered
recursively in `./config` (relative to the working directory). `--profiles-dir DIR`
replaces that directory. Detection uses captured prefixes: at most 128 physical
lines and 64 KiB per input, sharing a 256 KiB total sample allowance equally across inputs. Exactly one matching profile, with lifecycle
evidence in every nonempty input and no observed parsing/classification loss, is
selected. Overlapping grammars (including common `eyes`/`custom-start` wording),
mixed profiles, unknown formats and insufficient evidence fall back to generic
`base`; the highest match count never breaks a tie. Detection is sample-based
inference, not independent validation or proof of completion. Unsampled content may
differ, so inspect full-analysis coverage and per-goal support.

A **profile** defines parsing, normalization, lifecycle and correlation rules.
It can be built in or stored in a TOML **profile file**. Select either through the
global `--profile NAME_OR_FILE` option (or `LOG_ANALYZER_PROFILE`):

```bash
log-analyzer --profile eyes info logs/app.log
log-analyzer --profile ./config/team.toml info logs/app.log
```

Exact built-in names select built-ins; all other values are file paths, resolved
from the working directory. Use `./eyes` for a file named `eyes`. File profiles keep
`extends` inheritance. `--config FILE` and `--preset NAME`, including their existing
environment variables, remain hidden compatibility aliases with their original meanings. CLI help and
current examples show only `--profile`.
Selectors are mutually exclusive, including selectors supplied by environment
variables. `capabilities.profiles` advertises the unified option and built-in names;
the legacy capability and persisted JSON keys remain compatible.

Use `--profile` to override and bypass detection; `--profile base` keeps
inspection generic. Invalid explicit profiles remain errors. Detection never
rereads log sources or loads persistent mappings. The query's
`execution.profile_selection` reports the status, candidates, sample extents,
limitations and next step. Both explicit and automatic selection include the
selected profile name, origin(s) and effective `profile_sha256`. `detection_parse_passes` and `analysis_parse_passes` explain
`parse_passes`; detection work/time and temporary memory share processing limits,
while `probe_records` are separate from processed analysis record counts. Retrieval
still performs zero parsing. Other analysis commands retain their existing defaults.

Discovery considers at most 16 candidate TOML files (plus their inherited sources),
visits 256 directory entries through four
subdirectory levels, and does not follow subdirectory symlinks. Each profile source
(including inherited files) is capped at 64 KiB, with a 1 MiB aggregate read allowance
and the existing eight-profile inheritance limit. One extra byte may be read to
detect an oversized source. Built-in and relative-file `extends` use the same loader
as explicit profile files. The selected effective profile is retained; it is not reloaded
from disk after probing. Candidates with identical effective-profile hashes merge
origins, so copies of built-ins do not introduce ambiguity. Distinct matching profiles
remain ambiguous, even if their labels or observed results coincide.

`profile_selection.discovery` reports the directory, limits and diagnostics;
candidates include their effective hash, origin paths and inherited-source hashes.
A missing default `./config` is allowed. An explicitly requested missing directory,
invalid profile file or discovery cutoff prevents automatic selection and leaves generic
analysis available with an explicit gap. An incompatible candidate's parsing failure
does not disqualify a different candidate that can parse/classify the sample. This
allows custom normalization profiles to be detected. Explicit profile overrides
skip directory discovery entirely, including `--profiles-dir`. No mappings or config
files are changed. Report/artifact paths cannot overwrite discovered source profiles
or inherited files; redacted reports omit discovery paths and queries.

Recognition, timing, failures and incomplete-lifecycle evidence
have separate support requirements; scope aliases and missing timestamp provenance
prevent a timing support claim. Each supplied file is an independent run, even
when IDs or paths repeat. Analyze related capture fragments together through the
existing commands only after establishing their common run. Compare independent
runs with explicit comparison provenance, rather than merging their events.

Use a repeatable exact JSON selector after shared correlation to retain complete
boundaries, for example:

```bash
log-analyzer --profile examples/investigations/profile.toml investigate \
  examples/investigations/slow.jsonl --artifact selected-evidence.json \
  --select '{"input_ordinal":0,"kind":"request","name":"run","correlation_id":"parent","scope":["slow-run"]}'
```

Selectors combine their declared fields with equality, use the effective correlation
scope and OR across repeated selectors. Missing fields do not satisfy requested
values. Legacy `--filter` keeps substring semantics and runs before correlation;
removing an end through a filter cannot establish a hang. Elapsed source intervals
preserve UTC offsets and do not measure CPU time, critical-path time or causes.

Processing limits are separate from output limits. Defaults are 16 MiB captured
input, 100,000 record attempts, 10,000 expanded-row attempts, 10,000,000 charged
work units, a cooperative 30-second deadline, a 512 MiB conservative data allowance,
64 MiB artifact storage and 256 KiB per physical line/multiline record. See
`investigate --help` for overrides. Work accounts for parsing bytes, loop steps
and sorting reservations; it is not a CPU instruction count. Rejected candidates
consume attempt capacity; reported record usage counts successfully parsed records.
Input cutoffs retain consumed-prefix hashes, never a full-file hash claim; final
JSONL fragments and unclosed multiline candidates are left open. Expansion checks
run before row cloning; ordinary normalization consumes only the general record
budget when array expansion is disabled. `--cancel-file path` requests cooperative cancellation when
the file appears. Interrupted correlation publishes no tentative missing-end results.
A cutoff makes affected full-input assessments insufficient while preserving completed
processed-population facts. Independent inputs have separate scope completion;
a later capture failure does not downgrade an earlier completed input. Oversized physical or multiline records retain nonempty and rejection diagnostics;
a cutoff never turns observed content into empty input. Rejected physical or
normalized records make declared-input support insufficient;
known processed counts and elapsed measurements remain retained. Selected conflicting or invalid
classifications make semantic populations partial or unavailable. Unresolved policy
role matches prevent complete domain counts and cardinality-dependent joins. Paired-population
distributions require measurable source timestamps for every pair; individual
reliable intervals remain available when that requirement is unmet. Identity-only
classification does not establish lifecycle support or authorize zero lifecycle
counts. Mixed identity-only and boundary evidence retains observed boundary facts
while declaring incomplete lifecycle coverage. Outcome and opposite-boundary
recognition are checked per selected operation family. Literal success/failure
mappings do not authorize the other outcome; unavailable recognition withholds
zero counts and missing-boundary claims. Positive classified facts remain retained
with explicit partial population coverage when capabilities differ. Reading exactly the input byte cap conservatively
reports a prefix if physical EOF was not observed.

The memory setting bounds a conservative reservation model for buffered/retained
source and calculated data, including normalization field-mapping amplification
and owned classifications. Shared configuration is reserved once; fixed per-record
metadata is charged separately from source-byte amplification. Artifact collections
move into their retained document, and construction buffers are released before
stored-byte validation. Native classic, tracing and syslog records reserve worst-case
source amplification before parsing, then release unused amplification based on
retained text and structured payload storage. Dense payloads retain the original
reservation when needed; JSON envelopes and normalization keep their existing
allowances. Classification and fixed metadata remain separately charged. Default
limits are unchanged. A `memory_limit` stop reports exhaustion of this accounting
allowance, not a measurement of process RAM. It can stop earlier than the record cap and does
not promise a process RSS ceiling. Profile loading, compiled regexes, fixed report
metadata and final partial-outcome delivery are outside that accounting; effective
profile serialization is capped at 4 MiB. Deadlines are checked between bounded
operations, not during an individual JSON parse, regex match, sort or filesystem
call. Artifact writing still finishes a valid partial outcome after cancellation.
Storage exhaustion reports an unavailable artifact and retained inline facts,
with omissions explicit; retry with a new artifact destination and suitable limits.

`--report-max-bytes`, `--report-max-chars` and `--report-max-items` bound atomic main
finding bundles without changing artifact measurements. Mandatory metadata can
exceed an impossibly small output budget, with an explicit status. An oversized
atomic bundle requires a larger permitted budget, not automatic unlimited output.
`--redact` omits captured payloads, raw/message/field data and effective profile,
replaces excerpts with source omission markers, and hides paths and domain identities.
Arithmetic may remain checkable; source/rule verification losses are declared.
Unavailable source text affects its own input scope; independent retained evidence
keeps its source verification, with aggregate artifact losses still explicit.
Keep the original query/profile/snapshot digests as opaque provenance.

Optional strict version-1 `[investigation]` profile policy declares roles by existing
classification rule IDs, event/attempt/resource populations with explicit identity
fields and occurrence/identity grouping, and source-target relationships with exact
join fields, required scope fields and cardinality. All identity groups and joins
also remain inside the effective correlation scope. String selectors reuse structured
field names and `payload.<field>` selection. String, number and boolean payload
identity/join values use canonical scalar text, matching structured fields: numeric
`1` and string `"1"` intentionally share a declared identity. Null, objects and arrays
are unsupported policy identities. Missing identity or ambiguous joins produce
exclusions/unknowns. See [the synthetic policy](examples/investigations/domain-policy.toml).
Declare screenshot attempts, poll sends, observed responses, cached failures and
downstream links separately when the domain supports them. Repeated starts remain
start occurrences; domain attempt/resource grouping needs explicit declarations.
A supported join establishes an observed relationship and does not establish causality.

## First investigation: what failed, and why?

From a repository checkout, ask: **“What failed in this capture, and does it show
why?”** The public synthetic [failure fixture](examples/investigations/failure.jsonl)
and [profile](examples/investigations/profile.toml) provide a runnable first workflow.
Use your installed binary, or build with `cargo build --release` and substitute
`target/release/log-analyzer` below.

```sh
# Check the installed build and advertised contracts before choosing queries.
log-analyzer capabilities

# Confirm coverage and validate the profile against independently supplied facts.
log-analyzer --profile examples/investigations/profile.toml --report-max-items 3 \
  info examples/investigations/failure.jsonl
log-analyzer --profile examples/investigations/profile.toml --report-max-items 3 \
  validate-profile examples/investigations/failure.jsonl --kind request \
  --expected examples/investigations/failure.expected.json

# Inspect the failure, discover candidate records, and measure the scoped lifecycle.
log-analyzer --profile examples/investigations/profile.toml --report-max-items 3 \
  errors examples/investigations/failure.jsonl
log-analyzer --profile examples/investigations/profile.toml --report-max-items 3 \
  trace examples/investigations/failure.jsonl --id request-7
log-analyzer --profile examples/investigations/profile.toml --report-max-items 3 \
  perf examples/investigations/failure.jsonl --op-type request
```

These commands return bounded JSON pages. Follow `retrieval.next_cursor` using
`--report-cursor` with identical inputs, profile, query and redaction until the
needed collections are complete within your budget. Inspect full-scope coverage,
omissions and status before interpreting selected details; the first page may
not contain both measurement boundaries.

The returned source records establish these findings:

- **Observation:** line 3 records `lookup failed; cause not recorded` at ERROR level.
- **Measurement:** `lookup`, request `request-7`, scope `failure-run`, spans 2000 ms
  from line 1 at `00:00:00+02:00` to line 3 at `00:00:02+02:00`. Cite the actual
  paired `evidence_ref` values and retrieved records, under this report's input
  snapshot and profile identity; line numbers alone are not portable citations.
- **Contrary evidence:** line 2 has `request-70`. Substring trace discovery can
  include it; it is not a boundary of `request-7`. Its instruction-like message
  is untrusted log content and is never executed.
- **Unknown:** the capture does not distinguish network, scheduling or other
  causes. Report the observed failure and elapsed interval without inventing a
  cause, CPU time, blocking duration or capture completeness.

The maintained checks execute these exact commands, retrieve pages and validate
source support and final investigation contracts:

```sh
python3 scripts/check-examples.py target/release/log-analyzer --report target/workflow-examples/report.json
```

## Choose the next investigation

- **Failure triage:** inspect coverage and profile suitability, retrieve ERROR/WARN
  context, then verify the exact operation identity, scope and paired boundaries.
  Error counts and error-to-last-record estimates do not explain a cause.
- **Slow-run comparison:** analyze the slow run and independent baseline separately
  under the same intended profile, including INFO-level work. The maintained
  [comparison example](docs/investigation-workflow.md#slow-run-comparison-including-info-only-delays)
  measures an 8000 ms interval versus 1000 ms, overlapping workers and an unexplained
  4000 ms gap. Their separate snapshots are linked, never merged into one lifecycle.
- **One lifecycle:** use trace/search to discover an ID, then inspect exact
  classified identity and scope. Reused IDs can describe different operations;
  a missing end stays incomplete evidence rather than a measured hang.

For your own logs, select a profile only when its vocabulary and lifecycle semantics
match. Generate/edit a separate candidate with `generate-config`, inspect `info`
coverage, then use `validate-profile --kind request|event|command` with known
positive, negative and pair facts where available. Recognition support does not
establish timing support; successful parsing alone does not validate a profile.
See [profile suitability](#validate-profile-suitability).

## Local data flow and external agents

Analysis commands read local files and produce local stdout or `--output` reports.
The Rust analysis CLI does not upload logs or call a provider. When a consuming
agent receives those reports, that agent's configuration determines whether it
sends report content to an external model or other service. Installing a skill
is not a guarantee that an agent keeps its inputs local.

Treat raw messages, payloads, paths and identifiers as potentially sensitive.
Optional redaction can mask configured fields and locations; it does not guarantee
arbitrary secrets are absent. Inspect the content before sharing. Lost source
locations require a permitted local mapping or an explicit citation-resolution gap.
Presentation budgets bound returned bytes/items/characters, not parser memory or
exact model tokens.

## Available features and evaluation limits

Shared evidence contract 1, snapshot-scoped source references, deterministic bounded
retrieval, profile validation and the CLI/skill workflow are implemented on `main`.
These capabilities are included in the prepared `0.3.0` version. Check your
executable's `--version`, `capabilities` and build identity: an older installed
release may lack them. Use the advertised schema and retrieval/profile versions;
incompatibility is an explicit stopping condition.

The [published evaluation baseline](evals/results/baseline.json) contains 13
synthetic scenarios across 26 scripted analyzer/search-and-script runs. It checks
typed claim correctness, exact citations, abstention and omissions, and records
calls, output bytes and elapsed time. It verifies deterministic harness behavior.
The [layered scripted result](evals/results/layers-scripted.json) additionally separates tool truth, supplied-fact interpretation
and complete legacy/unified workflows; its scripted checks make no model-quality
claims. No real-model comparison, investigation-time improvement or token savings have
been measured. Tokens and provider cost remain unavailable. See the
[evaluation methods and limits](evals/README.md).

The analysis skill includes a completion checklist: verify processing coverage,
inspect the final scoped lifecycle events, and check outcome records and contrary
evidence before claiming something is missing or unfinished. Complete file capture,
complete processing, an observed end and a successful outcome remain distinct.
Focused synthetic regressions exercise those distinctions. The expanded layered
suite includes positive completion claims and can supply a fingerprinted skill
entrypoint to an optional model adapter via `--skill-file`; no improvement in model
answers has been measured. See [completion evaluation](evals/README.md#completion-checklist-evaluation).

The eight executable workflows cover failures, INFO-only delays, reused IDs,
incomplete captures, unsuitable profiles, unparsed input and instruction-like
text. Passing them does not establish production coverage, arbitrary model quality
or prompt-injection resistance. The optional MCP adapter remains a later phase;
the CLI and maintained skill are the available integration path.

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
filtering. JSON reports expose these under `coverage.files`, with aggregate
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
`report_metadata.evidence`. Inspect `log-analyzer capabilities` for embedded schema documents
and contract versions. Published [JSON Schemas](schemas/) and the
[evidence contract](docs/design/evidence-contract.md) document report variants,
citation resolution, measurement boundaries, changed-input behavior and limits.

Source IDs survive selection and presentation. Recheck input/profile identities
before reusing citations. Keep observed facts, measurements, hypotheses,
contrary evidence and unknowns distinct using the investigation-result schema.
Nonempty unparsed input returns a coverage-only JSON document with exit status 1,
including all declared inputs and the same document in `--output`.
Aggregate inventories require record retrieval before citing an occurrence;
legacy error blocking spans are explicitly estimates.

Investigation report contract v1 and retained artifact contract v1 are published in
[`docs/design/investigation-contract.md`](docs/design/investigation-contract.md),
with [synthetic examples](examples/investigation-contract/). `capabilities` embeds
these under `report_schemas.investigation` and `evidence_artifact`, and separately
advertises implementation availability. Schema 1 accepts both the existing agent-result shape and the retained report
shape. These contracts separate assessment, processing completion, presentation
omissions, occurrence/population identity and verification after redaction. `investigate` and `investigation-evidence` implement the retained contract;
[the unified workflow](#unified-investigation-and-retained-evidence) explains their limits.
Applied redaction in the retained contract omits record payloads and exposes only
source omission markers in excerpts; arithmetic can remain independently checkable.

## Bounded investigation output

Use `--report-max-chars`, `--report-max-bytes`, or `--report-max-items` for compact
JSON pages across investigation commands. Limits include the final serialized
metadata and newline; character counts are not model-token counts. Keep input,
profile, selection and redaction unchanged and pass `retrieval.next_cursor` to
`--report-cursor` for the next page. Use `--complete-output` for every collection.

Coverage, totals, applicability, ambiguity and omission counts remain visible.
`retrieval.collections` separates prior, displayed and remaining items;
`evidence_records` returns each selected source record once. Tiny budgets or an
atomic oversized item produce an explicit stop state. Increase the budget or
use complete output instead of truncating JSON. Changed-input cursors return a
structured error and exit 1. Returned output is bounded; parsing memory/CPU are
not. See [the common budget and retrieval contract](docs/design/bounded-reports.md)
for exact units, ordering, compatibility and measured synthetic resource behavior.

## Other CLI tasks

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

# Generate a starter profile from one or more related logs
log-analyzer generate-config logs/*.log --template custom-start --profile-name my-team

# Generate a profile starting from the Eyes profile
log-analyzer generate-config logs/*.log --template eyes --profile-name my-eyes-team
```

## Choose and validate a profile

The default `base` profile supports generic parsing, inventory, filtering and
comparison. Domain-specific commands, requests, events and session completion
require explicit rules matching the supplied logs. Start from a suitable profile
or editable candidate and validate its recognition, scope and timing support on
sample evidence before trusting lifecycle results. A generated profile is a
starting point, not proof of correctness.

**Why this matters:**

- **Session completion tracking** (`info` profile insights, `errors --sessions`) requires `[[sessions.levels]]` to know which `component_id` prefixes map to runners, tests, checks, and environments - and which commands create or complete them. Without this, incomplete/orphaned sessions go undetected.
- **Performance pairing** (`perf`) uses versioned `[event_rules]` to classify shipped-profile command/request/event phases before cleanup. Legacy-only custom profiles retain their `[perf]` substring markers; those phases are cached at parsing too. Incomplete evidence is diagnostic even without any completion in the capture.
- **Payload extraction** (`extract`, `search --payloads`) relies on `json_indicators` and `command_payload_markers` from `[parser]` to locate and parse embedded JSON. If these don't match your log format, payloads are invisible.
- **Request lifecycle tracing** (`trace`, `perf --orphans-only`) uses explicit phase/ID/scope mappings. Transport direction never establishes a phase by itself.

The shipped `eyes`, `custom-start`, `service-api` and `event-pipeline` profiles
now use version-2 `[event_rules]`. Full-message rules recognize command, request
and event names/phases independently, including completion-only records. `base` remains
generic. Typed JSON-field rules are also supported; string fields are not coerced
to booleans or numbers. Rules compile once at loading and classification uses the
original message before payload removal.

Supported shipped command grammar (double-quoted JSON string names, with Unicode
and JSON escapes):

- `eyes` / `custom-start`: `Command "name" is called`; completions use `finished`,
  `finished successfully`, `returned`, or `completed`. Optional suffix: `with settings {payload}`
  or `with settings [payload]`.
- `eyes` also accepts `with default driver and settings` on command starts and
  decodes those settings like the regular `with settings` form.
- `service-api`: `Operation "name" started` or `begin`; completions use `completed`,
  `finished`, or `failed`. Optional payload suffix starts with `with settings` or `settings`.
- `event-pipeline`: `Stage "name" begin` or `started`; completions use `done`,
  `completed`, or `failed`. Optional payload suffix starts with `with settings` or `config`.
- A bare subject such as `Operation "name"` is identity-only diagnostic evidence.
  Other prose, including `No evidence that Operation "name" completed`, establishes
  no lifecycle boundary. Payloads occupy the remainder of the original message;
  lifecycle words inside them or the name cannot supply phases. After the root
  JSON5 value, only whitespace and complete comments are accepted; malformed,
  too-deep or trailing noncomment content stays visible with no decoded settings.

Supported shipped request/event grammar:

- `eyes` / `custom-start`: `Request "name" [id] will be sent` starts; `finished
  successfully` and `respond with` end. `eyes` also declares `that was sent` and
  `is going to retried` as end forms. The ID is optional and must immediately
  follow the name; it cannot contain whitespace or `]`. Optional suffixes are
  `to the address "endpoint"` and `with body {payload}` / `[payload]`.
  Eyes request starts also accept scalar bodies such as `with body undefined`.
  Real SDK `that was sent` response tails accept `respond with OK(200)` (or another
  status name/code), optional `, dont retry returned true|false`, and optional
  `, httpVersion: default` (or another version). Retry tails accept
  `with body ... is going to retried due to ...`. Arbitrary prose such as
  `that was sent ... is still pending` does not establish an end boundary.
  A standalone object/array response body must be structurally complete, with
  only whitespace or complete JSON5 comments after its root container.
  `Received event of type "name"` starts and `Emit event of type "name"` ends,
  with optional `with payload {payload}` / `[payload]`. The compact
  `{"name":"name"}` event subject is also supported. Event identity uses `payload.key`.
- `service-api`: `Request "name" [id] sent`, `queued` or `requested` starts;
  `completed`, `responded` or `failed` ends. Optional `to "endpoint"` and
  `with body` / `payload` object/array suffixes are supported.
  `Consumed event "name"` / `Received event "name"` starts;
  `Published event "name"` ends, with optional `payload` object/array.
  Event keys are tried in declared order: `key`, `traceId`, `requestId`.
- `event-pipeline`: `Call "name" [id] started` / `dispatched` starts;
  `done`, `completed` or `failed` ends. Optional `target "endpoint"` and
  `payload` / `with body` object/array suffixes are supported.
  `Consumed "name"` / `Received "name"` starts; `Published "name"` /
  `Emitted "name"` ends, with optional `payload` object/array.
  Event key order is `key`, `eventId`, `traceId`, `jobId`.
- A bare request subject is identity only. Missing request IDs or event keys stay
  diagnostic. These profiles explicitly declare request send/start, receive/end
  and event receive/start, emit/end; other applications can map different phases.
- Every specialized profile also accepts canonical structured fields:
  `operation_kind` (`command`, `request`, `event`), `operation_name`,
  `operation_phase` (`start`, `end`), optional `correlation_id`, and required
  `operation_direction` for requests (`send`/`receive`) or events (`emit`/`receive`).
  Normalized JSON values must be strings; flat tracing fields use the same string
  forms. Correlation scope inherits `[perf].correlation_scope_fields`.

**Compatibility and migration:** shipped profiles have strict whole-message
lifecycle grammar; incidental substring matches in extra prose are unsupported.
Legacy-only custom configurations keep their marker semantics. Version-1 explicit
rules keep their grammar; version 2 adds direction/endpoint and `first_field`
mappings, including the declared `payload.KEY` namespace. First-field lookup uses
presence order; a wrong type or empty value is invalid, not skipped. No present
alternative means missing correlation identity. Direction is optional in custom
rules and remains `Unknown` when absent; it never implies a phase.

To migrate, customize a shipped global `[event_rules]` template and remove all
legacy lifecycle fields named in validation errors: parser event emit/receive,
command prefix/start, request prefix/send/receive and perf command start/completion
markers. Keep payload separators, normalization and real scope fields. Global
rules cannot coexist with `[command_rules]`. The latter remains a deprecated
command-only compatibility wrapper permitting legacy request/event recognition.
`generate-config` shares the template's rules and preserves mode without inferring
wording. See the [event classification contract](docs/design/event-classification.md)
for versions, resource bounds, conflict policy and the library migration.

`perf` reports `operation_coverage.classification` for selected parsed records:
classified (with identity-only/legacy subsets), unclassified, conflicting, invalid
and unavailable evidence. These counts precede operation-type selection and display
limits; parse coverage stays independent and pre-filter. Unknown records remain
searchable. Measured pairs retain start/end classification and source provenance;
text and JSON show the same diagnostics and selection/omission totals. New fields
are additive under report schema version 1. `unclassified_command_records` remains
a deprecated compatibility count of unclassified records across kinds.

Command names are explicit correlation identities in shipped rules, paired only
within nonempty `[perf].correlation_scope_fields` (default `component_id`), cached
at parsing together with IDs/phases for all explicit operation kinds.
Explicit scope mappings override these inherited fields. Empty command scope is
now diagnostic in every mode, including legacy profiles; configure real scope
fields before pairing. Missing IDs/scope,
overlapping starts, unestablished timestamp ordering and inferred years prevent
pairing. Failed completions report `status = "failure"`; other documented
completions report `"success"`. Identity-only, conflicting and invalid matches
are visible in `perf` diagnostics, including profile/rule identity; unknown
generic records are counted in `unclassified_command_records` and remain searchable.
Explicit session completion hints require an end phase that is not failed;
a command name or a start alone cannot mark a session complete.

For library users, `LogEntry` now has `classification` evidence. Constructors
leave it absent; caller-constructed operation records without evidence are
reported as diagnostic, rather than having their message interpreted during
analysis. Legacy callers may explicitly use `attach_legacy_event_evidence`
before analysis. Changing display text or analysis marker configuration cannot
change a parsed record's cached phase, identity or explicit scope.

**How to get started:**

```bash
# 1. Start from the right built-in profile/template for your log family
#    Eyes / Applitools-style logs:
log-analyzer generate-config logs/*.log --template eyes --profile-name my-team

#    Other structured logs:
log-analyzer generate-config logs/*.log --template custom-start --profile-name my-team

# 2. Review and refine the generated TOML - add session levels, fix markers
#    The generator infers what it can, but domain knowledge is yours to add

# 3. Pin your profile when running analysis
log-analyzer --profile my-team.toml errors logs/*.log --sessions
```

If your log directory path contains spaces, quote the directory part but not the wildcard (for example `"/path with spaces"/logs/*.log`).

A profile can start from another profile instead of copying it. Put `extends` at the top of the file and write only the differences:

```toml
extends = "base"            # a built-in name, or a path relative to this file
profile_name = "my-team"

[profile]
known_components = ["api", "worker"]
```

Tables merge key by key and the child wins. Arrays and scalars are replaced whole, so a child `[[event_rules.rules]]` list replaces the parent's list. An omitted `profile_name` is inherited. Chains are allowed up to 8 profiles, counting the child and every parent (including built-ins); cycles and unknown parents are errors. A built-in name wins over a file with the same name; use `./base.toml` for the file. Explicit `event_rules` cannot coexist with legacy marker keys, so a profile that extends one with `event_rules` (all built-ins do, even when empty) must not set the legacy keys.

See [Profile Configuration](#profile-configuration) for the full reference and examples.

## First success with your own logs

Apply the [first investigation](#first-investigation-what-failed-and-why) to a
stable copy of related inputs. Select a suitable profile or edit a generated
candidate, inspect parse coverage and payload samples, then validate the requested
operation kind with independently known facts. Follow the
[portable workflow](docs/investigation-workflow.md) for failure triage, slow-run
comparison or one lifecycle. Missing boundaries, unknown scope and rejected input
limit the conclusion even when some records parse successfully.

## What Counts as "Related Logs"?

Use multiple files together only when they belong to the same run/session (for example rotated/split chunks from one test run or one service execution window).

Good signs they are related:

- overlapping or contiguous timestamps for one investigation window
- same environment/test run/build context
- shared `component_id` hierarchy or correlation/request IDs
- files were split/rotated from one process/run

Avoid mixing unrelated runs, retries from different executions, or logs from different environments in the same command. That can distort counts, traces, session impact, and latency/orphan analysis.

## Commands

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
| `generate-config` | `gen-config` | Generate a profile TOML from logs |

## Global Options

| Option | Env Variable | Description |
|--------|--------------|-------------|
| `-F, --format <text\|json>` | `LOG_ANALYZER_FORMAT` | Output format |
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

### compare / diff

| Option | Description |
|--------|-------------|
| `-D, --diff-only` | Show only differences (always on for `diff`) |
| `--full` | Show full JSON objects |
| `-s, --sort-by` | Sort by: `time`, `component`, `level`, `type`, `diff-count` |

### info

Accepts one or more log files. When multiple files are provided, entries are merged and analyzed together.
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
reports. This happens after parsing, filtering, comparison, and correlation, so
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

Extracts fields from parsed payload/settings JSON for matching log entries. A single
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

Accepts one or more log files. Entries are merged and sorted by timestamp before analysis, which enables cross-file pairing (for example, orphan resolution when an operation starts in one file and completes in another).
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

Custom profiles can add event regexes and named start/end pairs. `perf` includes
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

Accepts one or more log files. Entries are merged and sorted by timestamp, then filtered by one selector:
- `--id <substring>` matches correlation/request IDs by substring in the raw log line (useful for truncated IDs from grep output)
- `--session <substring>` matches the `component_id` hierarchy/path (for example `manager-ufg-3nl`)

This is intended for tracing a single run/session across split logs. Mixing unrelated files may produce noisy traces.

| Option | Description |
|--------|-------------|
| `--id <substring>` | Trace by correlation/request ID substring |
| `--session <substring>` | Trace by `component_id` / session path substring |

### llm / llm-diff

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

Generate a profile from one or more related log files (for example, split/rotated logs from the same run/session).

| Option | Description |
|--------|-------------|
| `--profile-name <name>` | Name for the generated profile (defaults to file stem for a single input, otherwise `generated-profile`) |
| `--template <path-or-name>` | Base template path or built-in: `base`, `eyes`, `custom-start`, `service-api`, `event-pipeline` |

## Examples

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

# Generate a profile from related split logs (merged before inference)
log-analyzer generate-config logs/run-*.log --template custom-start --profile-name my-team
```

## Environment Configuration

Set defaults via environment variables (prefix `LOG_ANALYZER_`):

```bash
export LOG_ANALYZER_FORMAT=json
export LOG_ANALYZER_FILTER="!l:DEBUG"
export LOG_ANALYZER_COMPACT=true
export LOG_ANALYZER_PROFILE="eyes"
```

## Validate profile suitability

Parsing records does not establish that a profile recognizes the intended
lifecycle. Validate an explicitly selected profile or editable TOML candidate
against representative evidence and optional known facts:

```bash
log-analyzer --profile examples/profile-candidate.toml --report-max-items 4 validate-profile examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
```

The JSON report separates parsing, global classification, requested-kind pairing,
rule/source witnesses, missing identities and scope, and expected results.
`--purpose timing` requires reliable observed pairs; `--purpose recognition`
checks classification and can support identity-only or intentional start-only
events. Unsupported, conflicting, and insufficient evidence remain distinct.
Exit 0 means supported on the observed sample, without asserting general semantic
correctness from match counts. Candidates are never activated automatically.

Use `generate-config` to save a separate candidate, validate it, and explicitly
select it for subsequent analysis. All candidates may be unsuitable. See the
[profile validation workflow and expected-fact contract](docs/design/profile-validation.md)
for source-addressed positive/negative facts, exact pair assertions, inheritance,
redaction, bounds, and scope/capture limitations.

## Resolve a profile explicitly

`resolve-profile` is an opt-in, read-only JSON interface, separate from the bounded
grammar inference in `investigate`. Discovery reports selected identity/digest and provenance,
stably ordered alternatives, per-candidate structural coverage, independent
recognition/timing assessments, scope diagnostics, assertions and unresolved
assumptions. It does not activate a profile or write configuration:

```bash
log-analyzer resolve-profile examples/profile-validation.jsonl --kind request --candidate-config examples/profile-candidate.toml
log-analyzer resolve-profile examples/profile-validation.jsonl --kind request --candidate-config examples/profile-candidate.toml --expected examples/profile-expectations.json
log-analyzer --profile examples/profile-candidate.toml resolve-profile examples/profile-validation.jsonl --kind request
```

Precedence is explicit `--profile`, then a revalidated supplied association,
then revalidated project and user mappings, then exactly one eligible built-in
or `--candidate-config` alternative. Invalid or
unsupported explicit choices are reported and never replaced. Built-ins sort by
name, editable alternatives by path (duplicates removed; at most 16). Distinct
effective profiles remain ambiguous even with identical observed results;
only identical effective profile digests are one alternative. Exit 0 means
a supported selection on this input. Ambiguous, insufficient or invalid/unsupported
explicit choices exit 1 with useful generic base-profile inspection and diagnostics.
The outer metadata describes that generic inspection; each candidate owns its
consumed-byte, effective-profile, query identity and source references.

Selection by this resolver requires no reported structural rejections and independently supplied
semantic assertions, not names, filenames, language, rule IDs or match counts.
For recognition, every selected record classified as the requested kind needs
passing `/status = "event"`, `/semantics/kind`, exact name, phase, correlation ID,
scope and `end_expected` checks. Check outcome whenever the candidate assigns one.
Attached lines remain unverified even when there are no structural rejections.
Explicit null phase/identity checks support recognition only. At least one positive
requested-kind record is required; negative-only or other-kind assertions do not
qualify. All supplied checks must pass. Timing additionally needs supported
pairing and an exact source-addressed pair plus `duration_ms` assertion for every
reported operation. A partially asserted requested population cannot authorize
selection. Unclassified records remain semantically unknown: even complete
assertion coverage does not establish that every relevant lifecycle was recognized,
capture was complete, clocks were synchronized, or elapsed time establishes cause.
Explicit choices bypass automatic proof requirements; sample suitability and
semantic sufficiency remain separately visible. `validate-profile` continues to
work without an expected-facts file.

Use common report budgets/cursors to retrieve nested candidate witnesses, rules,
assertion results and canonical evidence records; selection and candidate identities
remain present on every page. A candidate normalization can expose row references
unavailable to the generic base parser. Structural rejection is distinct from
missing lifecycle semantics: a generated profile cannot repair every unsupported
input format. Candidates consume and hash inputs separately and reject changes
between reads; freeze active inputs first. Candidate count is bounded, but total
input processing memory/work is not globally bounded.

`--association FILE` accepts a strictly checked, read-only
[version-1 association](schemas/profile-association.schema.json): one `preset` or
`config` selector, its effective `sha256`, ordered exact source path labels and
selected parsers, `event_contract: 2` and `structural_contract: 1`. Relative config
paths resolve against the association file. Use candidate identity and parsing
coverage to construct it deliberately. No raw log content belongs in this file.
Changed digest, missing profile, contract mismatch or source scope/structure
invalidates it and falls back to candidate discovery. It still requires fresh
sample validation and independent semantic assertions for an automatic choice.
A zero-match filter leaves successful parsing and structural revalidation intact,
while analysis remains insufficient. Empty sources have unverified structure and
cannot revalidate an association; a genuinely incompatible nonempty source remains
invalid even when another source is empty.
An explicit CLI choice bypasses the association and persistent lookup. Resolution
reports version 2 for mapping provenance; the embedded report schema retains
version-1 resolution and the supplied association contract remains version 1.

### Prepare a separate profile candidate

When resolution abstains, `prepare-profile` combines sample inventory generation,
rule witnesses, validation and scope/boundary diagnostics in one JSON report:

```bash
log-analyzer prepare-profile examples/profile-validation.jsonl --template examples/profile-candidate.toml --candidate-output prepared-profile.toml --kind request --purpose timing --expected examples/profile-expectations.json
log-analyzer --profile prepared-profile.toml validate-profile examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
```

`--template` accepts a built-in template name or TOML path; otherwise the command
uses the explicit global profile selection or base profile. It preserves supplied
lifecycle rules. Observed components, commands and requests are inventory;
parser/module mapping and session-prefix changes remain unverified heuristics.
It never infers intended phases, correlation scope, completion or capture completeness.
Global filters apply to sample validation; inventory uses the complete parsed sample.

The candidate must be a new file in an existing directory. Starting profiles and their inherited filesystem parents,
inputs and assertions are protected from destination collisions. The exact saved
TOML is reloaded and checked against stable input snapshots before creation.
Wholly unsupported nonempty input and empty input create no candidate; partial
structural support can create a candidate while reporting insufficient evidence.
Unsupported Python headers require parser/normalization work rather than guessed
rules. Creation never activates a profile or saves a persistent mapping.

Inspect `creation`, `structure`, `sample_validation`, `semantic_proof` and
`missing_information` separately. Validation runs without expected facts; missing
or partial independent facts cannot verify the requested population and timing
pairs. A successful exit can report an unsuitable candidate or abstention.
There is no automatic repair/retry loop. If a report cannot be saved after candidate
creation, stdout reports the committed candidate and failed delivery; validate the
saved file rather than retrying creation.

`--witness-limit` bounds representatives (default 20, maximum 100). Representatives
with an atomic string over 512 Unicode scalars or serialized size over 4096 scalars
are omitted whole, preserving exact retained identities, source addresses and facts.
Presentation omissions count size and count limits; outcome metadata, coverage totals
and opaque retrieval identities remain exact. Per-source coverage, including its
parser diagnostic counters and existing diagnostic limits, is exempt from preparation
limits. Use the saved candidate with
`validate-profile` and common report cursors for omitted evidence, or `info` for
unparsed input. Preparation itself rejects common budgets/cursors because replay
must not create files. Limits bound presentation, not processing memory/work.

`--redact` protects the report, including copied assertion witnesses. The executable
candidate remains unredacted so its rules retain their meaning; inspect it before sharing.

### Persistent source/profile mappings

`resolve-profile` reads the project registry at
`<project-root>/.log-analyzer/profile-mappings.json`, then the user registry at
`~/.config/log-analyzer/profile-mappings.json` (Windows uses `USERPROFILE` when
`HOME` is unavailable). The project root defaults to the current directory;
there is no ancestor search. Use `--project-root`, `--project-mappings`,
`--user-mappings`, or `--no-mappings` to control lookup. Missing registries remain
absent: lookup does not create directories, lock files, caches or configuration.

Matching uses the exact ordered list of canonical source paths plus requested
kind and purpose. Project paths and custom profile selectors are root-relative,
so a project can move as a unit. User mappings use absolute paths and absolute
project context; they never match another project by basename. Project source paths
and saved profile selectors must stay inside the root, including after resolving symlinks.
Persistence requires UTF-8 paths. A mapping does not establish relationships
between sources or combine independent runs into a correlation scope.

Every matching profile is loaded and checked against its saved effective digest,
event/structural/resolution contracts, current source parsing and independently
supplied semantic assertions. Prior success cannot authorize a new automatic
choice. No assertions means insufficient evidence. Changed rules, missing
profiles, incompatible input and malformed registries produce diagnostics and
allow lower-tier fallback. Multiple exact matches in a higher tier stop automatic
selection; they cannot be hidden by a lower-tier match.

Saving requires an explicit management command and a current selected profile
that passes the same independent-assertion gate. Explicit selection alone is
insufficient to remember it. The [registry schema](schemas/profile-mappings.schema.json)
stores selectors, ordered source paths, digests, selected parsers, contract versions
and assertion-digest provenance. It stores no raw log excerpts, assertion values,
credentials, reports or disposable analysis cache. Inspect the metadata before
tracking or sharing it; project configuration and private evidence remain separate.

```bash
log-analyzer --profile examples/profile-candidate.toml profile-mappings --project-root . remember examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
log-analyzer profile-mappings --project-root . inspect
# Copy entry.id and digest from inspect; replacement requires fresh validation.
log-analyzer --profile examples/profile-candidate.toml profile-mappings --project-root . replace examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json --entry-id ID --if-digest DIGEST
log-analyzer profile-mappings --project-root . forget --entry-id ID --if-digest DIGEST
```

`--scope user` chooses user storage; `--registry PATH` overrides the selected
scope's file. Inspect and forget do not need source/profile files to exist.
Remember refuses an existing source key. Replace and forget require the currently
inspected entry digest and report a conflict if it changes. Mutations validate
before acquiring a bounded native lock, reread under the lock, then sync and
atomically replace a same-directory temporary file. Directory aliases share a
canonical lock path; registry/lock symlink files are rejected for mutation. The
stable sibling lock file remains after use and operating-system locks release
when the process exits. These guarantees assume filesystem support for native
locking and atomic replacement; they do not claim universal power-loss durability.
Management refuses report destinations that conflict with its registry or stable
lock, including resolved aliases; use a separate `--output` report path.
Management returns full JSON and rejects common report budgets/cursors, so a
retrieval request cannot repeat a mutation.

Mapping management reports `mutation.status` separately from `report_save.status`.
An invalid `--output` destination fails before registry mutation. If report delivery
fails after the registry commits, the command succeeds with a warning and
`report_save.status: "failed"`; do not retry the mutation. Recover the report with
`profile-mappings inspect --output <separate-report-path>`. Inspection save failures
remain errors. Report destinations are staged without truncating existing reports.

## Profile Configuration

Use profile TOML files to keep the binary generic and push case-specific knowledge into config.

Included built-ins:

- `config/profiles/base.toml` - minimal reusable defaults
- `config/profiles/eyes.toml` - Eyes/Applitools-specific profile
- `config/templates/custom-start.toml` - starter template for any project
- `config/templates/service-api.toml` - service/API wording template
- `config/templates/event-pipeline.toml` - event-driven wording template

These profiles/templates are also embedded in the binary and can be referenced by name in
`generate-config --template`:

- `base`
- `eyes`
- `custom-start`
- `service-api`
- `event-pipeline`

Examples:

```bash
# Generic base profile
log-analyzer info logs/app.log

# Built-in Eyes profile
log-analyzer --profile eyes info logs/app.log
```

Create your own profile from templates:

```bash
# In this repository
cp config/templates/custom-start.toml config/profiles/my-team.toml

# If only the skill is installed globally, create the destination first
mkdir -p ./config/profiles

# Codex or Pi global installation
cp ~/.agents/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml

# Claude Code global installation (alternative to the Codex/Pi command)
cp ~/.claude/skills/analyze-logs/templates/custom-start.toml ./config/profiles/my-team.toml

# Then run with your custom profile
log-analyzer --profile config/profiles/my-team.toml info logs/app.log

# Or generate a profile using an embedded built-in template
log-analyzer generate-config logs/app.log --template service-api --profile-name my-team

# Generate a profile from multiple related log chunks (merged before inference)
log-analyzer generate-config logs/run-1.log logs/run-2.log --template custom-start --profile-name my-team
```

Only combine related logs from the same run/session when using `generate-config`; mixing unrelated runs can pollute inferred commands/requests/session levels.

For consumer repositories, prefer a tiny wrapper script or Make target that pins `--profile <name-or-file>`. That keeps the binary generic while making repo workflows explicit and repeatable.

### Validate Your Profile (Quick Checklist)

Before relying on analysis results, verify the generated/custom profile with a few quick checks:

- `info --payloads --samples` shows expected command/request names and parsed payloads (not mostly missing payloads)
- `search --payloads` for a known message displays JSON payload/settings content you expect
- `extract --field <path>` returns real values for a known field (not only empty/missing)
- `perf` does not show obviously impossible orphan counts/latencies for known-complete runs
- `trace --id` or `trace --session` finds a known request/session path from your raw logs
- `errors --sessions` reports sensible affected sessions after `[[sessions.levels]]` is tuned

If these checks fail, adjust `[parser]`, `[perf]`, or `[[sessions.levels]]` markers before interpreting the output.

Optional session hierarchy/lifecycle hints (used by `info` profile insights):

```toml
[[sessions.levels]]
name = "runner"
segment_prefix = "manager-"
create_command = "makeManager"
complete_commands = ["getResults", "closeBatch"]
summary_fields = ["concurrency", "batch.id"]

[[sessions.levels]]
name = "test"
segment_prefix = "eyes-"
create_command = "openEyes"
complete_commands = ["close", "abort"]

[[sessions.levels]]
name = "environment"
segment_prefix = "environment-"
```

When `sessions.levels` is configured, `info` automatically summarizes session counts/completion health per level and can surface common create-time fields (for example `concurrency`).

`generate-config` also detects session-like prefixes from `component_id` paths and embeds them as generic `[[sessions.levels]]` entries (`level-1`, `level-2`, ...).

## Agent Skills: Codex, Pi and Claude Code

The canonical [analyze-logs skill](.agents/skills/analyze-logs/SKILL.md) supplies
one investigation workflow and shared references, examples and profile templates.
Claude's wrapper and bundle are generated from it; Codex and Pi use portable
frontmatter without Claude-specific permissions or fork settings. Install the
[Rust executable](#installation) separately; a skill or Pi package does not include it.

Run the installer from the destination project:

```bash
/path/to/log-analyzer/scripts/install-skill.sh --host codex --scope project
/path/to/log-analyzer/scripts/install-skill.sh --host pi --scope user
/path/to/log-analyzer/scripts/install-skill.sh --host claude --scope project
```

Defaults remain `--host claude --scope project`; `--global`/`-g` means user scope.
Codex/Pi share `.agents/skills/` (project) and `~/.agents/skills/` (user); Claude
uses `.claude/skills/` and `~/.claude/skills/`. Project means the current directory.
An identical source installation is a safe no-op; overlapping destinations and
destination child symlinks are rejected before copying. Repeat installs update
bundle files and retain unrelated destination files.

Explicit invocation in each host:

```text
$analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
/skill:analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
/analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
```

The lines above are for Codex, Pi and Claude standalone respectively. Confirm
Codex discovery with `/skills`; confirm Pi startup discovery and use `/reload`
after edits. Pi project resources may require project trust. Install the Pi Git
package with `pi install git:github.com/eirenik0/log-analyzer` (add `--local` for
project scope; pin a tag/commit containing the skill for reproducibility).

Claude plugin paths and invocation remain:

```text
/plugin marketplace add https://github.com/eirenik0/log-analyzer
/plugin install log-analyzer
/log-analyzer:analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
```

Supply an absolute executable path if `log-analyzer` is unavailable on the host's
PATH. The skill checks `capabilities` for schema/evidence contract 1 and version-1
bounded retrieval/profile validation, then checks coverage and profile suitability.
Missing binaries or unsupported contracts stop with installation/upgrade guidance.
Input and profile paths must be accessible to the agent and executable.
See [host setup](.agents/skills/analyze-logs/hosts.md) for discovery and compatibility,
the checked [failure](.agents/skills/analyze-logs/examples/debug-failure.md) and
[performance](.agents/skills/analyze-logs/examples/performance.md) examples,
and the [host validation record](docs/skill-host-validation.md) for tested versions
and explicit gaps. External data handling follows the agent's configuration; see
[local data flow](#local-data-flow-and-external-agents).

To change the skill, edit `.agents/skills/analyze-logs/`, run
`python3 scripts/sync-skills.py`, and commit the generated Claude bundle too.
Tests reject drift. Scripted workflow checks do not measure model accuracy or
claim token savings.

## Development

Python evaluation and skill-check scripts read and write UTF-8 text explicitly,
including on Windows systems using an ANSI code page.

See [CONTRIBUTING.md](CONTRIBUTING.md) for Conventional Commit rules, local hooks,
quality checks, and hosted Codex review. Agent and reviewer guidance lives in
[AGENTS.md](AGENTS.md).

## Features

- **Structured parsing** - Extracts and parses JSON payloads automatically
- **Semantic comparison** - Compares JSON objects regardless of property order
- **Diff context improvements** - Tracks source line numbers and marks changes as added/removed/modified
- **Advanced filtering** - Include/exclude by component, level, content, or direction
- **Operation lifecycle tracing** - Discover matching IDs/session paths, then verify exact scope and measured boundaries
- **Multi-file session analysis** - Merge and analyze `info`/`perf` inputs across multiple log files
- **Session lifecycle insights** - Profile-driven session tree/completion tracking in `info` (with legacy prefix compatibility)
- **Performance analysis** - Identify slow and orphan operations
- **Agent evidence** - Compact JSON with snapshot-scoped source references, optional masking and explicit omissions
- **Profile-driven customization** - Override parser/perf markers via TOML config or generated templates
- **Flexible output** - Text or JSON format with color and verbosity control

## Development

CI uses the latest stable Rust toolchain. The codebase is verified with Rust 1.99.0. Run formatting, lint, and test checks before submitting changes:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Measuring search and tracing performance

Use optimized builds for timing. The synthetic benchmark covers overlapping search
context, search without context, and long Rust tracing messages with and without
structured fields. It reports repeated end-to-end timings and checks identical
stdout between binaries, excluding the separately recorded build identity footer.
Save a release binary before editing to compare changes:

```bash
cargo build --release --locked
cp target/release/log-analyzer /tmp/log-analyzer-before
# Make the change, then rebuild.
cargo build --release --locked
python3 scripts/measure-search-performance.py target/release/log-analyzer \
  --baseline /tmp/log-analyzer-before --repeats 3
```

Use `--records`, `--context`, and `--words` to vary input size. These local synthetic
wall times include startup, parsing, and rendering; they do not measure peak memory
or establish production throughput. Search visits each displayed context entry once
after ordering matches. Tracing rejects prose tokens before searching for field values.

To reproduce investigation memory pressure on 8,018 synthetic records (4,009 paired
operations), run the Unix memory probe in a fresh process for each release binary:

```bash
python3 scripts/measure-investigation-memory.py target/release/log-analyzer
```

It reports the processing stop reason, accounted bytes, completed counts, and actual
child peak RSS separately. `--memory-bytes` overrides accounting for diagnosis of an
older build; the default probe uses the executable's unchanged limits. This is a
synthetic workload, not a guarantee for differently sized records or profiles.

### Structured export normalization

Run `log-analyzer schema export.jsonl --samples 3` to preview JSON Pointer paths
and types without processing or decoding strings. Preview limits are explicit
in JSON (four levels, twenty object fields, three array items); it does not infer
field meanings. Configure a profile explicitly, for example for tuple rows:

```toml
[normalization.fields]
timestamp = "/0"
message = "/1/info/event"
payload = "/1/info"
```

Paths use JSON Pointer syntax, including numeric array indexes. For nested arrays,
set `[normalization] root_path = "/rows"` and `expand_rows = true`. Field paths
are then relative to each expanded row. `decode_paths` explicitly decodes strings
before root selection; `row_decode_paths` decodes strings within each selected row.
No string is implicitly decoded. All mapped fields are required; missing, null,
and wrong-type fields cause that row to be skipped with a diagnostic.

`timestamp_unit` accepts `seconds`, `milliseconds`, `microseconds`, or `nanoseconds`
for integer Unix timestamps, including negative values. Without an explicit unit,
timestamps must be strings. Omitted level/component fields use parser defaults;
unmapped rows must already have the normal object schema. Configuration forces
JSON-lines parsing. Normalization diagnostics appear in analysis `coverage.files`
and text coverage; single-file commands also emit skipped-row diagnostics on stderr.
Provenance retains the original file, physical line, and expanded JSON row path.
Search and trace expose row paths; event and ID matching use only each normalized
record, while original source lines remain available as evidence.
Use `info -j` to inspect coverage before interpreting an investigation report.

### Safe compact text

Compact payload values, messages and field names retain complete Unicode graphemes
within their byte budgets, including combining accents and emoji sequences.
Shortened field names use deterministic `...~2`, `...~3` suffixes when needed and
reserve original short names, so retained values cannot overwrite one another.
If the input already contains `_truncated_fields`, the omission marker gets a
unique numbered suffix too. The existing field, array and depth limits still apply.

### Build identity and capabilities

`log-analyzer --version` includes the package version, short source revision and
build state. `log-analyzer capabilities` always emits JSON with the full build
identity, schema version, canonical command names, output/parser formats and
built-in profiles. It does not load a profile, so it also works when the configured
profile is unavailable.

JSON reports include `report_metadata` with `schema_version`, `build` and
`active_profile`. Text reports end with a `Build:` line; generated TOML uses a
comment. Control characters in profile names are escaped in text/comments; JSON
retains the full profile name. Bounded error reports count build metadata against the text budget and
retain it when details cannot fit. Text match-count output therefore includes
metadata; use JSON for machine-readable counts.

The source state is `clean` or `dirty` for a Git checkout (`dirty` means tracked
changes), and `unknown` with a null revision for source archives or unavailable
Git identity. Metadata describes the source at compilation time, rather than the
checkout where a binary is later run. Profiles describe the selected parsing
configuration, including the template used by `generate-config`.

Maintained examples live in `examples/commands.json`. `cargo test --locked` runs
them against the test binary. After `cargo build --release`, run
`python3 scripts/check-examples.py target/release/log-analyzer`; release packaging
also checks native binaries and explicitly skips targets that cannot run on the
build host.

## Grounded evaluation baseline

The [evaluation harness](evals/README.md) scores typed factual claims, exact citations,
unsupported causes, abstention and important omissions. Mandatory CI runs a synthetic
CLI corpus and paired scripted analyzer/search investigations without credentials.
The [first published baseline](evals/results/baseline.json) verifies the harness;
optional repeated same-model comparisons remain unmeasured until an adapter is run.
The new layered smoke separates independently checked tool truth, interpretation
of supplied verified facts, and paired legacy/unified workflows. It includes frozen
held-out variants and retains answers, known partial usage and independent attempt
statuses after failures. Canonical artifact records include parsed severity.
Real-model runs require an explicit provider allocation; none have been executed.
Unrestricted prose/causal judgment uses a separately documented calibration and
review protocol, whose review runs remain unperformed. Missing tokens/provider
cost stay unavailable; known partial usage keeps its completeness indicator.

## Repository and release wording

Suggested repository description: **Local evidence engine for AI log investigations:
deterministic parsing, scoped timing and verifiable sources.**

Release introduction: **Log Analyzer helps AI agents investigate failures and
performance problems using compact, verifiable evidence from logs. The local Rust
CLI calculates; the consuming agent chooses queries and explains findings.** Link
the [portable workflow](docs/investigation-workflow.md) and disclose the installed
build's capabilities and evaluation limits. The project name remains Log Analyzer.

## Publishing to crates.io

The **Publish crate** workflow supports GitHub Trusted Publishing with short-lived
OIDC credentials. It validates an existing stable GitHub release and defaults to a
package dry run. See [setup and publishing instructions](docs/trusted-publishing.md)
for crate ownership, licensing, and the one-time crates.io configuration.
