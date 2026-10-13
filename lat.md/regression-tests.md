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
[[tests/test_checked_presentation.rs#checked_subjects_require_exact_root_outcome_and_evidence_sets]]
rejects missing, duplicated or substituted subjects in original and redacted
reports, while accepting reordered subjects and older reports without the projection.
[[tests/test_checked_presentation.rs#measured_intervals_cannot_be_hidden_by_unavailable_status_or_missing_timestamps]]
rejects unavailable-status substitutions and hidden known timestamps. Unavailable
interval controls also check exact reasons for ambiguity, negative time and missing timestamps.
[[tests/test_checked_presentation.rs#retained_endpoint_names_are_checked_when_source_verification_is_unavailable]]
rejects changed or removed names for measured and unavailable intervals when stored
source verification is unavailable, while accepting redacted reports with omitted semantics.
[[tests/test_checked_presentation.rs#interval_cardinalities_are_checked_against_retained_chain_populations]]
rejects forged singleton root/source/target counts that would turn ambiguous joins
into measured intervals, and checks explicit deferral when redaction prevents replay.
[[tests/test_checked_presentation.rs#chain_validation_honors_raised_processing_limits_before_selection]]
checks publication and artifact validation after more than 100,000 expanded rows
are parsed under raised limits, with only ten selected chain records retained.
[[tests/test_checked_presentation.rs#replay_rejects_invented_completion_and_incomplete_step_results]]
rejects fabricated join identities and mutually consistent report/artifact edits
that invent a completion, remove steps or joins, or change labels, counts, ambiguity
and completeness. Processing cutoffs
explicitly defer completeness when calculation-stage history is unavailable.
[[tests/test_checked_presentation.rs#replay_binds_timestamps_and_provenance_before_interval_validation]]
rejects changed or removed retained timestamps and altered year/offset provenance,
even when event sequences and interval measurements are updated consistently.
[[tests/test_checked_presentation.rs#checked_presentation_requires_canonical_qualifications]]
rejects empty or replaced aggregate limits, finding assurance limits and population
rules in full and empty-page reports, with and without retained artifact bytes.
[[tests/test_checked_presentation.rs#replay_binds_nested_excerpt_payloads_to_source]]
rejects altered text, omitted-character counts, displayed fields and verification
metadata in linked, ambiguous and later-activity excerpts with configured source fields.
[[tests/test_checked_presentation.rs#replay_binds_chain_identity_and_context_to_retained_subjects]]
rejects coordinated report/artifact changes to chain identities, context identities,
scope fields, root event names and root ambiguity, even when checked projections agree.
It also rejects hidden later activity and earlier occurrences mislabeled as later samples.
[[tests/test_checked_presentation.rs#replay_requires_all_chain_findings_for_completed_inputs]]
rejects deletion of one or all chains despite consistent report totals and references.
It also rejects deleting the root record together with its chain finding.
[[tests/test_checked_presentation.rs#independent_cutoffs_do_not_defer_unaffected_chain_completeness]]
keeps earlier chain and step completeness checked when a later independent input
fails parsing or exhausts its record budget.
[[tests/test_checked_presentation.rs#replay_checks_classifications_against_captured_source_and_rules]]
rejects edited rule IDs and event semantics even when checked subject projections
agree with the altered retained classification.
[[tests/unit/grouped_assurance.rs#grouped_assurance_covers_every_member_and_preserves_scope_boundaries]]
checks mixed assurance in both representative orders, explicit representative-only
details and separation of matching dimensions across independent scopes. Configured
view CLI tests check agreement with per-finding JSON assurance, including redaction.

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
