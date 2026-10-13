---
lat:
  require-code-mention: true
---
# Evidence regressions

These existing tests anchor important invariants in [[domain]] and [[architecture-decisions]]. Lat checks the references; executing the tests checks the behavior.

## Subject binding

Changing the expected snapshot, scope, identity, root or endpoints must contradict a supported link rather than verify a nearby event.

Covered by [[tests/test_investigation_runtime.rs#subject_bound_links_reject_adjacent_names_scopes_ids_and_endpoints]].

## Checked finding presentation

Synthetic CLI fixtures preserve separate outcomes, counts, endpoint intervals, assurance and missing evidence in text, full JSON and bounded summaries.

[[tests/test_checked_presentation.rs#maintained_example_agrees_in_text_json_and_bounded_summary]]
checks independently specified facts in [[examples/investigations/checked/README.md]].
The same test suite covers failed ends, missing ends, ambiguous starts, renamed
identities, independent inputs, redaction, tiny budgets and rejection of mutated
arithmetic, source citations and event subjects. Existing coverage and zero tests
below also exercise the checked publication boundary.
[[tests/test_checked_presentation.rs#aggregate_subject_verification_preserves_unavailable_prerequisites_when_paged]]
keeps aggregate assurance unavailable for redaction, artifact limits and mixed
UTF-8/non-UTF-8 input, including pages with no displayed findings.
It rejects independently mutated uncertainty counts and assurance against retained
artifacts while accepting older reports that omit the optional aggregate.
[[tests/test_checked_presentation.rs#checked_assurance_rejects_levels_and_bindings_that_disagree_with_evidence]]
rejects altered assurance levels and coordinated binding changes.
[[tests/test_checked_presentation.rs#unavailable_intervals_reject_changed_endpoint_metadata_in_report_and_artifact]]
rejects timestamp and operation changes in both report and artifact for ambiguous
and negative intervals, preserving the distinction between endpoints and measurements.

## Missing join identity

A missing join key leaves chain counts partial; adjacent events cannot supply the missing relationship.

Covered by [[tests/test_investigation_runtime.rs#connected_chains_missing_join_identity_is_partial_and_never_guessed]].

## Interval boundaries

Chain intervals preserve offsets and fractional milliseconds. Negative time and ambiguous endpoint populations remain unavailable.

Covered by [[tests/test_investigation_runtime.rs#chain_intervals_preserve_offsets_fractional_precision_and_unknown_boundaries]].

## Coverage and zero

A measured zero has distinct boundary occurrences. No filter matches and unparsed input retain their own states rather than becoming measurements.

Covered by [[tests/test_investigation_contract.rs#measured_zero_filter_zero_and_unsupported_input_remain_distinct]].

## Processing cutoffs

Input, record, work, time and memory limits produce partial artifacts and unsupported assessments; cancellation records its reason.

Covered by [[tests/test_investigation_runtime.rs#limits_and_cancellation_return_valid_partial_processed_populations]].

## Redacted verification

Retained arithmetic can remain available while independent source and rule verification is explicitly unavailable after redaction.

Covered by [[tests/test_investigation_contract.rs#redacted_arithmetic_is_distinct_from_independent_source_checking]].

## Independent profile expectations

An inherited candidate is checked against positive, negative and exact pair facts without rewriting it. Contradictions and missing source witnesses fail explicitly.

Covered by [[tests/test_profile_validation.rs#strict_expected_positive_negative_and_pair_facts_validate_inherited_candidate_without_writes]].
