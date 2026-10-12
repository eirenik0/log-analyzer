# Recover a source-supported event role

Use this when original source meaning establishes a terminal role which the selected
profile does not recognize. Read the original records and starting profile first.
Keep the recovery in the authorized working directory. Do not edit the original
profile or derive expected truth from draft claims or candidate validation output.

For event-chain relationships, the kind is `event` and the purpose is
`recognition`, even when the final question asks for a duration. `timing` validates
classified start/end lifecycle pairs; a chain interval is checked later by evidence
claims. `--kind` is required on both commands. Use the same source files and filters.

```sh
log-analyzer profile prepare SOURCE --template STARTING_PROFILE \
  --candidate-output NEW_CANDIDATE --kind event --purpose recognition
# Edit NEW_CANDIDATE to recognize only the independently established role.
log-analyzer profile validate SOURCE --profile NEW_CANDIDATE \
  --kind event --purpose recognition --expected SOURCE_EXPECTATIONS
```

Use only the supported flags shown above. `--project-root` and `--profiles-dir`
belong to `investigate`/profile resolution, not `profile prepare` or `profile validate`.
An option error is a recoverable syntax error, not evidence that the time budget
expired. After preparation, edit the role/step using the selected profile's existing
selector and join syntax, then validate; do not end the workflow after creation.
Use the clock-based budget helper before declaring a budget stop. Even an actual
stop must preserve the user's JSON-only response format, with limits inside its fields.

Preparation preserves inherited investigation roles/chains. Inspect the generated
file before editing. Add or extend a role selector for the observed terminal field
value and connect it to the intended root with the existing scoped join keys. Keep
outcome separate from completion. Preserve unrelated roles and joins. Prefer
extending a known terminal role to adding a redundant step. Never remove scope
keys to obtain a match. Preparation's `next_action` retains the validation parameters;
if no expectations were supplied, append `--expected` and the independent file
path to `argv_prefix`. Its null `argv` is deliberately not executable yet.

Write a version-1 expectations file from the original source, not the edited
candidate. For example, suppose source lines 2 and 5 mean a `begin` and `finished`
for job `job-x` in scope `east`; line 8 is a finish of the reused ID in `west`.
The independent relationship expectation is:

```json
{"version":1,"records":[
  {"source":{"input":0,"line":2,"row_path":null},"checks":{
    "/status":"event","/semantics/kind":"event","/semantics/name":"begin",
    "/semantics/phase":null,"/semantics/correlation_id":"job-x",
    "/semantics/scope":["east"],"/semantics/end_expected":true}},
  {"source":{"input":0,"line":5,"row_path":null},"checks":{
    "/status":"event","/semantics/kind":"event","/semantics/name":"finished",
    "/semantics/phase":null,"/semantics/correlation_id":"job-x",
    "/semantics/scope":["east"],"/semantics/end_expected":true}}
],"links":[
  {"chain":"job-chain","step":"finish","root":{"input":0,"line":2,"row_path":null},
   "source":{"input":0,"line":2,"row_path":null},"target":{"input":0,"line":5,"row_path":null},"linked":true},
  {"chain":"job-chain","step":"finish","root":{"input":0,"line":2,"row_path":null},
   "source":{"input":0,"line":2,"row_path":null},"target":{"input":0,"line":8,"row_path":null},"linked":false}
]}
```

Replace all addresses, values, chain and step IDs using the actual source and
intended relationship. This example uses the classifier default `end_expected: true`;
use an explicit source-supported mapping value when different. This flag alone
does not establish an observed ending. `input` is a zero-based file ordinal, `line` a one-based
physical source line, `row_path` null for ordinary JSONL. Check the original source
for both endpoints, exact name, ID, scope and boundary meaning. Include supported
positive links and wrong-scope/ID negative witnesses available in that source.
Cover other links and relevant records when claiming entire-profile semantic
support; a small witness set validates only those witnesses. The schema is
`profile-expectations` (`schema --help` shows how to retrieve it). Do not insert
chain durations into `pairs`: those are classified lifecycle pair expectations.

Inspect `profile_validation.expected_facts.status` (`passed`) and each expected
record and relationship result, including the negative witness. Require
`relationships.status: passed_on_asserted_links` for your asserted links. Exit zero
or high match counts alone do not establish this. Failed assertions require a
source-supported correction, not changing expected truth to agree with output.
Record the expected-file identity and the dimensions checked. Disclose unasserted
coverage; don't describe partial validation as full profile validation.

Then explicitly rerun `investigate` with the candidate and a **fresh artifact**, the
same question/required IDs, filters and limits. Read relevant root bundles,
terminal/contrary evidence and omissions. Draft claims from the new checksum-bound
artifact. Select the actual root-to-terminal interval, remove cache/poll intervals
from capture-duration claims, and map the requested interval ID to the interval.
Verify with `--claims`, inspect `subject_verified` and the exact source/target
operations, then complete final review. A standalone manual timestamp difference
or a verified `not_observed` status does not verify the requested interval.
