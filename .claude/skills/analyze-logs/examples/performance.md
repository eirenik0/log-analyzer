# Example: Compare retained slow and baseline facts

Question: which elapsed intervals differ, even if neither run logs an ERROR?

Check `capabilities`, establish budgets and use the same validated profile
for these synthetic captures. For real captures, start with automatic detection
and verify comparable semantics. Choose fresh artifact paths with existing parents:

```sh
log-analyzer --profile examples/investigations/profile.toml \
  --report-max-items 5 investigate examples/investigations/slow.jsonl \
  --artifact /tmp/slow-evidence-UNIQUE.json
log-analyzer --profile examples/investigations/profile.toml \
  --report-max-items 5 investigate examples/investigations/baseline.jsonl \
  --artifact /tmp/baseline-evidence-UNIQUE.json
```

Inspect each report's coverage, goal support, omissions and processing stop reasons.
Retrieve needed findings, records and explicit paired memberships from each artifact:

```sh
log-analyzer evidence /tmp/slow-evidence-UNIQUE.json \
  --expected-sha256 SLOW_REPORT_ARTIFACT_SHA256 --collection /findings --report-max-items 5
log-analyzer evidence /tmp/baseline-evidence-UNIQUE.json \
  --expected-sha256 BASELINE_REPORT_ARTIFACT_SHA256 --collection /findings --report-max-items 5
```

Use each report's exact checksum and snapshot-bound cursor; never mix cursors or
identities between runs. Follow pages within agreed budgets and retrieve `/records`
for boundary citations and canonical severity. Retained retrieval reparses neither
capture. If required application semantics are missing, resolve that knowledge
before a new calculation; do not repeat broad commands to reproduce retained facts.

The parent intervals are 8000 ms and 1000 ms, a 7000 ms difference. Cite each run's
real start/end references separately. Worker intervals overlap; their sum is not
critical-path time. The 4000 ms observation gap after workers finish does not
establish CPU work, sleep or a cause. A zero ERROR count does not prove normal
performance. Keep the explanation unknown where the capture cannot distinguish it.

The layered smoke runner checks these source intervals, independent scope, severity
and uncertainty. It does not measure real-model accuracy or cost improvement.
