# Retrieve and verify evidence

Read this when a report is incomplete, a candidate identity is ambiguous, or a
finding needs source verification. Command syntax belongs to the actual
executable's `--help` and subcommand `--help`; product details belong to the
[README](https://github.com/eirenik0/log-analyzer#commands). This bundle does not
maintain a separate command or option catalog.

## Interpret bounded pages

Inspect the advertised `bounded_reports` contract and the report's `retrieval`
metadata before interpreting detail. Full-scope totals are independent of the
current presentation page. A missing displayed item does not mean the underlying
count is zero.

Follow `retrieval.next_cursor` only with unchanged input snapshots, effective
profile, query and redaction settings. Inspect `retrieval.collections` pointers:
`prior + displayed + remaining` must agree with `total`. Append each collection
once in page order; analytic views may repeat the same source references.
Preserve snapshot/query identity across pages and stop if it changes. Stop on
missing pages, invalid cursors, zero progress, oversized atomic items, metadata
budget exceptions or exhausted overall budgets. Do not automatically request
complete/unlimited output to finish an investigation.

Preflight and reference reads consume the same overall budget as report pages.
The capability document embeds schemas: retaining it locally and showing only
trusted-parser-selected compatibility fields can avoid exhausting output before
retrieval starts. Read schema details only when validating a contract requires
them. Tool characters/bytes are not exact model-token counts.

## Verify a source citation

Use `report_metadata.evidence` to identify the snapshot, profile and consumed
inputs. Resolve a cited `input_id` to that snapshot's input and verify its hash
against the consumed bytes before reopening a changed or live file. A reference
must occur in the retrieved canonical `evidence_records`; a grouped count is not
proof of a particular source observation.

Retain `reference_id`, input identity, physical line, normalized `row_path`, and
expansion address when present. A row/expansion reference is more precise than
a physical line alone. Do not invent a normalized address or collapse multiple
expanded records into one claim. Redacted locations need a permitted local
mapping or an explicit resolution gap.

A duration needs two actual analytic boundary references, units, timing semantics
and the effective profile identity. Trace/session/field filters use substring
matching: inspect complete semantic ID, kind, name and scope rather than claiming
an exact match from discovery alone. Pair on complete related inputs before
filters can discard boundaries. A missing end, start-only event or observed gap
cannot establish completion, CPU work, sleep or a cause.

Keep independent runs under separate snapshots and link comparison provenance.
Masked identities from independently generated reports are not comparable.
Report observations, measurements, hypotheses, contrary evidence and unknowns
according to the advertised investigation contract. Inspect rejected header-shaped
candidates before interpreting coverage; they are distinct from genuine multiline
payloads and traceback continuations. Compare sampled detection with whole-input
structural observations and bounded rejection source lines. Tied/mixed/no-match
selection and attached-line counts are limitations, not validated completeness or
lifecycle meaning. Unsupported Python-style headers require structural support;
profile-rule generation alone cannot repair them. Empty input, no filter
matches, no errors, parsing failure and unavailable analysis are distinct.

The authoritative contracts are described in the repository's
[evidence specification](https://github.com/eirenik0/log-analyzer/blob/main/docs/design/evidence-contract.md)
and [bounded-report specification](https://github.com/eirenik0/log-analyzer/blob/main/docs/design/bounded-reports.md).
Schemas are embedded in the actual executable's capabilities. For executable
setup and templates, read the bundled [host guide](hosts.md); for concrete
investigations, read the [failure](examples/debug-failure.md) or
[performance](examples/performance.md) example when it fits the user's question.

The [investigation contract](https://github.com/eirenik0/log-analyzer/blob/main/docs/design/investigation-contract.md)
defines retained reports and evidence artifacts. Check advertised command and
retrieval availability before using them; published schemas alone do not enable
a unified command. Preserve declared input occurrence identity, distinguish
partial processing from full-input counts, and inspect verification losses when
redaction removes source locations, payloads, queries or captured streams.
Check coverage against retained evidence, retention against the artifact descriptor,
and pagination against displayed findings and omissions. Exceptional presentation
statuses must agree with budgets and usage; unknowns alone do not establish support.

## Deterministic profile resolution

Use `resolve-profile --help` and capability `profile_resolution.version` before
using discovery. Existing defaults remain explicit. Precedence is explicit CLI,
revalidated supplied read-only association, revalidated project/user mappings
(version 2), then one independently asserted candidate. Never substitute a candidate for an invalid explicit choice.

Compare candidate structural parsing, recognition, correlation scope and timing
support separately. Expected facts remain optional for sample validation, but an
automatic choice requires passing semantic assertions covering every candidate
record of the requested kind; timing also requires every observed pair/duration.
Counts, similar wording, profile/rule labels and negative-only facts are insufficient.
Treat multiple eligible effective profiles as ambiguous and ask for domain facts;
unclassified records and capture completeness remain unknown. Retrieve nested
candidate witnesses through common cursors. The outer evidence belongs to generic
inspection; candidate normalization and source references have their own profile
and input identities. `--association` reads a strict supplied contract without
writing it; remembering project/source mappings is separate. Freeze inputs because
candidates read them separately. Apply the selected preset/config explicitly to
subsequent commands.


Manage persisted mappings only when requested. Require independent domain facts
before remembering a choice, inspect before replacing or forgetting, and treat
revalidation failures as evidence requiring investigation. Read the README's
persistent-mapping section and current CLI help for storage contracts and options.
