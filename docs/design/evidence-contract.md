# Evidence contract (version 1)

This contract extends `report_metadata` and schema version 1. Existing report keys
and their semantics are preserved. New fields are additive; consumers must allow
unknown fields. Removal, renaming, or a change of meaning requires a new report
schema version and migration notes. Evidence contract revisions are advertised
separately in `capabilities.report_schemas.evidence_contract_version`.

Published JSON Schemas are in [`schemas/`](../../schemas):
`report.schema.json` covers the supported CLI JSON variants, including pretty,
compact, redacted, count/row modes and coverage-only unparsed-input output;
`capabilities.schema.json` covers capability discovery;
`investigation.schema.json` describes findings produced by a consuming agent.
The binary embeds all three documents in `capabilities.report_schemas` under
`report`, `capabilities`, and `investigation`. Installed clients can retrieve them
from any working directory without source files or network access. Capability
documents contain static binary content and ignore global redaction/masking flags.
These schemas describe CLI output, not standalone library formatter calls, which
do not load an output context. `generate-config` produces TOML, not JSON.
The conformance tests execute each CLI variant and reject malformed boundaries
and findings; successful structural validation alone does not prove a citation
supports a claim.

## Input and profile identity

`report_metadata.evidence.inputs` records each ordered input's file, byte count,
SHA-256 of the exact byte stream consumed by the parser, parse coverage, and
selected-record count. Original bytes include whitespace and line endings.
`input_id` hashes the supplied path and content hash together: two files with
identical lines are distinct sources. `snapshot_id` hashes the ordered input IDs.
Paths are interpreted exactly as supplied; relative/absolute aliases and a
change of working directory are not interchangeable. Repeated inputs remain
repeated scope and count towards totals. For repeatable investigations, use
stable absolute paths and do not supply the same file twice.

`profile_sha256` hashes canonical JSON serialization of the effective loaded
configuration, after resolving inheritance. It includes parsing, normalization,
classification, timing and correlation rules. Changing profile contents while
retaining its name changes this identity; comments and inheritance structure
alone do not. Rule IDs/profile names remain attached to classified timing
boundaries, and this hash identifies their actual definitions.
`query` records effective command options and filters; `query_sha256` hashes the
unredacted query. Rendering, output destination and redaction flags are excluded.
Profile identity is recorded separately. Presentation limits are query options,
not evidence identity. Non-UTF-8 command paths use an object containing a lossy
`file` display and an `os_bytes_sha256` fingerprint of Rust OS-encoded path bytes;
these fingerprints are platform-specific and distinguish paths with the same
display. Output and configuration destination paths are never serialized into
the query. Source-location strings for non-UTF-8 paths use
`@os-bytes:<fingerprint>:<lossy-display>` so indistinguishable displays remain
distinct inputs. Valid paths beginning with `@` are escaped with `@utf8:`.
These labels identify supplied OS paths; they are not paths to open directly.

References may be reused only when input IDs and profile identity match the
saved manifest. Recompute identities after changing logs, paths or profile
contents. A source hash describes the stream read, not a filesystem lock:
freeze/copy actively written logs before an investigation. It makes no guarantee
that upstream exports or the capture window are complete.

## References and measurements

`evidence_ref` contains a stable opaque `reference_id`, `input_id`, one-based physical `line`, and `row_path`
(JSON Pointer for normalization-expanded rows, or null for ordinary entries).
It is attached to search/process/trace records, extraction row sources, error
samples and span boundaries, performance boundaries and diagnostics, comparison
instances, and event timeline sources. Resolve `input_id` in the manifest,
verify its content hash, read the physical entry starting at `line`, then follow
`row_path` when present. Multiline records start at that line. Extraction array
rows additionally include `expansion.path` (the configured extraction path) and
`expansion.index` (zero-based index). This identifies a derived row within the
original parsed payload, not a second physical source line.

Filtering, sorting and process payload compaction do not change references.
Comparison occurrence indexes and process `idx` are display indexes, not
citations. Comparisons pair occurrences and compare values; they do not establish
causal or lifecycle equivalence. Unique comparison type inventories and grouped
search/extraction counts are aggregates without individual record citations;
retrieve matching search/row output before citing a specific occurrence.
Schema preview samples are a structural inspection, not parsed analysis: their
evidence scope is `not_applicable`, with no input snapshot or record references.

Performance operations retain actual `start_source`/`end_source`, classification
and scoped correlation. Timeline intervals retain `start`/`end` source references
and rule names. Only measured timing has `measured_duration_ms`; inferred sleep
and unknown intervals keep it null. Missing, invalid or ambiguous boundaries
remain diagnostics and never generate a completed operation. Trace
`span_boundaries` identifies the first/last substring-selected match: the elapsed
span is not a correlation proof, a sum of work, or evidence of a root cause.
Trace rows expose year/offset provenance. Inferred years or assumed offsets
make the new boundary kind `unavailable`, with `measurement_ms=null`; the legacy
`total_duration_ms` remains a compatibility calculation, not a usable measurement.
The legacy empty trace duration of zero means no observations, as indicated by
`count=0` and absent boundaries; it is not a measured zero.

Error `blocking_ms` and longest-blocking duration remain legacy estimates of the
span from first error to the last observed session record. New source boundaries
and `timing_semantics=error_to_last_observed_session_record_estimate` identify
those observations. An outcome label does not establish a lifecycle completion.
Agent findings must describe this as an estimate, not measured blocking work.

## Scope, coverage, limits and redaction

Shared `scope` counts parsed and filter-selected records before display limits.
It distinguishes `empty_input`, `zero_filter_matches`, `unparsed_input`, `parsed`,
and `not_applicable`. Parse rejections and normalization diagnostics remain
visible for partial inputs. Event classification, ambiguous pairing, and analysis
applicability live in the command's existing `operation_coverage` or timeline
fields; absence means that analysis was not performed, never a measured zero.
Upstream capture completeness remains unknown.

Command totals and displayed-detail counts retain existing meanings. Shared
`omissions.records` is an omitted count when known, otherwise null. The details
field points to command-specific omissions: errors preserve counts for samples,
patterns and stack frames; performance preserves each collection's omission
counts. Process always compacts payloads and reports omitted records relative to
full selected scope. Aggregates and schema samples have no exhaustive individual
record omission count. For complete original payload evidence, use
`search --payloads`; `process --limit 0` removes the record limit but still compacts
payloads. `perf --top-n 0`, `errors --top-n 0 --complete`, and row/search/trace
commands expose their respective full detail paths. Bounded pagination is tracked
separately in #46; this contract does not claim it already exists.

`redaction.applied` describes explicit report-wide `--redact`.
`legacy_sanitization` separately describes default process/LLM-diff sanitization,
which has narrower coverage. ID masking and field redaction change presentation,
not evidence/input/profile/query digests. Paths and queries in the manifest are
redacted, while opaque IDs, physical line numbers and structural metadata are
preserved. If a normalized row pointer or extraction path contains a masked ID,
its reference retains `reference_id` but reports `location_redacted=true`; the row
pointer becomes null and the expansion path is replaced with `[REDACTED]`. This
is explicit location loss, not an ordinary unexpanded row. Display source pointers
are also masked. Raw source text for records with a redacted normalized location
is replaced by `[REDACTED SOURCE LOCATION]` and `raw_logline_omitted=true`, so
source export keys cannot re-expose the hidden pointer. Timeline `raw` and
operation diagnostic `context` exports follow the same omission rule. Displayed record payloads
are also omitted (null with `data_omitted`/`payload_omitted=true`) in that case.
Redacted query filters are `[REDACTED FILTER]`; the unredacted query hash still
identifies the executed selection without copying arbitrary secret search text. Keep local unredacted reports to resolve opaque references and
map masked paths back to files.
Hashes are identity aids, not anonymization; they can reveal equality across runs.
Redaction is optional and does not guarantee that arbitrary log content is secret.

## Agent findings and runtime behavior

The investigation schema requires explicit kinds: observation, measurement,
hypothesis, contrary evidence, or unknown. Measurements require numeric values,
units, timing semantics, effective profile identity and two actual evidence
boundaries. Hypotheses have confidence and supporting references, and cannot
carry measured values or boundaries. Unknowns carry a reason. The Rust core does
not generate agent hypotheses. Consumers must resolve citations and judge their
support; JSON Schema validates shape, not truth.

JSON stdout is a single structured document; diagnostics go to stderr. Nonempty
unparsed analysis emits a coverage-only JSON report and exits 1. Other runtime
errors exit 1 and may have no report; argument errors exit 2. Empty input and no
matches succeed. Saved JSON via `-o` follows the same report contract as stdout.
