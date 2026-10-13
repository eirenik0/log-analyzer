# Recover an unrecognized event role

Use this only when original records establish a role missing from the selected
profile. Keep the original profile intact and derive expected facts independently
of candidate output. Event-chain recovery uses `event`/`recognition`; `timing`
validates classified lifecycle pairs, not chain intervals.

```sh
log-analyzer profile prepare SOURCE --template STARTING_PROFILE \
  --candidate-output NEW_CANDIDATE --kind event --purpose recognition
# Edit the candidate's role/step, preserving identity and scope joins.
log-analyzer profile validate SOURCE --profile NEW_CANDIDATE \
  --kind event --purpose recognition --expected SOURCE_EXPECTATIONS
```

Preparation returns validation arguments; append `--expected PATH` to its
`argv_prefix` when expectations were not supplied. Author source-addressed record
facts plus positive and wrong-ID/scope negative links. Minimal illustrative syntax:

```json
{"version":1,"records":[
  {"source":{"input":0,"line":5,"row_path":null},"checks":{
    "/status":"event","/semantics/name":"finished",
    "/semantics/correlation_id":"job-x","/semantics/scope":["east"]}}
],"links":[
  {"chain":"job-chain","step":"finish","root":{"input":0,"line":2,"row_path":null},
   "source":{"input":0,"line":2,"row_path":null},"target":{"input":0,"line":5,"row_path":null},"linked":true},
  {"chain":"job-chain","step":"finish","root":{"input":0,"line":2,"row_path":null},
   "source":{"input":0,"line":2,"row_path":null},"target":{"input":0,"line":8,"row_path":null},"linked":false}
]}
```

Replace addresses and values from the actual source; `input` is zero-based, `line`
one-based, and `row_path` identifies expanded rows. Assert relevant endpoint roles,
boundaries and scope using the `profile-expectations` schema. Keep outcome separate
from completion. A small witness set validates only those witnesses.

Inspect `expected_facts.status: passed` and every record/link result, including
`relationships.status: passed_on_asserted_links`. Correct failures from source
meaning, not by copying candidate output into expectations. Rerun investigation
with the same filters/checklist and a fresh artifact, then draft and verify the
requested interval. Manual subtraction or a verified missing status is not retained
interval verification.
