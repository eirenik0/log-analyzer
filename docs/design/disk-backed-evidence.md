# Indexed evidence storage (issue #86)

SQLite is an opt-in artifact backend; JSON version 1 remains the default and keeps
its reader and public report contracts. SQLite was chosen over a custom offset
index because transactions, uniqueness constraints and ordered B-tree lookup are
maintained by an established library. A single file simplifies retention and
checksum binding; there are no published WAL or journal sidecars. The tradeoffs
are a bundled SQLite dependency, a second storage contract, and a whole-file
hash verification pass on every retrieval. Verification is linear I/O, but does
not deserialize the complete artifact. Indexed lookup follows verification.

The SQLite storage contract is version 1 (independent of evidence JSON version
1). Metadata and collection items retain the same evidence semantics. Nested
membership and sequence collections are indexed separately. Exact-ID lookup
uses an index rather than scanning JSON. The page cache, maximum serialized
item and retained page have explicit budgets. Oversized items fail explicitly;
there is no silent truncation or eviction of retained evidence.

Construction validates the complete logical evidence contract before publishing.
A buffered staged writer and no-clobber atomic rename keep incomplete artifacts
private. SQLite uses DELETE journals, disables mmap, bounds its page cache, and
caps database size. Temporary snapshots and journals are removed on ordinary
failure/cancellation; abrupt termination may leave private staging files that
can be removed. Published artifacts remain until the user deletes them.
Readers copy and hash into a private temporary snapshot before opening SQLite,
so changes to the original after verification cannot affect returned facts.
Disk exhaustion is an explicit write/read failure. Artifact integrity binds the
stored bytes plus snapshot, profile, query and redaction using existing cursors.
SQLite readers do not accept unsupported versions. Existing JSON readers cannot
read SQLite artifacts; choose JSON for older clients. Claims use the legacy JSON
backend until claim verification can consume an indexed store.

## Remaining processing work

This storage change does not implement a disk-backed parser/correlator. Parsed
entries, scoped joins, populations and chains still use the existing cooperative
memory/work/time/record limits and report partial coverage when they stop. No
correlation state is evicted to claim completion. Source streaming and spilling
those working sets remain necessary to satisfy all of issue #86. Measured
retrieval RSS is separate from the conservative processing allocation allowance.

## Retrieval memory experiment

Run `cargo build --locked --release` and
`python3 scripts/measure_indexed_evidence_memory.py`. The script measures each
retrieval process with the OS time utility (macOS bytes or Linux KiB converted
to bytes), keeping cache and record limits at 8 MiB. It retrieves one actual
checksum-bound finding from the synthetic slow fixture while adding unrelated
SQLite storage pages. This isolates retrieval scaling; the padding is not
additional parsed log evidence. It asserts a 96 MiB absolute peak RSS ceiling
and at most 16 MiB growth across the sweep. These are regression thresholds for
this workload, not general process ceilings for arbitrary metadata or records.

Measured on macOS on 2026-10-13, artifact sizes 155,648; 8,556,544; 33,755,136;
and 134,549,504 bytes produced peak RSS 10,272,768; 10,223,616; 10,240,000;
and 10,338,304 bytes respectively. Both thresholds passed. Machine-readable
results are in `docs/evals/indexed-evidence-memory.json`. The large-input
processing RSS sweep remains outstanding, together with correlation spilling.

## Disk and cache planning

`artifact_max_bytes` bounds the published SQLite file through its page limit,
including indexes and separately indexed nested collections. It is not a cap on
all simultaneous disk use: allow space for private staging, a rollback journal,
and a verification snapshot up to the artifact limit. Read-side snapshots use
the system temporary directory. Read failures (including disk exhaustion) return
an error before evidence delivery. Cache limits govern SQLite pages and a separate
conservative JSON page allowance; metadata has its own serialized record cap.
JSON tree amplification and executable/library overhead must be measured as RSS.
Creation still has the existing bounded-by-accounting in-memory logical tree.

The SQL tables are `metadata(value, bytes)`, `collections(path, count)`,
`items(path, ordinal, value, bytes)`, and `identities(path, id, ordinal)`.
Composite primary keys provide ordered collection and exact-ID lookup. Stored
byte sizes allow preflight before loading JSON; SQLite's value-length limit also
protects against inconsistent stored lengths. The private verified snapshot is
opened read-only, with mmap disabled and trusted schema disabled. Creation
validates all logical relationships; retrieval trusts that validation only through
the original report's checksum. It never infers new facts from stored rows.
