# Map every question, including unknowns

Keep stable IDs for all requested durations, completion questions, counts and cause
questions, including those which remain unknown. Pass these IDs as repeated
`--required-relationship` arguments before drafting. Do not omit an unknown to get
a clean review, and do not rename a cached link or status fact to its question ID.

## Compile a reviewed plan

Prefer the supplied compiler to reconstructing assertion syntax. List candidates (add `--analyzer EXECUTABLE` to create a missing draft):

```sh
python3 -I SKILL_DIR/scripts/map_questions.py --draft DRAFT --artifact ARTIFACT
```

Select the exact draft claim IDs and source-checked finding anchors. Save this
plan to a new file in your authorized working folder, replacing these illustrative
values; include every actual question, not just the example's four:

```json
{"questions":[
  {"id":"first-capture-duration","claim_id":"EXACT_DRAFT_INTERVAL_ID"},
  {"id":"second-capture-duration","kind":"interval","finding_id":"SECOND_ROOT_FINDING",
   "end_operation":"capture completed","reason":"No supported ending for this scoped root after recognition recovery."},
  {"id":"send-count","claim_id":"EXACT_DRAFT_COUNT_ID"},
  {"id":"cause-question","kind":"fact","finding_id":"SECOND_ROOT_FINDING",
   "reason":"No causal mechanism established by these records."}],
 "supporting_claim_ids":["EXACT_DRAFT_CACHED_LINK_ID"],
 "reviewed_checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics",
                    "terminal_later_cached_contrary","summary_consistency","causal_support"]}
```

`claim_id` appears **only in this plan's supported selection**. The compiler writes
schema-correct `id` fields, typed requirements and anchored unknowns; it keeps
supporting links under their own IDs. It derives interval root and operation names
from the selected retained proposal, so independently confirm their meaning first.
For unknown intervals you supply the expected end name from source meaning. It
does not infer absence or choose a question's kind. Only supply `reviewed_checks`
after actually inspecting the evidence and summary. Compile and verify in one command:

```sh
python3 -I SKILL_DIR/scripts/map_questions.py --draft DRAFT --artifact ARTIFACT --plan PLAN --output NEW_CLAIMS --analyzer EXECUTABLE --verification-output NEW_REPORT
```

Inspect `requirement_results` and final review. Fix errors and verify again while
budget remains; the helper runs retained verification and exits nonzero if it fails. When recovery creates a
fresh artifact, regenerate its draft and plan against that artifact. Initial
checklist IDs cannot be omitted. Additional question IDs may be added explicitly.

## Manual assertions when needed

After reading the draft and source evidence, populate `requirements` with one entry
for **every** `required_claim_ids` ID. Each entry declares the required claim `kind`.
For an interval also supply its retained root `finding_id` and the exact expected
`start_operation` and `end_operation` names from independent source meaning. The
checker requires that interval to start at this scoped root and end at the named
operation. A link, literal status or cache/poll interval cannot satisfy it.

Keep supported claims in `claims`. For an unresolved question, add its ID and a
specific reason to the separate `unresolved` array; do not add a same-ID claim.
Anchor its requirement to the relevant retained chain `finding_id`. Unknowns are
**declared unknowns**, not verified facts or proof of absence. A retained supported
interval with the requested endpoints cannot be declared unknown. Before using
this path, complete source-supported recognition recovery within the real budget.
If the source does not establish an ending, completion and elapsed time stay unknown.

For example, after independently checking the sources for two distinct scoped roots:

```json
{
  "requirements": [
    {"id":"first-capture-duration","kind":"interval","finding_id":"FIRST_ROOT_FINDING",
     "start_operation":"First item","end_operation":"capture completed"},
    {"id":"second-capture-duration","kind":"interval","finding_id":"SECOND_ROOT_FINDING",
     "start_operation":"Second item","end_operation":"capture completed"},
    {"id":"send-count","kind":"distinct_records"},
    {"id":"cause-question","kind":"fact","finding_id":"SECOND_ROOT_FINDING"}
  ],
  "unresolved": [
    {"id":"second-capture-duration","reason":"No supported capture ending for this scoped root; capture completeness is unknown."},
    {"id":"cause-question","reason":"Only ordering and cached outcome are available; no causal mechanism is established."}
  ],
  "final_review": {
    "finding_claim_ids":["first-capture-duration","send-count"],
    "summary_claim_ids":["first-capture-duration","send-count"],
    "unresolved_question_ids":["second-capture-duration","cause-question"],
    "checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics",
              "terminal_later_cached_contrary","summary_consistency","causal_support"]
  }
}
```

These are fields to merge into your version-1 assertions file, not a complete file.
Replace IDs, finding anchors and operation names with actual source-checked values.
Include **all** requested questions and supported selected assertions, not just this
example's four entries. Supporting cached/contrary links keep their own descriptive
IDs in `claims` and the verified review lists. They explain an unknown but do not
answer its measurement question. Repeat the same requirements/unknowns when rerunning
with a recovered artifact, updating finding IDs and claim bindings to that artifact.

Inspect `requirement_results` individually. Their statuses are `verified`,
`declared_unknown` or `needs_revision`. `unresolved_required_claim_ids` remains
visible even when `missing_required_claim_ids` is empty. The final review's
`unresolved_question_ids` must exactly match the unresolved list. With valid mappings
and acknowledgments, the verifier returns `verified_with_unknowns` and the review
returns `declared_checks_passed_with_unknowns`. Exit success means the asserted subset
and explicit unknown bookkeeping passed; it does not verify unknown conclusions.
Report those unknowns in detailed findings and summary. A `needs_revision` mapping
requires correction, not a clean-result claim. Legacy files without `requirements`
remain accepted, but cannot guard against using the wrong claim kind for a question.

## Preserve the response format

A verification limitation belongs inside the user's requested response fields.
If the user requests a single JSON object, do not prepend a warning about an unknown
or a verifier status. Include it in `explanation` (or the specified equivalent).
Do not add Markdown fences, headings, preambles, a second object or trailing prose.

Save the proposed final object to a new file **inside your authorized working
folder**, validate it locally and emit the exact validated object. For example:

```sh
python3 -I -c 'import json,pathlib; p=pathlib.Path("final-answer.json"); d=json.loads(p.read_text(encoding="utf-8")); assert isinstance(d,dict); print(json.dumps(d,ensure_ascii=False,separators=(",",":")))'
```

Check required keys, exact identity strings, types and summary agreement against the
user's supplied contract before this final call. Put limitations inside the object.
Combine file creation, final verification inspection and format validation when
practical to conserve calls. Copy only the validated JSON into the final response;
any conversational progress stays outside that final response. No post-hoc scorer
extraction or silent correction of a failed answer counts as a successful trial.
