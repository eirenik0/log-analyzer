# Investigation report contract version 1

Version 1 defines a bounded investigation report and the retained evidence needed
to check it. The Rust core owns parsing, classification, correlation and calculation;
agents may add explicitly attributed hypotheses. Logs and instruction-like strings
are evidence, never instructions. This contract implements issue #66; the command,
artifact storage and processing-limit enforcement are tracked in #71.

`capabilities.report_schemas.investigation` embeds the report schema and
`evidence_artifact` embeds retained artifact contract 1. The `investigation` schema accepts both the existing agent-authored result shape
and the retained report shape under version 1. Existing result fields retain their meanings.
`investigation_contracts` advertises versions and explicitly says whether the command
and artifact retrieval are implemented. Publishing a schema does not advertise a
working command. These schemas are standalone Draft 2020-12 documents.

## Report identity and occurrences

The report retains `report_metadata` and its existing version-1 evidence manifest.
The new schemas reuse its exact definitions; no existing field changes meaning.
The ordered manifest identifies consumed input streams, active/effective profile,
query, producer build, parsing coverage, selection and redaction. Read the
[evidence contract](evidence-contract.md) for path aliases, non-UTF-8 paths, repeated
inputs and original reference rules.

An occurrence is `{snapshot_id, input_ordinal, evidence_ref}`. Its population key is
`(snapshot_id, input_ordinal, evidence_ref.reference_id)`. A repeated input declaration
can therefore contribute another occurrence without inventing another source.
Physical records, normalization-expanded records and extraction rows are separate
entities; extraction expansion does not create another physical line.

Reference identity does not establish correlation scope. Scopes explicitly list
input ordinals, the effective selection, declared/processed extent, correlation
fields, capture completeness and analysis completion. Independent runs remain
separate unless independently supported rules establish a relationship. Identical
resource or request suffixes, paths, filenames or languages do not establish one run.

## Automatic profile inference

Without an explicit profile selection (`--profile`, or compatibility `--config`/`--preset`), `investigate` probes bounded prefixes of the same
captured bytes later analyzed. The effective query records candidate observations,
sample extents and selection/fallback status in `execution.profile_selection`.
This grammar inference includes built-ins and bounded TOML discovery in the declared
project root's `config` directory (current directory by default)
(or `--profiles-dir`), with configuration deduplicated by `analysis_sha256`, which
excludes only `profile_name`. Full effective hashes and labels remain on origins. Candidate
origins and inherited-source digests are retained in the query; the effective selected
configuration is retained without reloading. Invalid/incomplete discovery is an
explicit selection gap. This inference does not claim independent semantic validation;
assertion-based `profile resolve` remains a separate interface. Explicit overrides
bypass probing. Detection work/time and scratch memory share processing budgets,
with scratch released between probes; its record counts are separate from analysis
records. `parse_passes` includes both `detection_parse_passes` and
`analysis_parse_passes`. Retained retrieval still performs no source parsing.
A capture stop that prevents shared inference marks every declared scope affected;
completed earlier captures retain their generic facts but cannot claim completed
analysis. Discovery's global directory limit permits 256 entries plus one sentinel
entry to detect exhaustion; reaching that sentinel stops ancestor sibling walks.

## Assessment, processing and presentation

`capabilities --summary` advertises compatibility without embedded schemas.
`investigate --summary` projects the final report into `brief_version: 1`, described
by `investigation-brief.schema.json`. Processing and artifact contracts do not
change. The summary displays up to five findings and supplies
retrieval from the first omitted finding. Hidden `--brief` retains the older
navigation-only projection and starts retrieval at zero. When the artifact is unavailable, it retains the full
report's displayed partial findings inline so positive evidence is not discarded.
Full-report guidance resumes from its presentation cursor instead. Guidance version
1 distinguishes discovery, semantic, processing and retention failures and contains
only analyzer-owned instructions. Path-bearing literal arguments are withheld
under redaction. Presentation counts/sizes include guidance; an impossible budget
remains an explicit metadata-over-budget exception.

Assessments are per goal and scope. `supported` means the referenced finding answers
the declared goal on that scope with applicable evidence; it does not mean no failure
was found. `insufficient_evidence` names missing observations or applicability,
`conflicting` retains incompatible interpretations/witnesses, and `unsupported`
names unsupported input or semantics. Mixed goals may have different assessments.

`processing.status` is complete, partial or failed. `stop` identifies the stage,
reason, applicable limit and affected scopes. Resource limits and usage are nullable
when unavailable; null is not measured zero. Input progress distinguishes complete
consumption, a retained prefix and unread input. A consumed-stream digest describes
only those bytes: a prefix must never be labeled a complete original file.

Capture completeness and `scope.analysis_completion` are separate. A supported
`declared_input` assessment requires both complete capture and completed analysis.
A parse/classification/correlation/calculation cutoff makes affected scopes partial
or not performed. Explicitly completed independent scopes may remain complete.
Artifact-write failure can preserve completed analysis while retention is partial
or unavailable. #71 must implement the checkpoints that justify these declarations.

A partial fact is exact only for its declared processed population. Unread evidence
can invalidate a pairing, resolve an incomplete lifecycle or alter distributions.
This revision deliberately does not serialize full-input lower-bound estimates;
a future monotone calculation must establish that guarantee explicitly. An observed
missing end is not proof of a hang or failure.

Presentation separately states its budget units, displayed and omitted findings,
and per-collection counts. Byte/character counts cover the final serialized document
including newline; characters are Unicode scalars, not model tokens. Finding bundles
are atomic. Tiny budgets retain mandatory metadata with an explicit exception;
oversized bundles do not silently lose their required witnesses. See
[bounded reports](bounded-reports.md) for the unchanged legacy interface.

## Facts and support

Every finding has an ID, author, scope, material limitations, verification guarantees,
and representative excerpts with occurrence references and explicit clipping counts.
Observations, measurements, hypotheses, contrary evidence and unknowns remain
separate. Only an agent may author a hypothesis, with explicit confidence and support.
Contrary evidence identifies its target finding. Unknowns retain a reason and may
have no source witness when the needed observation is absent.

Measurements require milliseconds, named observed boundary semantics, actual start
and end occurrences, full source-offset timestamps, source year/offset provenance,
rule IDs, effective profile identity and a clock-relationship declaration. A negative
interval is rejected before millisecond truncation, including negative submillisecond
intervals. Explicit offsets do not establish synchronized clocks, causal latency,
configured sleep, CPU time or blocking work. Unknown clock relationships and capture
limitations must remain visible. Legacy error blocking estimates do not become
measurements under this contract.

Calculated facts declare a population, counted entity, exclusions, identity/rule
provenance, completeness and an exhaustive membership reference/digest. Counts use
cardinality of the processed population. Paired lifecycles retain the existing
`perf.operations` meaning; logical-operation grouping, attempt grouping and distinct
resources require explicit applicable rules. A record count cannot be relabeled as
failed operations or distinct resources.

Distributions name the contributing measurement IDs, sample count, unit, statistic
and method. Their sample set must equal their population's contributing measurements.
The compatible percentile method sorts durations and selects index
`floor(n * percentile / 100)`, exactly the current performance implementation.
Changing the algorithm requires an explicitly different method/compatibility policy.

A displayed positive finding includes every essential boundary/support witness,
its calculation/population definition and all material limitations. It need not
inline exhaustive members or diagnostics. Those collections remain in the artifact,
with exact counts and references. An excerpt is a prefix of its retained record’s `message` or `raw_text`;
`omitted_characters` counts the remaining Unicode scalars in that field.
Applied redaction omits retained `raw_text`, `message`, and `fields` payloads
(`null`, `null`, and `{}` respectively) and sets `data_omitted: true`. Source
verification is unavailable. Excerpts use only `[REDACTED SOURCE]` or a declared
prefix of that marker; their omission counts describe the marker, not original logs.
Arbitrary replacement text cannot prove redaction without the original inputs.
Clipping an excerpt is a declared prefix projection;
it cannot alter the claim, fact, support identity, limitations or verification.

## Retained artifact and historical retrieval

Artifact contract 1 retains the shared metadata, processing state, scopes,
assessments, full findings, records, population memberships, effective configuration,
captured streams and scoped event sequences. It contains no self-digest; the report
stores SHA-256 of its exact serialized bytes separately from original input digests.
Membership digests hash compact JSON serialization with lexicographically ordered
object keys, UTF-8 values and no whitespace or trailing newline (the existing Rust
`serde_json::Value` serialization). Member array order remains significant.

Captured streams use UTF-8 strings or byte arrays. Original consumption identities
remain provenance even when redaction changes or removes saved bytes. Stored-byte
digests identify decoded retained content. Retained records preserve original
source-offset timestamps, physical lines, normalized paths and expansion identities.
Event sequences carry scope, completeness, classification/rule provenance and stable
ordering by observed timestamp, input ordinal and record ordinal; unavailable
timestamps sort last. The order is not a causal or clock-synchronization claim.

An artifact is complete only for its declared captured/processed scope. Capture,
analysis completion, artifact integrity and upstream completeness are distinct.
Retained history remains readable after the originals change or disappear. Reading
history does not assert that current originals match: checking current originals
is a separate revalidation step before a new analysis or reuse against current data.

Retention states its actual policy: unspecified, until explicit deletion, or an
expiry. Raw-byte archival and a default lifetime are not selected by this contract.
#71 must choose/document storage location, privacy, cleanup and cancellation behavior.
Artifact storage belongs outside tracked project/profile configuration.

Requested redaction applies after analysis and before persistence/output. Each
artifact/finding/excerpt declares artifact integrity, arithmetic reproducibility and
independent source/rule checking separately. Source checking is unavailable when
original source bytes, locations, classification data or effective rules are lost;
loss reasons are mandatory. A redacted configuration is not a reusable effective
profile. Arithmetic may remain checkable from retained measurements/members without
being independently reproducible from the original logs. Hashes identify content;
they do not authenticate artifacts or anonymize logs.

Artifact retrieval is a separate versioned interface, not a changed interpretation
of `--report-cursor`. It targets findings, source references, populations and event
sequences without reparsing. A cursor binds stored-artifact digest, snapshot,
effective profile/query, producer semantics, redaction policy and retrieval version.
Budget changes may vary presentation, never the selected analysis. Changes/deletion
of original inputs do not invalidate historical artifact retrieval. Changed,
deleted, corrupt or unsupported artifacts produce explicit errors. A new analysis
revalidates inputs, configuration and analysis contracts rather than reusing stale
facts. Redacted artifacts cannot reconstruct discarded original bytes or grant a
less restrictive output view.

## Validation and compatibility

Validate JSON shape against the installed schema, then call Rust
`investigation::validate_relations(report, optional_exact_artifact_bytes)`.
It checks source/occurrence identity, scope and support references, captured digests,
retained membership, duplicate resource identities, calculation arithmetic,
source-offset boundaries, processing cutoffs and presentation reconciliation.
It returns which artifact checks ran and which relations were deferred. Without
artifact bytes, omitted finding/sample IDs require explicit retrieval targets;
a successful summary-only check is not exhaustive evidence verification.

These checks do not establish event meaning, replay classification, authenticate
files, synchronize clocks, prove upstream completeness or judge unrestricted prose.
The producer must supply valid scoped classifications/calculations; #71 will exercise
this contract through the shared Rust engines. Shape validation alone cannot prove
that a citation supports a claim.

Existing CLI outputs, report/evidence versions, agent-result fields, capabilities keys
and stateless bounded retrieval keep their meaning. No `investigate` or saved-artifact
retrieval command is implemented by #66. Consumers validate either shape against the advertised
version-1 schema and must not infer feature availability from its existence. Text and JSON in #71 must render the same selected model,
including scope, assessment, limitations and omissions.

Synthetic [examples](../../examples/investigation-contract/) cover supported,
insufficient, conflicting, unsupported, output-limited, processing-limited, redacted,
measured-zero and zero-filter-match results. The cutoff fixture contains a later
out-of-order start that invalidates the apparent prefix pair. Tests also cover
repeated declarations and mutation of still-schema-valid facts and artifact contents.
These are hand-authored contract fixtures. `investigate` now produces the retained
report/artifact shapes, with runtime regressions checking their relational invariants.

Distinct resource identities use only the population’s declared identity fields
and correlation scope; extra descriptive fields cannot create another resource.

Manifest validation recomputes query, input and ordered snapshot identities using
existing evidence-contract serialization. When paths or queries are redacted or
masked, original query/input identity checks are explicitly deferred; the ordered
snapshot digest remains checkable. Reported non-null input-byte usage must equal
the checked sum of consumed input bytes. A supported assessment references at least
one finding. Measured intervals require distinct boundary occurrences, even when
the observed timestamps are equal.

Coverage validation reconciles per-input parsed/selected totals and manifest status,
checks semantic counters against scoped selected records, and bounds retained source
occurrences by selected parse coverage. Supported assessments require a positive
observation, measurement or calculated fact; unknowns and hypotheses alone do not
establish support. A bounded page may defer that check to explicitly retrievable facts.
Applied redaction requires redacted artifact content and unavailable independent
source/rule verification; original captured byte streams and effective rules must
not be persisted. Descriptor retention must match the retained artifact's policy.

Applied redaction omits captured stream data and its stored-byte digest entirely;
a changed declared original hash is not proof of sanitization. Presentation collection
counts reconcile prior, displayed and remaining items against totals, actual arrays
and retained artifact collections. A complete presentation has known zero omissions.

Finding-level source verification requires retained rules and the finding’s dependent
source data even when it has no excerpts. Unrelated record losses do not invalidate a
measurement. Empty-population and absence claims require retained scoped captures. Contrary-evidence targets omitted from a page may
be explicitly retrieved; report-only validation records that deferred relation.

Semantic event counters are bounded by selected records and their containing
populations. Ambiguous and rejected counts overlap unmatched counts and are not
summed as disjoint sets. Raw-source comparison preserves blank continuation lines.

Record usage, when available, equals parsed-entry coverage. Declared mask fields
without applied redaction do not disable identity verification. Contrary evidence
can target only a finding in the same scope. An unavailable artifact declares its
integrity unavailable, as do every finding and excerpt referring to it. Other
verification dimensions remain independently assessed. Integral statistics (sum, minimum, maximum and percentiles)
use exact JSON integers; sums use checked arithmetic. Only means use floating point.

Source verification indexes each consumed text stream once. Physical witnesses and
normalized records share a logical source: count normalized paths per physical line
when present, otherwise count that line once. Multiple excerpts of one occurrence
are matched by their own text projection and verification guarantees.

Applied redaction in retained reports omits the entire query with command
`[REDACTED QUERY]` and filter `[REDACTED FILTER]`, and replaces both input and
coverage file paths with `[REDACTED PATH]`. Original identity hashes remain provenance;
this conservative retained contract does not attempt to prove arbitrary transformed text.

Presentation limits require serialized usage for each declared size budget.
`item_limit_zero` requires zero item budget, no displayed items, and remaining items.
`oversized_item` requires a size budget, fitting metadata, no displayed items, and
remaining items. `mandatory_metadata_over_budget` requires an exceeded size budget
and no displayed items. A `page` makes progress while declaring omissions.

Declared serialized usage cannot be smaller than a conservative JSON syntax lower
bound plus its final newline. Integer spellings preserve mandatory digits; floating
point values use the shortest equivalent decimal or scientific spelling. Exact pretty-printed wire size requires
the original report bytes; relational validation cannot reconstruct those spaces.

Assessments are unique by `(goal, scope_id)`; duplicate or conflicting answers for
the same question are invalid. Different goals can assess the same scope.
