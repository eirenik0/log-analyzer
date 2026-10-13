# Architecture decisions

ADRs record costly architectural choices and their tradeoffs. [[architecture]] describes current structure; [[non-functional-requirements]] defines quality targets; [[checked-analysis]] remains a proposal.

- [[adr-001-shared-event-semantics]] — classify once and share scoped semantics.
- [[adr-002-snapshot-bound-evidence]] — retain evidence independently of presentation.
- [[adr-003-verification-boundaries]] — separate consistency, meaning and prose checks.

## Record lifecycle

Use permanent `adr-NNN-slug.md` names and the metadata and sections shown in these records. Add each record to this index and link the code and requirements it constrains.

New choices start `proposed`; accepted choices retain their rationale. A replacement
gets a new number with reciprocal `supersedes` and `superseded-by` metadata. Use
`obsolete` or `historic` when a decision no longer governs the system. Record
context, decision, at least two alternatives, both benefits and costs, scope and
concrete review triggers. Use `@lat:` for source backlinks and run `lat check`.

## Initial records

ADRs 001–003 retrospectively document existing implemented contracts. Their dates and author identify this documentation work, not the original decisions or a new approval event.

Alternatives are a present-day analysis of tradeoffs, not a claim about historical
meetings. Future changes should follow these contracts or explicitly supersede
them. The structure follows Fluxomni's ADR conventions.
