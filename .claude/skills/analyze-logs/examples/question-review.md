# Compile a question plan

Read the draft inventory and original evidence before selecting claims. Use every
initial question ID, with exact draft selectors for supported answers and scoped
finding anchors for unknowns. For example:

```json
{"questions":[
  {"id":"capture-duration","claim_id":"EXACT_DRAFT_INTERVAL_ID",
   "binding":{"finding_id":"ROOT","pointer":"EXACT_RETAINED_INTERVAL_POINTER","start_operation":"capture","end_operation":"capture completed"}},
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

Choose each expected `binding` independently of its claim selector: interval finding
and exact interval `pointer`/endpoints (the source may be an intermediate step); link `finding_id`/`step_id` and exact join `pointer`; fact `field: {pointer, equals}`.
Replace illustrative values and include every actual question. Supply the
review checks only after inspection. Supporting cached/contrary links keep their
own IDs; they cannot substitute for an unknown measurement. Count questions must
independently declare the expected `population` pointer and operation from the inventory (`operation: null` means unfiltered); a mismatched
selector is rejected. Scoped counts are not automatically global attempts.
Unknown interval endpoints must come from source meaning; set `start_operation`
for an intermediate start (otherwise it defaults to the root name). Complete
source-supported recognition recovery before declaring it unknown. Unknown links
use `kind: "link"` and `step_id`; unknown counts use `kind: "distinct_records"`
and `population`. The verifier rejects invalid selectors and available requested facts.

Compile and verify using the command in the skill entrypoint. For artifacts above
64 MiB, pass the agreed `--artifact-max-bytes` allowance to the helper. The helper creates
`requirements`, `unresolved`, and exact final-review mappings; it does not infer
question meaning. Regenerate the draft after changing the artifact. Existing
output paths are rejected; failed checks preserve their report and exit nonzero.

Inspect each `requirement_results` status: `verified`, `declared_unknown`, or
`needs_revision`. An available requested interval cannot be declared unknown.
`verified_with_unknowns` means the asserted subset and unknown bookkeeping passed,
not that unknown conclusions or absence were proved. All-unknown plans need only
typed, anchored unresolved entries; no draft file is needed. Preserve those
unknowns in the answer, using the user's requested format.
