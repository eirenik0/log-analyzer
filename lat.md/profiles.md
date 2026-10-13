# Profiles

Choose, validate and adapt parsing and lifecycle semantics for your logs. See [[contracts/profile-validation]] for the expected-fact contract.

## Choose and validate a profile

The default `base` profile supports generic parsing, inventory, filtering and
comparison.

Domain-specific commands, requests, events and session completion
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
wording. See the [event classification contract](contracts/event-classification.md)
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
# 1. Prepare a new candidate from a suitable template (Eyes shown here)
#    Use custom-start for another log family; choose request, event, or command
log-analyzer profile prepare logs/*.log --template eyes --profile-name my-team \
  --candidate-output my-team.toml --kind request --summary

# 2. Edit missing domain rules in my-team.toml, then check independent known facts
log-analyzer --profile my-team.toml profile validate logs/*.log --kind request \
  --expected known-facts.json --summary

# 3. Investigate with the justified profile and retain its evidence
log-analyzer --profile my-team.toml investigate logs/*.log --summary \
  --artifact new-evidence.json
```

Preparation can save a candidate while reporting unsupported or insufficient evidence
and exiting 1. Inspect that report, supply missing rules and independent assertions,
and validate before selection. `known-facts.json` must describe facts established
from the source or domain knowledge, never assertions copied from candidate output.

If your log directory path contains spaces, quote the directory part but not the wildcard (for example `"/path with spaces"/logs/*.log`).

A profile can start from another profile instead of copying it. Put `extends` at the top of the file and write only the differences:

```toml
extends = "base"            # a built-in name, or a path relative to this file
profile_name = "my-team"

[profile]
known_components = ["api", "worker"]
```

Tables merge key by key and the child wins. Arrays and scalars are replaced whole, so a child `[[event_rules.rules]]` list replaces the parent's list. Replacing `event_rules.rules` also clears the inherited `investigation` policy unless the child explicitly declares one, so parent rule IDs cannot invalidate a custom grammar. Explicit policies are still validated against the resulting rules. An omitted `profile_name` is inherited. Chains are allowed up to 8 profiles, counting the child and every parent (including built-ins); cycles and unknown parents are errors. A built-in name wins over a file with the same name; use `./base.toml` for the file. Explicit `event_rules` cannot coexist with legacy marker keys, so a profile that extends one with `event_rules` (all built-ins do, even when empty) must not set the legacy keys.

See [Profile Configuration](#profile-configuration) for the full reference and examples.

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
log-analyzer --profile examples/profile-candidate.toml --report-max-items 4 profile validate examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
```

The JSON report separates parsing, global classification, requested-kind pairing,
rule/source witnesses, missing identities and scope, and expected results.
`--purpose timing` requires reliable observed pairs; `--purpose recognition`
checks classification and can support identity-only or intentional start-only
events. Supplied relationship assertions are checked for either purpose.
Unsupported, conflicting, and insufficient evidence remain distinct.
Exit 0 means supported on the observed sample, without asserting general semantic
correctness from match counts. Candidates are never activated automatically.

Use `profile prepare --candidate-output PATH --kind KIND` to save a separate
candidate and report initial validation gaps. Edit and validate it against known
facts, then explicitly select it for subsequent analysis. All candidates may be unsuitable. See the
[profile validation workflow and expected-fact contract](contracts/profile-validation.md)
for source-addressed positive/negative facts, exact pair assertions, inheritance,
redaction, bounds, and scope/capture limitations.

## Resolve a profile explicitly

`profile resolve` is an opt-in, read-only JSON interface, separate from the bounded
grammar inference in `investigate`.

Discovery reports selected identity/digest and provenance,
stably ordered alternatives, per-candidate structural coverage, independent
recognition/timing assessments, scope diagnostics, assertions and unresolved
assumptions. It does not activate a profile or write configuration:

```bash
log-analyzer profile resolve examples/profile-validation.jsonl --kind request --candidate-config examples/profile-candidate.toml
log-analyzer profile resolve examples/profile-validation.jsonl --kind request --candidate-config examples/profile-candidate.toml --expected examples/profile-expectations.json
log-analyzer --profile examples/profile-candidate.toml profile resolve examples/profile-validation.jsonl --kind request
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
semantic sufficiency remain separately visible. `profile validate` continues to
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
[version-1 association](../schemas/profile-association.schema.json): one `preset` or
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

When resolution abstains, `profile prepare` combines sample inventory generation,
rule witnesses, validation and scope/boundary diagnostics in one JSON report:

```bash
log-analyzer profile prepare examples/profile-validation.jsonl --template examples/profile-candidate.toml --candidate-output prepared-profile.toml --kind request --purpose timing --expected examples/profile-expectations.json
log-analyzer --profile prepared-profile.toml profile validate examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
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

Inspect `creation`, `structure`, `adaptation`, `sample_validation`, `semantic_proof` and
`missing_information` separately. Validation runs without expected facts; missing
or partial independent facts cannot verify the requested population and timing
pairs. A successful exit can report an unsuitable candidate or abstention.
There is no automatic repair/retry loop. If a report cannot be saved after candidate
creation, stdout reports the committed candidate and failed delivery; validate the
saved file rather than retrying creation.

`adaptation` describes the **starting profile** against the selected sample:
observed payload/structured-field shapes, representative first/final and error or
unclassified records, role/rule matches, selector failures, missing join keys,
recognized chain roots, step counts and later context. Field paths use JSON Pointer
for payload shapes and `structured:NAME` for flattened fields. Shapes are sampled
(128 paths, depth 8, first three array members), and excerpts are bounded. They
provide evidence for editing a candidate; similar wording or fields do not establish
event meaning. A component rename, changed payload field, or SDK-like custom
grammar can reuse a profile's lifecycle while needing different adapters/selectors.
Keep entity, request and run identities separate, retain scope boundaries, and
declare an identity bridge only when records establish it. Replacing `event_rules`
through inheritance clears inherited investigation policy; redeclare the applicable
roles and relationships when doing so.

Version-1 expected facts can include `links` alongside `records` and `pairs`:

```json
{
  "version": 1,
  "links": [{
    "chain": "capture", "step": "submitted",
    "root": {"input": 0, "line": 1, "row_path": null},
    "source": {"input": 0, "line": 1, "row_path": null},
    "target": {"input": 0, "line": 2, "row_path": null},
    "linked": true
  }]
}
```

For direct relationships, replace `chain`, `step`, and `root` with
`"relationship": "DECLARED_ID"`. `linked:false` asserts that recognized endpoints
must remain separate. An unknown role, missing key, unreachable predecessor,
rejected input structure, or stopped processing fails an assertion with unavailable
evidence; it cannot verify a negative link. Addresses distinguish independent input
ordinals, physical lines, and normalized row paths. Checks use the runtime's scoped
joins and cardinality **before evidence sampling**, including middle polls and late
completions. They establish supported links, not temporal order, duration, or cause.

`sample_validation.relationships` reports assertion status, supported and unasserted
link counts, root witnesses, steps and later context. `not_asserted` means no link
facts were supplied; `passed_on_asserted_links` covers only those facts. Record facts
remain necessary to establish lifecycle meaning. When supplied during resolution,
link facts must pass and cover every observed supported link, in addition to the
existing requested-kind record/pair requirements. This still cannot prove undiscovered
relationships or capture completeness. Relationship processing has separate limits
of 20 million work units, 10 seconds and 256 MiB of accounting reservations; a stop
reports unavailable totals. These limits cover relationship calculation, rather
than parsing, field inventory or the process's total memory.

Adaptation is the agent's bounded recovery procedure: inspect the packet and raw
source witnesses, edit a separate candidate, establish independent positive and
negative facts, validate, then rerun `investigate --profile CANDIDATE` with fresh
evidence. Never derive expected facts from candidate output or activate by match
count. The analysis skill follows this procedure and verifies later completions,
cached outcomes, contrary evidence, and named timing boundaries before answering.

`--witness-limit` bounds representatives (default 20, maximum 100). Representatives
with an atomic string over 512 Unicode scalars or serialized size over 4096 scalars
are omitted whole, preserving exact retained identities, source addresses and facts.
Presentation omissions count size and count limits; outcome metadata, coverage totals
and opaque retrieval identities remain exact. Per-source coverage, including its
parser diagnostic counters and existing diagnostic limits, is exempt from preparation
limits. Use the saved candidate with
`profile validate` and common report cursors for omitted evidence, or `info` for
unparsed input. Preparation itself rejects common budgets/cursors because replay
must not create files. Limits bound presentation, not processing memory/work.

`--redact` protects the report, including copied assertion witnesses. The executable
candidate remains unredacted so its rules retain their meaning; inspect it before sharing.

### Persistent source/profile mappings

`profile resolve` reads the project registry at
`<project-root>/.log-analyzer/profile mappings.json`, then the user registry at
`~/.config/log-analyzer/profile mappings.json` (Windows uses `USERPROFILE` when
`HOME` is unavailable).

The project root defaults to the current directory;
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
insufficient to remember it. The [registry schema](../schemas/profile-mappings.schema.json)
stores selectors, ordered source paths, digests, selected parsers, contract versions
and assertion-digest provenance. It stores no raw log excerpts, assertion values,
credentials, reports or disposable analysis cache. Inspect the metadata before
tracking or sharing it; project configuration and private evidence remain separate.

```bash
log-analyzer --profile examples/profile-candidate.toml profile mappings --project-root . remember examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
log-analyzer profile mappings --project-root . inspect
# Copy entry.id and digest from inspect; replacement requires fresh validation.
log-analyzer --profile examples/profile-candidate.toml profile mappings --project-root . replace examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json --entry-id ID --if-digest DIGEST
log-analyzer profile mappings --project-root . forget --entry-id ID --if-digest DIGEST
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
`profile mappings inspect --output <separate-report-path>`. Inspection save failures
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
`profile prepare --template` (also accepted by compatibility `generate-config`):

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

# Alternatively, prepare a new candidate using an embedded template
log-analyzer profile prepare logs/app.log --template service-api --profile-name my-team \
  --candidate-output service-candidate.toml --kind request --summary

# Prepare a candidate from multiple related chunks; a fresh path is required
log-analyzer profile prepare logs/run-1.log logs/run-2.log --template custom-start \
  --candidate-output split-candidate.toml --kind request --summary
```

Only combine related logs from the same run/session when preparing a profile; mixing unrelated runs can pollute inferred commands/requests/session levels.

For consumer repositories, prefer a tiny wrapper script or Make target that pins `--profile <name-or-file>`. That keeps the binary generic while making repo workflows explicit and repeatable.

### Validate Your Profile (Quick Checklist)

Before relying on lifecycle results, run `profile validate --kind KIND --expected
FACTS.json` with the selected `--profile` and independent known facts.

Inspect
coverage and suitability; these supplementary checks help diagnose specific gaps:

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
