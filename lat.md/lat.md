# Log Analyzer knowledge

This [Lat](https://www.lat.md/) graph is the canonical project documentation. It connects usage, contracts, design decisions and validation. [[AGENTS.md]] keeps repository working rules; [[CHANGELOG.md]] records releases.

- [[architecture]] — implemented modules and execution paths.
- [[domain]] — records, events, identities, findings and artifacts.
- [[architecture-decisions]] — numbered decisions, tradeoffs and review triggers.
- [[non-functional-requirements]] — measurable quality targets and validation scope.
- [[regression-tests]] — key invariants linked to executable tests.
- [[usage]] — installation, investigations, evidence retrieval and limits.
- [[investigation-workflow]] — worked failure and performance investigations.
- [[cli-reference]] — input, output, filters and command options.
- [[profiles]] — profile selection, adaptation, validation and saved mappings.
- [[agents]] — portable skill installation and supported hosts.
- [[contracts]] — evidence, event, reporting and profile contracts.
- [[contract-fixtures]] — synthetic report and artifact examples.
- [[evaluation]] — deterministic and optional model evaluation protocols.
- [[prose-review]] — evaluation of prose and causal claims.
- [[skill-host-validation]] — dated compatibility observations and limitations.
- [[contributing]] — commit, review and quality requirements.
- [[development]] — build identity, performance checks and release practices.
- [[trusted-publishing]] — crate publication setup and procedure.
- [[philosophy]] — agreed design principles and success criteria.
- [[checked-analysis]] — proposed interface and acceptance cases, not shipped behavior.

## Working with the graph

Use the installed `lat` CLI to retrieve context and validate documentation links.

Install with `npm install -g lat.md` if needed. Run `lat locate "topic"` and
`lat section "usage"` to read relevant context; run `lat check` after updates.
Commands in the guides run from the repository root unless stated otherwise.
Existing agent entry points link here; no generated hooks are required.
Lat validates structure and references, not whether prose or tests prove correctness.
