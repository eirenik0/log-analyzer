# Non-functional requirements

NFRs define measurable quality targets and how to check them. They complement [[architecture-decisions]]: ADRs record the chosen approach; requirements state the quality it must preserve.

- [[reliability]] — evidence correctness and historical retrieval.
- [[performance-efficiency]] — observable processing and report bounds.
- [[security]] — redacted evidence retention.
- [[usability]] — one-command initial analysis.

## Record lifecycle

Each requirement is a leaf section with a permanent NFR ID, target, measurement, rationale, scope, owner, status, review cadence and links to implementing code or ADRs.

Follow Fluxomni's field structure using `**Target** — value` labels. New targets
start `draft`; `agreed` records an established requirement, `at-risk` records a
known breach, `met` requires dated measurements for the declared scope, and
`retired` preserves a withdrawn target. Never infer a passing measurement from
a structural documentation check. Link NFR implementations with `@lat:` so Lat
can discover them. Run `lat check` after edits and execute the affected checks.

The initial targets capture existing regression contracts. They do not establish
production performance or reliability on arbitrary logs. Numerical latency,
throughput and model-quality commitments require a specified workload, environment
and independent measurements before acceptance; see [[evaluation]] and [[development]].
