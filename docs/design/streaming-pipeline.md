# Streaming investigation pipeline

The pipeline yields parsed records to a consumer. Storage is separate from parsing;
SQLite is not an artifact format or a prerequisite for consuming records.

```mermaid
flowchart TD
    A[Input files] -->|64 KiB chunks, hash, byte limit| B[Private temporary snapshots]
    B --> C[Bounded profile samples]
    C --> D[Selected profile]
    B -->|bounded physical / multiline records| E[Parser visitor]
    D --> E
    E -->|one normalized row at a time| F[Global filter]
    F --> P[Selected record collection: still resident in CLI]
    P --> G[Correlation spool with threshold-triggered SQLite indexes]
    G --> R[Memory-budgeted report collections]
    B -->|stream and escape captured bytes| H[Staged JSON artifact]
    R --> H
    H --> I[Digest and source-witness validation]
    I --> J[Atomic no-clobber publication]
    J --> K[Private checksum-verified retrieval snapshot]
    K --> L[File offsets and reference indexes]
    L --> M[Bounded semantic validation passes]
    M --> N[Selected items streamed to staged response]
```

## Implemented boundaries

- Source capture reads 64 KiB at a time, hashes the exact retained prefix, validates
  UTF-8 across chunk boundaries, and retains a 64 KiB profile sample per input.
  Capture scratch reservations are released after reading. Input byte limits bound
  temporary source disk usage; temporary files are private and automatically removed.
- The parser consumes bounded physical lines and carries the current multiline
  candidate across read boundaries. A prefix cutoff never flushes an unfinished
  record. Automatic parser detection bounds buffered sampling by physical-line
  count and accumulated bytes; blank continuations and physical addresses survive.
- `visit_log_file` emits records synchronously to a fallible callback. Returning an
  error applies backpressure immediately; no subsequent records are emitted. The
  parser does not retain a collection of emitted records. Physical/multiline record
  size is explicit. Normalization retains at most twenty diagnostics before an
  explicit error; structural diagnostics already have a bounded sample.
- Normalization holds the bounded physical JSON envelope and emits one expanded
  row at a time. This is not an incremental parser for an arbitrarily large JSON
  value: `record_max_bytes` remains the envelope limit. Row paths, physical lines,
  offsets and raw witnesses remain attached to every record.
- Investigation filters emitted records before its analysis collector and moves
  them into correlation without the previous second parsed-record clone. Discarded
  records release their reservations immediately, including expanded rows. Counts
  still describe parsing before filters. Correlation state survives all read chunks;
  a chunk ending never establishes completion or absence.
- Artifact capture data is written directly from snapshots through bounded JSON
  escaping (or a bounded byte-array writer for non-UTF-8 captures). The retained
  logical tree contains capture metadata, not complete capture strings. The writer
  enforces the artifact byte cap. A staged file is published only after source,
  identity and relational validation; failures preserve explicit unavailable status.
- Construction-time raw-witness verification walks snapshot files in source order
  with a bounded line cursor. Multiple normalized rows can revisit the same physical
  record. It does not construct an index of every line in each input.
- Retrieval hashes into a private snapshot before parsing or emitting retained
  facts. Temporary file indexes address JSON nodes and cross-references; bounded
  projections reuse the contract validator, with separate global membership,
  uniqueness, ordering and calculation checks. Version-1 JSON and its existing
  cursors remain compatible.

## Remaining state boundary

The investigation consumer still retains selected `LogEntry` values and calculated
report collections under its existing conservative memory budget. It emits a
partial outcome if that budget is exhausted. Pending correlation indexes now spill,
but completed results and retained report collections do not. Retrieval now avoids
whole-tree materialization. The report-construction stage still lacks constant RSS,
so this PR does not close issue #86's end-to-end out-of-core requirements.

Pending correlation records and per-key summaries use append-only temporary payload
files. The resident index spills to SQLite at a fixed allowance; SQLite stores
only offsets, identities and sort keys, with a 2 MiB page cache per state store,
`mmap_size=0` and disposable journaling disabled. It is never the external artifact
format. Payload plus database page limits are checked on every append; failed
writes cannot produce a successful partial stream. Temporary directories are
removed on drop, including cancellation and consumer errors.

Correlation makes two ordered passes. The first establishes inferred-year and
whole-key overlap decisions, including late ambiguity that invalidates earlier
pairs. The second holds one pending boundary and emits zero, one or two source
records per decision. Records require unique input/line/row addresses; duplicate
addresses and inconsistent input-to-file mappings fail explicitly. A single
correlation stream owns its effective profile and is consumed by finalization.
The CLI uses this correlator, but collects its completed operations and unmatched
records for the existing report builder. The library consumer can remain bounded.

## Measurement

Build and run the synthetic yielding-consumer probe:

```sh
cargo build --release --locked --example stream-records
python3 scripts/measure-streaming-memory.py target/release/examples/stream-records
```

Every size runs in a fresh process, parses real JSONL records, checks the record
count and source digest, and records OS-reported child peak RSS. The probe consumer
counts and discards records; it measures the parser boundary, not correlation,
artifact retrieval or an end-to-end investigation. Saved results are in
[`../evals/streaming-parser-memory.json`](../evals/streaming-parser-memory.json).
For investigation state growth, use the existing
`measure-investigation-memory.py` separately; its accounting is not RSS.

The same script can exercise capture, filtering and JSON artifact publication with
zero selected records and a fixed 8 MiB accounting budget:

```sh
cargo build --release --locked --bin log-analyzer
python3 scripts/measure-streaming-memory.py target/release/log-analyzer --investigate
```

[`../evals/streaming-filtered-investigation-memory.json`](../evals/streaming-filtered-investigation-memory.json)
records that separate experiment. It verifies complete parse coverage and artifact
publication; the excluding filter means it does not exercise growing correlation
state. No input is padded with unrelated database pages.

On the recorded macOS arm64 runs, source size grew from 66,000 to 66,000,000 bytes.
The parser probe stayed below 10 MiB peak RSS; the excluding-filter investigation
stayed below 14 MiB while publishing complete artifacts. These are measured stage
results on the synthetic corpus, not a bound for retained correlation state.

### Selected correlation workload

```sh
cargo build --release --locked --example stream-correlations
python3 scripts/measure-correlation-memory.py target/release/examples/stream-correlations
```

This separate probe selects every parsed record, writes every start before any
completion, and checks exact emitted-record counts, pair counts, each duration,
source ordering, scope, and input hashes. It uses a 1 MiB resident index allowance,
two 2 MiB SQLite page caches, a 256 KiB serialized-record cap and a 4 GiB disk cap.
Every size runs in a fresh worker. The script fails above a 64 MiB absolute RSS
ceiling or a 24 MiB growth tolerance across the sweep; these are reproducible
measurement thresholds, not a cross-platform allocation guarantee.

The recorded 2,000 / 20,000 / 200,000 selected-record sweep used 13.3 / 15.6 /
14.6 MiB peak RSS and correctly completed 1,000 / 10,000 / 100,000 late pairs.
Results are in [`../evals/streaming-selected-correlation-memory.json`](../evals/streaming-selected-correlation-memory.json).
It measures the yielding parser plus real growing correlation state. It does not
measure full CLI evidence construction or retrieval. Retrieval has a separate
selected-record sweep below; report construction still prevents an end-to-end
bounded-memory claim.

## Bounded artifact retrieval

The retained format stays version-1 JSON. Every request copies and hashes a private
immutable snapshot before validation, then builds 64-byte JSON offset entries in a
temporary file. Navigation and exact finding-ID lookup use offsets; strings and
large arrays remain in the snapshot. Reference maps use fixed on-disk hash slots
and exact key comparisons, without mmap or SQLite. No persistent cache or format
migration is required; old readers still read the same JSON. This trades additional
temporary I/O and a full verification pass per request for compatibility and bounded
working memory. It does not promise constant-time retrieval from an unverified file.

Records are checked in chunks of at most 128 (about 1 MiB, plus one bounded item).
Memberships, sequences, assessments and distribution sample IDs are visited one at
a time. Global uniqueness, exhaustive membership hashes, source dependencies,
scope, sample ownership and event order use disk indexes. The existing contract
validator checks bounded projections against actual retained witness records;
checks that depend on the full collection run separately. In particular, a projected
empty population never becomes a new absence claim. Exact mean calculation uses
four external radix passes with a 512 KiB histogram to preserve the legacy sorted
floating-point summation order. Original source hashes are cached only for private
snapshots; raw-record verification is cached by the exact record digest. Redacted
stored bytes never establish original-source availability.

The default item/metadata/witness allowance is 8 MiB. JSON offset disk usage is
capped at 1 GiB. Validation/reference state has a 2 GiB allowance divided into
sixteen shares; fewer than sixteen indexes are live at once, but an individual
index can exhaust its share before the total allowance. Structural limits include
128 nesting levels, 64 KiB object keys/reference keys, 32 captured inputs, 128 root
fields and 1,024 fields per indexed record/finding/population. A 180-second
cooperative deadline and 500-million work allowance bound validation; blocking
filesystem operations and one bounded JSON operation can overshoot the deadline.
Use the `--artifact-max-*` flags documented in CLI help to adjust applicable limits.

The private snapshot plus decoded sources require at most twice the artifact read
allowance. Page payload scratch is capped at the larger of 16 MiB or eight times
that allowance, with a second staged delivery copy. These are separate disk
allowances from the offset and validation indexes. Captures and output are copied
through bounded buffers. `--complete-output` spools items instead of collecting a
page in RAM. Nested `/memberships/N/members` and `/sequences/N/events` remain usable
when their parent array is too large to return as one atomic item. Validation or
storage failures return no verified page and publish no destination file. Temporary
files are removed on drop; requested output uses atomic no-clobber publication.

### Selected artifact retrieval workload

```sh
cargo build --release --locked --bin log-analyzer
python3 scripts/measure-artifact-retrieval-memory.py target/release/log-analyzer
```

The generator selects every record and completes all starts only after the last
start. Generation uses a larger processing allowance and is **excluded** from the
retrieval RSS measurement. Each fresh retrieval child either returns every selected
record with `--complete-output` or looks up an exact measurement ID. The harness
compares every returned item to the artifact, checks selected/pair counts and source
hashes, and enforces a 64 MiB RSS ceiling with 24 MiB growth tolerance across sizes.
The measured process uses fixed 8 MiB item and 2 GiB index/state allowances. Raw
results are saved in
[`../evals/streaming-selected-retrieval-memory.json`](../evals/streaming-selected-retrieval-memory.json).
These are measured synthetic thresholds, not an OS-enforced RSS ceiling.
