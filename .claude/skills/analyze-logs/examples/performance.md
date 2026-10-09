# Example: Compare a slow run with its baseline

Question: which elapsed intervals differ, even if neither run logs an ERROR?

Run these commands from a repository checkout on the synthetic fixtures. For
real captures, use stable absolute paths and the same intended, validated profile
for each run. Keep independent runs in separate analyses and snapshots. Installed
skills can apply this sequence to their own inputs; the
[fixtures](https://github.com/eirenik0/log-analyzer/tree/main/examples/investigations)
are available in the repository.

Agree on tool-call, output and time budgets. Check capabilities, coverage and
profile suitability for each run before interpreting timing. Every bounded command
returns a page: follow `retrieval.next_cursor` with unchanged arguments plus
`--report-cursor`, within the agreed budgets, until the needed boundaries are
retrieved. Check omissions and stop on unsupported analysis, missing progress,
oversized items or exhausted budgets. Do not automatically request unlimited output.

## Check the executable

```sh
log-analyzer capabilities
```

## Inspect slow-run coverage

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 info examples/investigations/slow.jsonl
```

## Inspect baseline coverage

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 info examples/investigations/baseline.jsonl
```

## Validate the slow run

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 validate-profile examples/investigations/slow.jsonl --kind request \
  --expected examples/investigations/slow.expected.json
```

## Validate the baseline

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 validate-profile examples/investigations/baseline.jsonl --kind request \
  --expected examples/investigations/baseline.expected.json
```

## Inspect the ERROR inventory

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 errors examples/investigations/slow.jsonl
```

## Measure the slow run independently

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 perf examples/investigations/slow.jsonl --op-type request
```

## Measure the baseline independently

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 perf examples/investigations/baseline.jsonl --op-type request
```

## Inspect gaps in the slow-run trace

```sh
log-analyzer --config examples/investigations/profile.toml \
  --report-max-items 3 trace examples/investigations/slow.jsonl --id slow-run
```

## Interpret the evidence

After retrieving all required pages, the parent lifecycle lasts 8000 ms in the
slow run and 1000 ms in the baseline, an elapsed difference of 7000 ms. Cite each
run's real boundaries under its own snapshot identity. An empty ERROR inventory
does not establish normal performance.

Worker intervals overlap, so summing their durations does not measure critical-path
time. The trace's 4000 ms gap is between observations; the logs do not distinguish
CPU work, network delay, scheduling or sleep. Treat possible causes as hypotheses
and seek telemetry that can distinguish them.

A missing completion boundary establishes incomplete captured evidence, not a hang,
deadlock or infinite duration. Check scope, rejected records and capture limits;
intentional start-only events have no measured lifecycle duration. Pair complete
related inputs before applying discovery filters that could remove boundaries.

Return separate investigation contracts for the two snapshots and link them through
comparison provenance. Report the measured difference with its limits, and keep the
cause unknown when the available evidence cannot establish it.

The maintained example runner executes these documented commands, follows pages,
and verifies citations and expected findings. This checks the workflow, not a
model's reasoning. See the [portable workflow](https://github.com/eirenik0/log-analyzer/blob/main/docs/investigation-workflow.md)
for the full evidence and stopping contract.
