# Implemented design decisions

These choices are visible in the current code. [[philosophy]] sets the broader direction; [[checked-analysis]] describes proposed changes rather than completed behavior.

## Classify once and share semantics

The parser retains owned classification evidence so performance, investigations and profile validation consume the same rule decisions.

[[src/event_rules/mod.rs#CompiledEventRules#classify_owned]] retains rule provenance
and conflicts. This avoids each consumer guessing event meaning from message text.
[[contracts/event-classification]] defines the classification boundary.

## Preserve uncertainty at boundaries

Scoped identity and explicit timestamp provenance prevent plausible but unsupported joins and durations.

[[src/perf_analyzer/mod.rs#analyze_performance_checked]] reports ambiguous or missing
boundaries. [[src/investigation_runtime/chains.rs#endpoint_interval]] refuses
ambiguous, negative or unsupported intervals. [[src/processing.rs#Budget]] records
partial processing; its conservative memory accounting is not a hard process RSS
limit. [[regression-tests]] anchors these distinctions in tests.

## Retain evidence separately from presentation

Historical evidence must remain retrievable when report pages are short or original logs change.

[[src/investigation_runtime/artifact.rs#retrieve]] validates saved bytes and resolves
retained collections. The command-report [[src/report_budget.rs#Policy]] instead
binds cursors to recomputed reports. Neither path allows a cursor to silently
switch input, profile or query. [[contracts/bounded-reports]] explains the limits.

## Check relations separately from meaning

Schema shape, relational consistency, intended subject and prose support are different checks.

[[src/investigation.rs#validate_relations]] checks identity, arithmetic and retained
support. [[src/investigation_runtime/claims.rs#subject_check]] compares a supplied
subject with retained facts. Successful checks do not establish that the caller
selected the intended event or that an explanation is causal. Automatically copied
claim drafts remain candidates requiring review.

## Keep profile adaptation independent

Profiles declare domain meaning; matching more records does not prove that a proposed interpretation is correct.

[[src/profile_validation.rs#analyze]] compares supplied expected classifications,
pairs and links against observed results. [[src/profile_preparation.rs#run]] writes
a separate candidate; it does not activate it. Independent expectations and
negative cases belong to the caller, as described in [[contracts/profile-validation]].
