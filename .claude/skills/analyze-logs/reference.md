# Resolve investigation gaps

Use the executable's `--help` for syntax and embedded schemas for report contracts.
This reference adds decisions for incomplete evidence; it is not another command catalog.

## Processing versus presentation

Use `--summary` for concise JSON across commands. For paginated analysis and
evidence it caps displayed items at five (or a smaller requested limit); profile
preparation caps representative witnesses. Schema and mapping summaries use a
`summary_version: 1` wrapper: inspect `report`, `omissions`, `limitations`, and
`next_steps`. Remove `--summary` for full detail, or follow a supplied cursor.
After a mapping mutation, inspect the mapping instead of repeating the mutation.
`--summary` conflicts with `--complete-output` and the compatibility text format.
Scalars, coverage and diagnostics can be large; use supported report budgets for
size limits. Saved reports have the same presentation as stdout; artifacts retain
the evidence independently of the summary.

Inspect `processing.stop`, per-input coverage and consumed/parsed counts first.
Pagination exposes retained results; it cannot recover records never processed.
Repeating the same input and limits repeats a cutoff. Retry with revised limits
only within the agreed budget, preserving partial facts and the unprocessed scope.
`memory_limit` measures accounted data, not process RSS; measure peak RSS separately
when investigating RAM consumption.

Retrieve retained collections using `evidence`, the exact artifact
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

Establish one absolute project root and use `investigate --project-root ROOT --summary`.
Relative `--profiles-dir` paths resolve from that root; log, explicit profile and
artifact paths still resolve from the working directory. Start with automatic
detection unless the user supplied a profile. Inspect the
selected name, origins, effective hash, sample coverage and discovery diagnostics.
Under redaction, use `report_metadata.profile_selection` for status and coverage;
hidden profile names and paths remain unverifiable.
Detection considers built-ins and `ROOT/config` (or `--profiles-dir`); a unique matching
grammar does not prove intended semantics. Use `--profile base` for generic facts.
Structural coverage or an `info` disclaimer cannot establish lifecycle support.

For a semantic gap, inspect `profile resolve` candidates or create an editable
candidate with `profile prepare`. Validate recognition, identity, scope, phase and
timing boundaries against independently known assertions with `profile validate`.
Counts, similar wording, labels and negative-only facts cannot choose a profile.
Missing structural parsing support cannot be repaired by inventing lifecycle rules.
Apply only a justified profile and keep unsupported goals explicit. Manage saved
mappings only when requested, with independent proof and inspected entry digests
before replacement or deletion.
Read-only `profile resolve --project-root ROOT` uses saved mappings only after
current revalidation. A remembered selection is not a permanent semantic proof.
Renamed copies with otherwise identical effective configuration share a detection
candidate; all source hashes remain available. Changes to actual rules remain distinct.

## Decision guidance

`capabilities --summary` avoids loading schema catalogs. `investigate --summary`
returns a versioned navigation document, not a complete evidence report. It includes
coverage, goal support, binding hashes, an artifact location and state-specific
`guidance.next_actions`. The full report exposes the same guidance and keeps its
existing evidence contract. Follow the action relevant to the question; do not loop
over unchanged diagnostics or mechanically execute every suggestion.

The summary displays up to five findings; its retrieval action resumes `/findings`
after the displayed items. If retention fails, available partial findings remain
inline with displayed/total counts.
Full-report guidance resumes omitted findings. Record retrieval starts independently
at zero. Continue each collection with its own checksum-bound cursor. Redaction
preserves safe instructions but withholds executable arguments containing paths.
An impossible byte budget is reported as `mandatory_metadata_over_budget`; it never
means all evidence was delivered. An unavailable artifact has no retrieval action.

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
