# Investigation contract examples

These generic synthetic documents specify investigation report v1 and retained
artifact v1.

Browse the [fixture files](../examples/investigation-contract/). These are
hand-authored contract fixtures; the [[architecture#Architecture#Retained investigations|implemented runtime]]
produces reports and artifacts through `investigate` and retrieves them through
`evidence`. Existing Rust performance reports supply the fixtures'
source identities and supported intervals. The retained effective profile comes
from `examples/investigations/profile.toml` after default/inheritance resolution.

Each `*.report.json` links its exact `*.artifact.json` bytes by SHA-256. The
`output-limited` report refers to the complete supported artifact. Retention is
`unspecified` in these fixtures. Runtime artifacts use `until_deleted` retention
as described in the [contract](contracts/investigation-contract.md#retained-artifact-and-historical-retrieval).

- `supported`: paired lifecycles, physical record counts and the current percentile method.
- `insufficient`: missing completion is unknown, not proof of failure.
- `conflicting`: overlapping boundaries reject unambiguous full-input pairing.
- `unsupported`: nonempty unparsed input does not produce a zero-error conclusion.
- `processing-limited`: exact facts about a retained prefix with a partial full-input assessment.
- `output-limited`: completed analysis with all detailed findings omitted under a tiny budget.
- `redacted`: arithmetic is retained while source/rule checking is explicitly unavailable.
- `measured-zero`: two genuine boundaries at the same timestamp.
- `zero-filter-matches`: parsed source with no selected records, distinct from unparsed input.

`cutoff.jsonl` has a start and end followed physically by an out-of-order overlapping
start. Its first two records appear pairable; the full engine correctly rejects that
pair. The processing-limited artifact retains the historical prefix despite the
current file containing additional evidence. `zero.jsonl` supplies a genuine zero.

Run `cargo test --locked --test test_investigation_contract` for shape, identity,
calculation and artifact checks. The [contract specification](contracts/investigation-contract.md)
documents semantics, deferred checks and compatibility. Byte digests for source
inputs describe consumed bytes; an artifact's stored digest describes artifact bytes.
