# Development reference

Build identity, performance checks, evaluation limits and release practices support reproducible changes. See [[contributing]] for contribution requirements.

## Development

Python evaluation and skill-check scripts read and write UTF-8 text explicitly,
including on Windows systems using an ANSI code page.

See [the contribution guide](contributing.md) for Conventional Commit rules, local hooks,
quality checks, and hosted Codex review. Agent and reviewer guidance lives in
[AGENTS.md](../AGENTS.md).

## Features

The analyzer provides parsing, comparison, filtering, lifecycle analysis and source-backed reports.

- **Structured parsing** - Extracts and parses JSON payloads automatically
- **Semantic comparison** - Compares JSON objects regardless of property order
- **Diff context improvements** - Tracks source line numbers and marks changes as added/removed/modified
- **Advanced filtering** - Include/exclude by component, level, content, or direction
- **Operation lifecycle tracing** - Discover matching IDs/session paths, then verify exact scope and measured boundaries
- **Multi-file session analysis** - Merge and analyze `info`/`perf` inputs across multiple log files
- **Session lifecycle insights** - Profile-driven session tree/completion tracking in `info` (with legacy prefix compatibility)
- **Performance analysis** - Identify slow and orphan operations
- **Agent evidence** - Compact JSON with snapshot-scoped source references, optional masking and explicit omissions
- **Profile-driven customization** - Override parser/perf markers via TOML config or generated templates
- **Flexible output** - Text or JSON format with color and verbosity control

## Build and performance

CI uses the latest stable Rust toolchain. The codebase is verified with Rust 1.99.0. Run formatting, lint, and test checks before submitting changes:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Measuring search and tracing performance

Use optimized builds for timing.

The synthetic benchmark covers overlapping search
context, search without context, and long Rust tracing messages with and without
structured fields. It reports repeated end-to-end timings and checks identical
stdout between binaries, excluding the separately recorded build identity footer.
Save a release binary before editing to compare changes:

```bash
cargo build --release --locked
cp target/release/log-analyzer /tmp/log-analyzer-before
# Make the change, then rebuild.
cargo build --release --locked
python3 scripts/measure-search-performance.py target/release/log-analyzer \
  --baseline /tmp/log-analyzer-before --repeats 3
```

Use `--records`, `--context`, and `--words` to vary input size. These local synthetic
wall times include startup, parsing, and rendering; they do not measure peak memory
or establish production throughput. Search visits each displayed context entry once
after ordering matches. Tracing rejects prose tokens before searching for field values.

To reproduce investigation memory pressure on 8,018 synthetic records (4,009 paired
operations), run the Unix memory probe in a fresh process for each release binary:

```bash
python3 scripts/measure-investigation-memory.py target/release/log-analyzer
```

It reports the processing stop reason, accounted bytes, completed counts, and actual
child peak RSS separately. `--memory-bytes` overrides accounting for diagnosis of an
older build; the default probe uses the executable's unchanged limits. This is a
synthetic workload, not a guarantee for differently sized records or profiles.

### Structured export normalization

Run `log-analyzer schema export.jsonl --samples 3` to preview JSON Pointer paths
and types without processing or decoding strings.

Preview limits are explicit
in JSON (four levels, twenty object fields, three array items); it does not infer
field meanings. Configure a profile explicitly, for example for tuple rows:

```toml
[normalization.fields]
timestamp = "/0"
message = "/1/info/event"
payload = "/1/info"
```

Paths use JSON Pointer syntax, including numeric array indexes. For nested arrays,
set `[normalization] root_path = "/rows"` and `expand_rows = true`. Field paths
are then relative to each expanded row. `decode_paths` explicitly decodes strings
before root selection; `row_decode_paths` decodes strings within each selected row.
No string is implicitly decoded. All mapped fields are required; missing, null,
and wrong-type fields cause that row to be skipped with a diagnostic.

`timestamp_unit` accepts `seconds`, `milliseconds`, `microseconds`, or `nanoseconds`
for integer Unix timestamps, including negative values. Without an explicit unit,
timestamps must be strings. Omitted level/component fields use parser defaults;
unmapped rows must already have the normal object schema. Configuration forces
JSON-lines parsing. Normalization diagnostics appear in analysis `coverage.files`
and text coverage; single-file commands also emit skipped-row diagnostics on stderr.
Provenance retains the original file, physical line, and expanded JSON row path.
Search and trace expose row paths; event and ID matching use only each normalized
record, while original source lines remain available as evidence.
Use `info -j` to inspect coverage before interpreting an investigation report.

### Safe compact text

Compact payload values, messages and field names retain complete Unicode graphemes
within their byte budgets, including combining accents and emoji sequences.

Shortened field names use deterministic `...~2`, `...~3` suffixes when needed and
reserve original short names, so retained values cannot overwrite one another.
If the input already contains `_truncated_fields`, the omission marker gets a
unique numbered suffix too. The existing field, array and depth limits still apply.

### Build identity and capabilities

`log-analyzer --version` includes the package version, short source revision and
build state.

`log-analyzer capabilities` always emits JSON with the full build
identity, schema version, canonical command names, output/parser formats and
built-in profiles. It does not load a profile, so it also works when the configured
profile is unavailable.

JSON reports include `report_metadata` with `schema_version`, `build` and
`active_profile`. Text reports end with a `Build:` line; generated TOML uses a
comment. Control characters in profile names are escaped in text/comments; JSON
retains the full profile name. Bounded error reports count build metadata against the text budget and
retain it when details cannot fit. Text match-count output therefore includes
metadata; use JSON for machine-readable counts.

The source state is `clean` or `dirty` for a Git checkout (`dirty` means tracked
changes), and `unknown` with a null revision for source archives or unavailable
Git identity. Metadata describes the source at compilation time, rather than the
checkout where a binary is later run. Profiles describe the selected parsing
configuration, including the template used by `generate-config`.

Maintained examples live in `examples/commands.json`. `cargo test --locked` runs
them against the test binary. After `cargo build --release`, run
`python3 scripts/check-examples.py target/release/log-analyzer`; release packaging
also checks native binaries and explicitly skips targets that cannot run on the
build host.

## Grounded evaluation baseline

The [evaluation harness](evaluation.md) scores typed factual claims, exact citations,
unsupported causes, abstention and important omissions.

Mandatory CI runs a synthetic
CLI corpus and paired scripted analyzer/search investigations without credentials.
The [first published baseline](../evals/results/baseline.json) verifies the harness;
its optional adapter comparisons remain unmeasured. The separate
[native evidence-workflow comparison](../evals/results/question-plan-compact-native.json)
records historical Haiku/Opus trials against frozen baseline and candidate versions;
the subsequently shortened skill has not been rerun with models.
The new layered smoke separates independently checked tool truth, interpretation
of supplied verified facts, and paired legacy/unified workflows. It includes frozen
held-out variants and retains answers, known partial usage and independent attempt
statuses after failures. Canonical artifact records include parsed severity.
Real-model runs require an explicit provider allocation. The native results preserve earlier format, schema, fixture-publication and workflow
failures. The latest comparison uses the same generic native JSON-schema contract
in both arms at default effort: all 16 answers passed the unchanged factual scorer,
and seven of eight candidate trials completed typed final reviews with anchored
unknowns. Haiku still skipped recovery and final verification in one held-out repeat;
consistent workflow completion is not established. Native format enforcement is a
runtime protocol change shared by both arms, not a skill-only accuracy gain.
Unrestricted prose/causal judgment uses a separately documented calibration and
review protocol, whose review runs remain unperformed. Missing tokens/provider
cost stay unavailable; known partial usage keeps its completeness indicator.

## Repository and release wording

Suggested repository description: **Local evidence engine for AI log investigations:
deterministic parsing, scoped timing and verifiable sources.**

Release introduction: **Log Analyzer helps AI agents investigate failures and
performance problems using compact, verifiable evidence from logs. The local Rust
CLI calculates; the consuming agent chooses queries and explains findings.** Link
the [portable workflow](investigation-workflow.md) and disclose the installed
build's capabilities and evaluation limits. The project name remains Log Analyzer.

## Publishing to crates.io

The **Publish crate** workflow supports GitHub Trusted Publishing with short-lived
OIDC credentials.

It validates an existing stable GitHub release and defaults to a
package dry run. See [setup and publishing instructions](trusted-publishing.md)
for crate ownership, licensing, and the one-time crates.io configuration.
