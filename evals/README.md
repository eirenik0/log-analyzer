# Grounded investigation evaluations

Use Python 3.10+ and a built executable. All committed inputs are synthetic. Mandatory
checks use no model credentials, packages or network:

```sh
cargo build --release
python3 -m unittest discover -s evals -p 'test_*.py'
python3 evals/run.py --binary target/release/log-analyzer --strict
python3 evals/agents.py --binary target/release/log-analyzer
```

`cargo test` also executes the CLI corpus and paired scripted investigations, then
validates returned pages and generated investigation contracts against the actual
binary's advertised schemas. CI runs scorer regressions without credentials.

## CLI corpus and historical migrations

`cases.json` contains 28 desired contracts for text/Rust tracing/syslog/console/JSONL,
coverage, extraction, timing, selection, offsets and Unicode. Unsupported access
and tuple inputs test honest rejection, not parser support. `original-cases.json`
preserves the exact local draft and `migrations.json` records its hash, executable
hash, issue links and passing regression assertions before stale markers were
removed. The collision case retains 2000/3000 ms expectations and binds them to
exact parent scopes; current duration sorting makes array order unsuitable proof.
`scenarios.draft.json` preserves the original four manual investigation drafts;
all four original questions now execute in both arms, including actual split-file
boundaries and the unsupported-source coverage witness.

`PASS`, `FAIL`, `ERROR`, `XFAIL` and `XPASS` remain distinct. An exact issue-backed
signature may produce XFAIL; it never counts as PASS. Default mode fails on FAIL,
ERROR or XPASS; `--strict` also fails on XFAIL. Setup errors exit 2. Manifest mistakes,
changed failure signatures and crashes cannot silently inherit an exemption.
Use `--case 'perf-*'`, `--list`, `--report PATH` or `--timeout SECONDS` for CLI runs.
Timezone tests must actually run on a host honoring the requested TZ setting.

## Executable investigation scoring

Version-2 `scenarios.json` has thirteen questions and covers failure triage, independent slow/baseline runs,
INFO-only delay, reused IDs, incomplete capture, unsuitable profiles, nested
exports, redacted evidence/location loss and instruction-like context. Questions
and public tasks use a typed subject/predicate vocabulary. Expected values, exact
support addresses and the scoring rubric are omitted from participant messages.

A final response has exactly `status` (one status per declared group) and `findings`.
Each finding has `group`, `subject`, `predicate`, `kind`, `value`, and `refs`.
The harness validates passing contracts against the advertised contract-1 schema
vocabulary; unknown schema keywords fail explicitly. Measurements also have ordered `boundaries.start` and `.end`. Predicates are listed
in `investigation.py`; public tasks disclose which questions to answer, never their
expected values or citations. Free-form prose, additional fields, unrelated claims
and predicates are rejected, because this deterministic scorer cannot establish
unrestricted natural-language support. Generated version-1 investigation contracts
retain each group's independent snapshot/profile identity.

Credit requires the expected typed value and epistemic kind, exact scoped support,
resolvable consumed-byte identity, physical line/normalized row, genuine ordered
measurement boundaries, and evidence revealed by that participant's tools. A correct
guess with an invalid, unrelated or unseen citation fails. Incorrect causes fail
and increment unsupported-cause counts. Omitted important measurements or unknowns,
wrong scope and confidence despite missing evidence fail. Expected abstention is
success. Explicit location-loss abstention additionally requires observing loss;
it cannot excuse missing evidence for other claims. This corpus does not support
expansion-address citations or unrestricted semantic judgment.

## Two tool arms and budgets

The same scripted participant reads the same questions/inputs under analyzer and
search/script arms. It is a deliberately limited fixture participant that checks
harness execution; it is not an AI model or evidence of agent quality. Analyzer
requests use existing read-only commands and five-item cursor pages; the baseline
uses literal search, five-record reads/explicit nested rows, fixture-limited classic/Rust tracing
recognition, and a fixed timestamp interval script. The baseline is not a universal log parser.
Pairing uses exact operation/session/ID boundaries; durations never establish cause.

Both arms get the same immutable input scope/profile context, prompt/tasks, 80 tool
calls, 2 MB visible output, and 60 seconds including common measured preflight.
Preflight calls and visible metadata are charged equally. Tool traces, output bytes,
elapsed time and order are recorded. Repeats alternate arm order; filesystem/model
caches and host timing still affect measurements. Baseline redaction is deliberately
limited to the synthetic fixture's token/ID fields and requested row-location loss;
it is not a claim of general redaction equivalence. Both arms preserve original
source identities through a permitted local mapping. Raw tool traces stay local.

Tokens and provider cost are null when unavailable. There is no byte-to-token
estimate or zero substituted for missing usage. Adapter-reported usage is labeled
unverified and totals are null if any turn is missing a measurement.

## Optional same-model comparisons

Supply a **trusted external adapter** which reads one JSON request on stdin and
writes one JSON object frame on stdout per process invocation (no diagnostic text):

```sh
python3 evals/agents.py --binary target/release/log-analyzer \
  --model YOUR_MODEL_ID --configuration non-secret-config.json --repeats 3 \
  --report target/evals/model-comparison.json --adapter /absolute/path/to/adapter
```

Each request contains `protocol_version: 1`, the identical base prompt, task,
model/configuration, arm/tool contract, prior tool history and remaining budgets.
Return exactly one `tool_call` or `final`, with optional `usage.tokens` and
`usage.provider_cost_usd`. Tool requests identify the declared `group`. Analyzer
requests use `tool: analyzer`, `command`, and literal option/value pairs; baseline
requests use `tool: read|search|interval`, `input` ordinal and `offset`, `needle` or
`start`/`end` `[physical_line, row_path]`. Interval requests may also set `end_input`
to another ordinal within the same related group. Both arms may use `tool: profile`
to read the selected fixture profile and its permitted parents. Read/search return `next_offset`; analyzer
pages return their existing retrieval cursor. Final is the typed response above.

The identical adapter argv/model/configuration runs both arms with at least two
repeats. Adapter requests are at most 2 MB and responses at most 256 KiB; the wall
ceiling includes adapter processing and stdin transmission. A complete JSON frame
does not wait for inherited pipe EOF; isolated process-group cleanup is used on
POSIX, and Windows pipe cancellation avoids waiting on a live reader during cleanup. The broker rejects writes, new input paths,
profile changes, arbitrary script execution and shell requests. Adapter executables
share filesystem/environment access and are **not sandboxed**: hidden ground truth
is a protocol boundary for trusted adapters. Keep credentials in provider-specific
environment variables read by the adapter, never argv/config/report files. Do not
commit traces or unsanitized model responses. No embedded provider runtime is added.

## Published baseline and limits

[baseline.json](results/baseline.json) records the first sanitized scripted run,
with build/binary, input/profile/corpus, harness/prompt identities and diagnostic
costs. Create an aggregate with `python3 evals/publish.py REPORT OUTPUT`. Inspect
optional model configuration before sharing; the publisher is a projection, not
a universal secret detector.

There is currently **no executed model comparison** and no measured model variance,
accuracy improvement, token savings or provider-cost improvement. Scripted success
establishes harness/scorer regression gates only. First real paired model runs should
establish repeat variability and evidence/cause/abstention baselines before choosing
quality targets. Small public fixtures do not establish production coverage,
acquisition completeness, arbitrary prompt-injection resistance or throughput.
