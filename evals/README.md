# Grounded investigation evaluations

## Profile discovery and recovery

`discovery.py` starts with a question, log paths and project context, **without an
injected profile**. Its separate nine-case suite covers project/nested-directory
discovery, conflicting outcome rules, current saved-mapping revalidation, terminal
evidence beyond the first page, grammar beyond the detection prefix, processing
cutoffs, unparsed input and unsupported semantics. The broker permits discovery,
profile/fact reads, resolution, validation, bounded investigation reruns, and
checksum-bound evidence retrieval. Project/user registries are isolated from the
operator's real home. Profile/mapping mutations are unavailable to participants;
the saved-mapping fixture is prepared separately from the measured workflow.

```sh
python3 evals/discovery.py --binary target/release/log-analyzer \
  --report target/discovery-smoke.json
python3 evals/discovery.py --binary target/release/log-analyzer --view full
```

`--view brief` (the default) invokes `investigate --summary`; `--view full` uses
the full investigation report. `brief` names the evaluator arm and report contract,
not the legacy CLI flag. `cargo test` runs the summary smoke and validates the
emitted briefs against JSON Schema.
Python regressions reject wrong profiles, wrong outcomes, unsupported completion,
unseen/stale terminal citations, missing limitations and scope/command escapes.
Saved Windows mapping selectors can use verbatim drive/UNC paths; the broker
normalizes them only after filesystem resolution and retains project containment.
Failed Rust smoke tests print each failing case and its errors from the saved report.
The scorer requires retained evidence retrieval. It does not grade unrestricted
prose, causes or native host skill activation. Scripted passes prove harness and
CLI execution only; they do not measure model quality or instruction adherence.

The existing trusted adapter protocol supports model trials:

```sh
python3 evals/discovery.py --binary target/release/log-analyzer --repeats 3 \
  --model YOUR_MODEL_ID --allocated-budget-usd YOUR_POSITIVE_ALLOCATION \
  --configuration non-secret-config.json --skill-mode discover --view brief \
  --report target/discovery-model.json --adapter /absolute/path/to/adapter
```

Compare `--skill-mode none|entrypoint|discover` and `--view full|brief`, keeping
model configuration and budgets fixed and alternating run order. `none` withholds
skill resources, `entrypoint` supplies the actual entrypoint on every turn, and
`discover` exposes the installed bundle through bounded resource tools. The latter
measures protocol resource discovery, **not native Codex/Claude/Pi activation**;
test those hosts separately. Scripted runs reject skill modes other than `none`.
Artifacts, prompts and traces stay local; adapters share filesystem access and are
trusted, not OS-sandboxed. Provider usage is adapter-reported, never estimated from
output bytes. Missing spend stops further paid calls through the shared adapter.

Reports retain failed attempts, typed answers, output bytes, tool calls, repeated
investigations, skill reads, elapsed time, known/unknown usage, corpus/skill/harness/
binary hashes and each case's all-trials-pass result. This last result is a small
sample consistency observation, not a population reliability guarantee. Fixtures
and scripted strategy are public development cases; use independently authored
held-out incidents before setting production quality targets. Use the existing
prose-review protocol to check unsupported causal implications in natural answers.

This separation follows the interface-testing approach of
[SWE-agent](https://arxiv.org/abs/2405.15793), the procedural-skill comparison in
[SkillsBench](https://arxiv.org/abs/2602.12670), and repeated-trial reliability in
[τ-bench](https://arxiv.org/abs/2406.12045). Their results motivate experiments;
they do not establish reliability for this analyzer.

Use Python 3.10+ and a built executable. All committed inputs are synthetic. Mandatory
checks use no model credentials, packages or network:

```sh
cargo build --release
python3 -m unittest discover -s evals -p 'test_*.py'
python3 evals/run.py --binary target/release/log-analyzer --strict
python3 evals/agents.py --binary target/release/log-analyzer
python3 evals/layers.py --binary target/release/log-analyzer --repeats 2
```

`cargo test` also executes the CLI corpus and paired scripted investigations, then
validates returned pages and generated investigation contracts against the actual
binary's advertised schemas. CI runs scorer regressions without credentials.

## Three separate evaluation layers

The [published layered smoke](results/layers-scripted.json) records 98 scripted
attempts with corpus, harness and binary hashes.

`layers.py` reports tool correctness, verified-fact interpretation and the complete
workflow separately. It reuses the strict typed scorer without treating that scorer
as a natural-language judge. [The prose/causal protocol](prose-review.md) supplies
calibration anchors, independent reviewers and reconciliation rules. Reviewer
calibration and actual unrestricted-prose review have **not been run**.

`layer-cases.json` freezes source/profile/fact-packet hashes, incident families,
development/held-out splits, compatibility eligibility and visible exclusion reasons.
Ten historical cases, two completion development cases and four public held-out
variants cover failures, INFO-only timing, scoped identities, incomplete capture,
unsuitable profiles, nested records,
redacted locations, mixed grammars and unparsed input. The four variants change
identities, scopes, UTC offsets, durations and capture boundaries. Historical split-
file timing remains explicitly ineligible because unified inputs are independent;
named redacted identities use a different disclosure contract. A duplicate fixture
question remains in the historical smoke instead of duplicating paired totals.
Exclusions are semantic compatibility decisions, not removals of failed attempts.

The default two repetitions now produce **112 scripted attempts**: 16 direct tool checks,
32 interpretation runs and 64 paired workflow runs. The published 98-attempt report
predates the two completion development cases and remains a historical result.
Tool checks compare direct Rust records, severity, source-backed intervals/boundaries
and capture uncertainty with
independently authored fixture facts. They do not grade through the artifact adapter.
`verified-facts.json` contains source/operation packets, not expected answers, statuses,
omission labels or scoring hints. Only interpretation participants receive those
packets; reconstruction/profile tools are disabled. Its arithmetic and source
addresses are checked independently. Workflow participants receive the original
question/scope and obtain facts through delivered tools only.

Legacy uses bounded info/perf/profile-validation pages; unified uses one investigate
per group and checksum-bound retained pages. The broker only joins explicit operation
memberships to measurements and canonical records. It does not parse raw logs, pair
events, recover redacted fields or manufacture absent values. Parsed severity is
retained as `records[].fields.level`; older artifacts lacking it fail explicitly.
Artifact/item validation uses advertised schemas; retrieval page envelopes use
separate pinned broker field/hash/count/cursor invariants. Rust integration
cross-checks actual delivered items and reports plus schema composition regressions.

Both workflow arms receive the same supplied explicit profile, immutable context,
80 calls, 2 MB output and 60 seconds. Common capability/scope preflight, including its
info invocation, bytes and runtime, is charged to every arm. The broker records
profile-resolution status, delivered validation/support checks, repeated queries,
custom interval scripts, command invocations, calls, elapsed time, output bytes and
model usage. Automatic profile resolution and participant attention are not measured.
Engine instrumentation omitted by an older or redacted response stays null. Counts
of participant engine passes exclude common preflight; command invocations remain
separate from unknown engine work.

Execution status, answer availability, budget compliance, schema validation and
factual score are independent fields. Received final answers and valid incremental
usage are saved before later process-exit, protocol, deadline, budget or validation
failures. A schema-valid wrong answer can have validation passed and factual score
failed. A failed execution can still have a received factually correct answer.
Failed attempts and their known output/cost stay in aggregates. Tool output bytes
count received subprocess stdout, including nonzero exits, malformed output and
captured timeout prefixes, plus serialized local broker/fact output. No bytes-to-token
estimate is used. Known partial usage has a completeness label and missing-response/
missing-attempt counts; missing usage is null, never measured zero.

Real models remain opt-in with trusted adapters, fixed model/configuration and at
least two repeats. An explicit total provider allocation is required:

```sh
python3 evals/layers.py --binary target/release/log-analyzer --repeats 3 \
  --model YOUR_MODEL_ID --allocated-budget-usd YOUR_POSITIVE_ALLOCATION \
  --configuration non-secret-config.json --adapter /absolute/path/to/adapter
```

The protocol reports **incremental usage per response**, not cumulative conversation
usage. The broker includes remaining tool/wall/provider budgets, preserves known
overspend, and halts later paid calls when spend is missing. Missing spend means
unknown compliance, not measured overspend. Provider reporting is unverified; the
adapter must enforce its requested remaining allocation before incurring new cost.
No provider runtime or inference is embedded in the Rust analyzer. Adapters share the
filesystem: withholding truth/rubrics is a trusted protocol boundary, not an OS sandbox.

Use `--report PATH` for local traces/answers and `--publish PATH` for the sanitized
projection. Published smoke results establish reproducible harness checks only;
public held-out scripted repetitions establish neither model reliability nor broad
accuracy, time, token or cost improvement. Codex/Pi/Claude packaging is reused from
the existing portable skill work; interactive host behavior remains untested here.

## Completion checklist evaluation

The skill checks processing coverage, final lifecycle evidence and outcome/contrary
records before an absence or completion claim. `tests/test_completion_evidence.rs`
exercises ten synthetic captures: a paginated late success, a failed end, processing
cutoff, a rejected terminal record, an unrecognized result, an unsuitable profile,
start-only/end-only captures, a different-scope end, and failure followed by success.
These are evidence availability and interpretation constraints, not a test of whether
a model follows the skill.

The layered corpus additionally asks for positive `completion` claims on late-success
and failed-terminal fixtures. Here `completion=true` means a unique observed
start/end lifecycle, irrespective of its outcome; it does not mean successful or
whole-run completion. Existing incomplete fixtures require `unknown`. The scripted
participant preserves ambiguity when a later unmatched boundary exists. The strict
scorer requires the correct boundaries and outcome citations: blanket abstention,
false absence and unsupported completion fail their corresponding cases.

For an opt-in model comparison, `layers.py --skill-file PATH` supplies the exact
UTF-8 entrypoint (up to 64 KiB) in the adapter's `base_prompt` on every turn. It
records its SHA-256 and byte length as `skill_input` and includes the delivered
instructions in each participant prompt hash and initial input-byte accounting.
It loads no linked references; this tests the entrypoint under the broker's declared
tool contract, not full installed-host behavior. Scripted runs reject this option.
No credentials or provider calls are needed for regression checks.

To compare old and new instructions, freeze both entrypoints and run the **same new
harness, binary, corpus, model and configuration**, changing only `--skill-file`:

```sh
python3 evals/layers.py --binary target/release/log-analyzer --repeats 3 \
  --skill-file /absolute/baseline-SKILL.md --report target/evals/skill-baseline.json \
  --model YOUR_MODEL_ID --allocated-budget-usd YOUR_POSITIVE_ALLOCATION \
  --configuration non-secret-config.json --adapter /absolute/path/to/adapter
python3 evals/layers.py --binary target/release/log-analyzer --repeats 3 \
  --skill-file .agents/skills/analyze-logs/SKILL.md --report target/evals/skill-checklist.json \
  --model YOUR_MODEL_ID --allocated-budget-usd YOUR_POSITIVE_ALLOCATION \
  --configuration non-secret-config.json --adapter /absolute/path/to/adapter
```

Each command requires its own explicit provider allocation. Alternate baseline and
candidate run order across batches and compare like-for-like arms and cases, retaining
failed attempts. Evaluate false unfinished claims, false completion claims, unnecessary
abstention, outcome correctness and citations separately. Use the prose-review protocol
for natural-language claims and the broader coverage-gap cases; typed fixture success
alone cannot establish skill effectiveness. No such model comparison has been run.

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
estimate or zero substituted for missing usage. Adapter-reported incremental usage is labeled
unverified. Complete totals are null when a turn is missing, while known partial
totals and completeness/missing counts are retained even for failed attempts.

## Optional same-model comparisons

Supply a **trusted external adapter** which reads one JSON request on stdin and
writes one JSON object frame on stdout per process invocation (no diagnostic text):

```sh
python3 evals/agents.py --binary target/release/log-analyzer \
  --model YOUR_MODEL_ID --allocated-budget-usd YOUR_POSITIVE_ALLOCATION \
  --configuration non-secret-config.json --repeats 3 \
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

Evaluation sources and fixtures use LF checkout bytes via `.gitattributes` so
Windows checkout conversion does not invalidate the preserved draft hash or corpus
identities. Hashes always describe the actual consumed bytes.

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
