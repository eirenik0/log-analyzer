# Profile suitability on sample evidence

`validate-profile` checks the explicitly selected `--preset` or `--config`
against representative logs. It emits JSON, uses the existing deterministic
parser/classifier/pairing engines, and never edits or activates a configuration.
External agents can propose TOML in a separate file and invoke the same command.

```sh
log-analyzer --config examples/profile-candidate.toml --report-max-items 4 validate-profile examples/profile-validation.jsonl --kind request --expected examples/profile-expectations.json
log-analyzer --config examples/profile-candidate.toml --complete-output validate-profile examples/profile-validation.jsonl --kind request --purpose recognition
log-analyzer --config candidate.toml validate-profile sample.log --kind command
```

The maintained example includes positive start classification, negative context,
and an exact 1000-ms start/end pair assertion. Both preset names and inherited or
generated candidates can be checked. Generate an editable candidate with
`generate-config --template TEMPLATE --profile-name NAME`; validate its saved
TOML before explicitly choosing it for subsequent commands. A high match count
never automatically selects a profile. Repeated candidate invocations can all
return unsuitable results; there is no ranking, model call, or wording-similarity
inference of lifecycle meaning.

## Purpose and verdict

`--kind request|event|command` is required. `--purpose timing` is the default;
`recognition` only establishes classification of the requested kind. Parsing and
global classification cover all selected records. Requested coverage is
calculated from requested-kind events and relevant conflicting/invalid records,
so events of another kind cannot establish applicability. Generic unclassified
context does not by itself invalidate correctly supported requested evidence.

`profile_validation.suitability.status` is:

- `supported`: requested evidence supports the purpose on the observed parsed
  sample, supplied expected facts pass, and relevant defects are absent.
- `unsupported`: representative parsed input does not recognize the requested
  kind, supplied semantic expectations fail, or only intentional start-only
  events exist for requested timing.
- `conflicting`: relevant rules disagree; timing also rejects ambiguous boundary
  associations. Recognition retains timing diagnostics without using them in
  its verdict.
- `insufficient_evidence`: empty/unparsed/filtered input, invalid identities,
  identity-only timing, missing boundaries, assumed chronology, or unresolved
  timing scope adequacy prevent the requested conclusion.

Exit 0 means `supported` on this sample. Other verdicts emit the report and exit
1. Invalid configuration, malformed expected facts, and I/O errors exit 1 with
a diagnostic. CLI argument errors use Clap's usual exit 2. A missing boundary
is incomplete observed evidence, not proof that an operation never completed.
A recognized `end_expected=false` start is intentional, not an orphan or measured
completion. Recognition can support such starts or identity-only records;
timing requires reliable paired boundaries. Expected pair facts are checked
when supplied under either purpose.

## Evidence and configuration identity

The report separates input `coverage`, `global_classification`,
`requested_coverage`, expected results, and suitability. `records` contains rule
IDs and mapped semantics, with snapshot-scoped evidence references. The
`effective_rules` section exposes the configured adapters/conditions/mappings;
rule IDs connect each classification to the rule that matched. Text adapters
match their declared patterns; structured adapters use typed conditions.
Classification diagnostics retain invalid or conflicting rules instead of
assuming their intended meaning. The shared metadata contains the effective
profile hash after inheritance, input snapshot, query, build, and redaction state.

`scope_origin` distinguishes explicit rule scope, inherited record fields,
legacy record fields, and unavailable classification. Empty explicit rule scope
inherits `perf.correlation_scope_fields` through the existing parser (default:
`component_id`). Disabling inherited scope or naming unavailable fields exposes
missing scope. A nonempty constant scope alone is not proof of adequate scope.
When one effective pairing key combines differing component identities or
configured record-scope tuples, the report cites witnesses and marks scope
adequacy unknown for timing. Components can legitimately share a lifecycle;
these diagnostics request review, not an invented domain interpretation.

Suggestions point to source records or diagnostic witnesses and ask for review
of a separate candidate. Intended lifecycle vocabulary and domain scope remain
unknown without representative facts. Partial parse coverage, unclassified
records, normalization losses, and upstream capture completeness remain visible;
`supported` is not a claim that every input byte or future lifecycle is understood.

## Expected facts

Use version 1 JSON with `records`, `pairs`, or both. Addresses require a declared
input ordinal (zero-based), physical line (one-based), and normalized JSON
Pointer `row_path` (null for ordinary rows). Missing, ambiguous, or filtered-out
targets fail explicitly. Addresses refer to original source locations before
redaction. Repeated input declarations retain their ordinal scope.

```json
{
  "version": 1,
  "records": [
    {"source": {"input": 0, "line": 1, "row_path": null},
     "checks": {"/status": "event", "/semantics/phase": "start"}},
    {"source": {"input": 0, "line": 3, "row_path": null},
     "checks": {"/status": "unclassified"}}
  ],
  "pairs": [
    {"start": {"input": 0, "line": 1, "row_path": null},
     "end": {"input": 0, "line": 2, "row_path": null}, "duration_ms": 1000}
  ]
}
```

Checks are exact comparisons at supported classification pointers. Supported
fields/types are declared in `schemas/profile-expectations.schema.json`, embedded
as `capabilities.report_schemas.profile_expectations`. Missing fields do not
compare equal to null: `observed_available=false` and a failed result distinguish
absence from an explicitly nullable phase/outcome/identity. The omitted
`end_expected` default is checked as true. Unknown fields, unsupported pointers,
wrong value types, and empty assertion documents are rejected. Expected facts
are supplied ground truth, not conclusions generated from candidate match counts.

The expectations byte digest is reported and binds retrieval even when source,
profile, and expectation filename remain unchanged. Positive and negative
classification assertions and exact pair witnesses remain separate. A supplied
pair duration checks the existing engine's measured milliseconds; validation
does not manufacture a duration for missing or ambiguous boundaries.

## Redaction and bounds

Common report controls apply to validation. Rule inventories, representative
records, diagnostics, suggestions, operations, expected results, and source
records paginate; full-scope totals and verdicts remain mandatory metadata.
Consult `retrieval.collections` for this page's samples. Complete output and
cursor errors follow the shared bounded-report contract. Large items remain
atomic and tiny budgets use the explicit metadata exception.

Redaction precedes budgeting. Expected/observed assertions use their pointer's
field context, candidate condition values use the declared field, and literal
mappings use their semantic target. Thus unmatched candidate values and failed
expectations do not evade selected ID masking. Redacted rule/expectation values
are presentation evidence, not a reusable copy of the candidate configuration.
Response addresses replace hidden row pointers with null plus
`location_redacted=true`; the strict input-facts schema retains original pointers.
Array ID masking also learns individual values for consistent raw/structured-field
masking. Source-location losses remain explicit; masked rule containers can be represented
by a mask label. Redaction retains the limitations described in the README.

Validation operation boundary times retain each paired source record's explicit
offset, including mixed-offset pairs; elapsed durations compare their instants.
Assumed chronology remains insufficient evidence for timing support.

Validation source locations include zero-based `input_ordinal`, so expected pairs
match the declared input occurrence as well as physical line and normalized row.
Repeated inputs still participate in shared correlation; duplicate or ambiguous
boundaries remain diagnostics rather than becoming independent completed pairs.
