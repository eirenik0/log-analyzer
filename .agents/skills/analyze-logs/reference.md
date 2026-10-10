# Resolve investigation gaps

Use the executable's `--help` for syntax and embedded schemas for report contracts.
This reference adds decisions for incomplete evidence; it is not another command catalog.

## Processing versus presentation

Inspect `processing.stop`, per-input coverage and consumed/parsed counts first.
Pagination exposes retained results; it cannot recover records never processed.
Repeating the same input and limits repeats a cutoff. Retry with revised limits
only within the agreed budget, preserving partial facts and the unprocessed scope.
`memory_limit` measures accounted data, not process RSS; measure peak RSS separately
when investigating RAM consumption.

Retrieve retained collections using `investigation-evidence`, the exact artifact
checksum and snapshot-bound cursor. Check displayed, prior, remaining and total
counts; append each collection once. A missing displayed item is not a measured
zero. Stop on missing pages, changed bindings, no progress or oversized items.
Preflight, reference reads and retrieval share the overall budget; bytes and
characters are not exact model-token counts.

## Profiles

A profile supplies parsing, normalization, lifecycle and correlation rules.
`--profile NAME_OR_FILE` selects a built-in or TOML file. Exact built-in names
select built-ins; use `./eyes` for a file named `eyes`. File profiles can inherit
built-ins or relative parent files through `extends`.

Start with automatic detection unless the user supplied a profile. Inspect the
selected name, origins, effective hash, sample coverage and discovery diagnostics.
Under redaction, use `report_metadata.profile_selection` for status and coverage;
hidden profile names and paths remain unverifiable.
Detection considers built-ins and `./config` (or `--profiles-dir`); a unique matching
grammar does not prove intended semantics. Use `--profile base` for generic facts.
Structural coverage or an `info` disclaimer cannot establish lifecycle support.

For a semantic gap, inspect `resolve-profile` candidates or create an editable
candidate with `prepare-profile`. Validate recognition, identity, scope, phase and
timing boundaries against independently known assertions with `validate-profile`.
Counts, similar wording, labels and negative-only facts cannot choose a profile.
Missing structural parsing support cannot be repaired by inventing lifecycle rules.
Apply only a justified profile and keep unsupported goals explicit. Manage saved
mappings only when requested, with independent proof and inspected entry digests
before replacement or deletion.

## Source and lifecycle evidence

Bind citations to `report_metadata.evidence`: snapshot, input identity, consumed
hashes and effective profile. Retain occurrence identity, `reference_id`, physical
line, normalized `row_path` and expansion address where present. Resolve actual
retrieved records; a grouped count alone does not support a particular observation.
Changed current sources do not rewrite retained facts. Redacted locations require
a permitted mapping or an explicit verification gap; never reconstruct hidden fields.

A measured duration needs two actual boundaries from the same scoped occurrence.
Discovery filters may use substrings; verify full kind, name, ID and scope.
Inspect later ends, reused IDs, outcomes and contrary/unclassified records before
claiming completion or absence. Conflicts, invalid classifications and unavailable
recognizers prevent complete semantic counts. An unmapped outcome is a gap, not
proof of absence. A later success does not erase an earlier failure or prove a retry.
Canonical severity is `records[].fields.level`.

Keep independent comparisons under separate snapshot/profile bindings. Masked IDs
from different reports are not automatically comparable. Cite each run's real
boundaries and preserve the distinction between elapsed intervals, observed gaps,
overlapping work and causal explanations.

For supplementary scripts, name the precise unanswered question and why retained
analysis cannot answer it. Example: “Request boundaries are recognized, but the
profile defines no resource-to-request relationship.” A later analyzer cutoff
cannot explain a script that was already written earlier.

The [investigation contract](https://github.com/eirenik0/log-analyzer/blob/main/docs/design/investigation-contract.md)
is authoritative. Deterministic fixture checks verify commands and evidence;
they do not establish improved model adherence, accuracy or token savings.
