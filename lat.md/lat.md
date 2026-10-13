# Log Analyzer knowledge

This [Lat](https://www.lat.md/) graph connects agreed principles, the proposed design and existing code. It complements the CLI reference in [[README.md]] and working rules in [[AGENTS.md]].

- [[philosophy]] — agreed design principles and success criteria.
- [[checked-analysis]] — proposed interface, failure behavior and acceptance cases.

## Working with the graph

Use the installed `lat` CLI to retrieve context and validate documentation links.

Install with `npm install -g lat.md` if needed. Run `lat locate "checked analysis"`
and `lat section "philosophy"` to read relevant context; run `lat check` after
updates. Existing agent entry points link here; no generated hooks are required.
Lat validates structure and references, not whether prose or tests prove correctness.
