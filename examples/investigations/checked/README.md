# Checked findings example

This synthetic capture specifies the presentation contract independently of generated
reports. No source or values come from incident logs.

Expected facts (line numbers are one-based):

- Input 1, session `demo`, capture `alpha`: checkpoint at line 1; acceptance at
  line 3 is intermediate. Capture completion at line 10 is 9000 ms after line 1,
  despite different timestamp offsets. Check outcome at line 9 is `false`.
- The enclosing session reports `Passed` at line 11. It has its own subject and
  citation; no policy is inferred to explain its difference from the check result.
- The `alpha` populations contain two poll sends (lines 4–5), one response
  (line 6, status `Failure`), and two cached failures reused (lines 7–8). Each counts distinct
  source occurrences within the selected input and scoped chain, not unique
  requests, attempts, or network failures. These populations must stay separate.
- Capture `beta` starts at line 12, has acceptance at line 14 and a successful
  check outcome at line 15, but no capture completion. Its completion interval
  is unavailable: completion was not observed within this 15-record input.
  This does not establish a stall, cancellation, failure, or zero duration.
- Source binding and relation checks do not verify the intended question.
  An existing link has `link_exists` assurance; `subject_verified` requires the
  separate claim verifier with independently specified expected subjects.

Run from the repository root, using fresh artifact destinations:

```sh
cargo run --locked -- --profile examples/investigations/checked/profile.toml investigate examples/investigations/checked/events.jsonl --artifact /tmp/checked-text.json
cargo run --locked -- --profile examples/investigations/checked/profile.toml --summary --report-max-items 0 --report-max-bytes 16000 investigate examples/investigations/checked/events.jsonl --artifact /tmp/checked-summary.json
```

The second command returns a small navigation summary with coverage, material
uncertainty, assurance limits and an explicit omitted-finding count. Findings are
retained in the artifact; the supplied retrieval action starts at the first one.
Use `--summary --report-max-items 2` without the byte budget to include both atomic
chain findings and their separate populations. The CLI regressions check both
forms. Neither needs `head -c`.
An impossibly small byte budget explicitly reports mandatory metadata over budget.
These deterministic examples establish CLI behavior, not model accuracy or adoption
of final claim verification.
