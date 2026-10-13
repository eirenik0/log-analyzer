# Compile a question plan

Complete helper flow (replace paths and selectors after independent inspection):

```sh
log-analyzer investigate /absolute/capture.jsonl --artifact /absolute/evidence.json \
  --question 'How long did this capture take?' --required-relationship capture-duration --summary
python3 -I SKILL_DIR/scripts/map_questions.py --artifact /absolute/evidence.json \
  --draft /absolute/draft.json --analyzer EXECUTABLE --prepare-plan /absolute/plan.json
# Inspect delivered sources and candidate identities; edit plan.json as below.
python3 -I SKILL_DIR/scripts/map_questions.py --artifact /absolute/evidence.json \
  --draft /absolute/draft.json --analyzer EXECUTABLE --plan /absolute/plan.json \
  --output /absolute/claims.json --verification-output /absolute/verification.json
```

Read the draft inventory and original evidence before selecting claims. Use every
initial question ID and select at most 100 claims (the candidate inventory may be larger),
with exact draft selectors for supported answers and scoped
finding anchors for unknowns. For example:

```json
{"questions":[
  {"id":"capture-duration","claim_id":"EXACT_DRAFT_INTERVAL_ID",
   "binding":{"finding_id":"ROOT","pointer":"EXACT_RETAINED_INTERVAL_POINTER","start_operation":"capture","end_operation":"capture completed"}}
],"supporting_claim_ids":[],
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
use `kind: "link"` and `step_id`, with full retained `source`/expected `target`
addresses for endpoint-specific questions. Use an unknown interval's exact retained
`pointer` when same-named joins differ in availability. Unknown counts use `kind: "distinct_records"`
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
