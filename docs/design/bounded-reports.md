# Bounded reports and evidence retrieval

Common report controls support `info`, `search`, `extract`, `perf`, `trace`
(including configured timelines), `process`, comparisons, and `errors`. Discover
these controls through `capabilities.bounded_reports`. Existing output and
bounded-error options keep their behavior when common controls are absent.

```sh
log-analyzer --preset eyes --report-max-chars 12000 --report-max-items 10 search examples/synthetic.jsonl --payloads
log-analyzer --preset eyes --report-max-items 10 --report-cursor CURSOR search examples/synthetic.jsonl --payloads
log-analyzer --preset eyes --complete-output search examples/synthetic.jsonl --payloads
```

Replace `CURSOR` with the preceding report's `retrieval.next_cursor`. Keep input
paths, profile contents, filters, command selections, sorting and redaction
settings unchanged. Budgets may change between pages. `--output` receives the
same bytes as stdout, including cursor errors. Common controls imply compact
JSON even if the original command defaults to text; `schema`, `capabilities`
and `generate-config` do not accept them.

## Units and exceptions

`--report-max-bytes` measures serialized UTF-8 bytes; `--report-max-chars`
measures serialized Unicode scalar characters. Both include JSON escaping,
metadata, cursor, separators and the final newline. Characters are neither
bytes, grapheme clusters nor exact model tokens. Simultaneous limits all apply.
`--report-max-items` limits atomic presentation items across collections. Pages
default to 100 items when only a byte/character limit or cursor is supplied.

Scope, parse/classification/analysis coverage, totals, applicability, capture
limitations, redaction and retrieval state remain in every page. High-cardinality
component buckets and designated sample/detail collections are pageable.
Collection totals preserve ambiguity/diagnostic counts before their samples are
shown. Metadata may exceed a very small budget: the report explicitly sets
`retrieval.metadata_over_budget = true`, emits zero presentation items, and
retains its offset. JSON is always a complete document. This exception also
applies to cursor-error diagnostic metadata.

Records, comparison groups, error clusters and nested payloads are atomic.
A next item that cannot fit produces `oversized_item` if no item was emitted;
its cursor remains at the same offset. Increase the budget or use complete
output to retrieve it. `--report-max-items 0` returns `item_limit_zero` without
advancing. Stop on these states; repeatedly requesting the same insufficient
budget cannot make progress. A page containing items may stop before an oversized
next item and return the next cursor normally.

## Ordering and reconstruction

`retrieval.collections` identifies each JSON Pointer, its complete total, prior
items, items displayed in this page, and remaining items. Arrays concatenate in
page order, maps merge by key, and designated strings concatenate. An item's
identity is `(collection pointer, original ordinal)`. Items retain their complete
nested values and source references. This explicit registry prevents arbitrary
payload arrays from becoming presentation collections.

`evidence_records` provides each selected parsed source record once, ordered by
timestamp, then declared input ordinal, then physical record order (including
normalized row order). It follows the global filter and trace substring selector.
Repeated declared inputs remain repeated scope. Comparison sides remain distinct.
Analytic views can intentionally cite the same source more than once, such as a
performance operation also appearing in threshold violations or a timeline event
appearing in an interval. Reconstruct each view separately; these repetitions
are not additional source records.

Native filters and sorts run before pagination. Comparison key ties, difference
paths and performance source ties have deterministic ordering. Aggregate counts
and timing calculations retain the full declared scope. Legacy `omitted` values
retain their meaning before common pagination; their basis is explicit under
`report_metadata.evidence.omissions`. Common pagination omissions are in
`retrieval.collections`. Fields such as error `clusters_displayed` describe the
complete analytic view before common pagination; consult retrieval counts for
this page's displayed items.

Cursors bind the input snapshot, effective profile, effective query, redaction
policy, retrieval version and complete redacted report digest. Changed files,
paths, profile rules, selection, redaction or report behavior return
`invalid_cursor`, a structured document and exit status 1. Restart retrieval;
never silently reuse old offsets. Cursor identities are consistency checks,
not authentication or access control. Filenames and output locations use the
existing evidence contract's OS-path handling.

## Complete output, redaction and resource limits

`--complete-output` uses the same collections and ordering with pagination
disabled. It conflicts with common budgets/cursors. Common mode overrides native
`process --limit`, `perf --top-n`, and `errors --top-n` to zero and disables
process message/payload compaction. It retains semantic selections such as
operation type, orphan-only, warning inclusion and comparison differences.
Legacy error sample/stack/output clipping conflicts with common mode because
clipped content cannot be recovered; use its original bounded workflow separately.

Redaction runs over the complete view before budgeting, so masks agree across
pages and complete output. Redacted normalization locations preserve their
existing explicit loss markers. Default process/LLM comparison payload
sanitization still applies unless `--no-sanitize` is supplied; canonical source
records also sanitize payload/structured fields and omit raw lines in this mode.
Sanitization and optional redaction retain their documented limitations.

Pagination bounds returned output, not parsing memory or CPU. Each page reparses
inputs, calculates full-scope analysis and hashes the complete redacted report.
There is no database or persistent index. Packing measures the skeleton once
and each considered item plus retrieval metadata, without repeatedly serializing
the growing full report. See the reproducible synthetic resource measurement
below; it is a local observation, not a scalability or model-cost claim.

## Reproducible resource measurement

Build the release binary and run:

```sh
cargo build --release --locked
python3 scripts/measure-report-retrieval.py target/release/log-analyzer --records 10000
```

The script creates generic synthetic input in a temporary directory, traverses
all pages, verifies source references are returned exactly once, and reports
input/output sizes, first-page latency, complete-output latency, cumulative
traversal latency and child-process peak RSS. It uses 1000-item/2-MiB pages.
Results depend on hardware, build mode, corpus and page size. Peak RSS covers the
maximum binary invocation, not the Python driver's memory. Measurements are
reported in `docs/evals/report-retrieval-resources.json` with build/input identity.
