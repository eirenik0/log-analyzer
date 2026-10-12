# Compile a question plan

Read the draft inventory and original evidence before selecting claims. Use every
initial question ID, with exact draft selectors for supported answers and scoped
finding anchors for unknowns. For example:

```json
{"questions":[
  {"id":"capture-duration","claim_id":"EXACT_DRAFT_INTERVAL_ID"},
  {"id":"other-duration","kind":"interval","finding_id":"OTHER_ROOT",
   "end_operation":"capture completed","reason":"No supported ending for this scoped root."},
  {"id":"send-count","claim_id":"EXACT_DRAFT_COUNT_ID",
   "population":{"pointer":"/records","operation":{"kind":"request","name":"send"}}},
  {"id":"cause","kind":"fact","finding_id":"OTHER_ROOT","reason":"No causal mechanism established."}
],"supporting_claim_ids":["EXACT_CACHED_LINK_ID"],
"reviewed_checks":["event_subjects","duration_endpoints_and_units",
"operation_types_and_count_semantics","terminal_later_cached_contrary",
"summary_consistency","causal_support"]}
```

Replace all illustrative IDs/names and include every actual question. Supply the
review checks only after inspection. Supporting cached/contrary links keep their
own IDs; they cannot substitute for an unknown measurement. Count questions must
independently declare the expected `population` pointer and operation from the inventory (`operation: null` means unfiltered); a mismatched
selector is rejected. Scoped counts are not automatically global attempts.
An unknown interval's expected ending must come from source meaning. Complete
source-supported recognition recovery before declaring it unknown.

Compile and verify using the command in the skill entrypoint. The helper creates
`requirements`, `unresolved`, and exact final-review mappings; it does not infer
question meaning. Regenerate the draft after changing the artifact. Existing
output paths are rejected; failed checks preserve their report and exit nonzero.

Inspect each `requirement_results` status: `verified`, `declared_unknown`, or
`needs_revision`. An available requested interval cannot be declared unknown.
`verified_with_unknowns` means the asserted subset and unknown bookkeeping passed,
not that unknown conclusions or absence were proved. All-unknown plans are valid
when every required ID has a typed, anchored unresolved entry. Preserve those
unknowns in the answer, using the user's requested format.
