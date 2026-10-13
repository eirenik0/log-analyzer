# Log Analyzer philosophy

These are the project's agreed design principles, guiding the [[checked-analysis|proposed checked-analysis design]]. They do not claim every current workflow already satisfies them.

Build a log analyzer that gives useful, trustworthy answers through one command.

- **Keep deterministic work inside the tool.** Parsing, correlation, timing,
  counting, evidence binding and verification belong in the analyzer.
- **Return checked findings.** Each fact carries its subject, source citations,
  endpoints or count population, and limitations. Text, JSON and follow-up queries
  use the same results. Checks establish consistency with captured evidence and
  declared semantics, not unlogged facts.
- **Let agents investigate.** Agents choose questions and intended subjects,
  interpret evidence and explain findings. They should not assemble proof files
  or manage hashes.
- **Make uncertainty explicit.** Ambiguous identity, missing events, unsupported
  semantics and partial coverage must never become invented facts or measured
  zeros. Temporal order does not prove causality.
- **Keep the interface simple.** Default output shows what happened, timings,
  coverage and unknowns. Technical detail belongs behind `--verbose`; helpers
  stay optional and the skill stays short.
- **Validate adaptations independently.** A generated profile cannot provide
  its own ground truth. Validate against independent expectations and negative
  cases; stop recovery when evidence or budgets are exhausted.
- **Enforce mandatory checks in the runner.** Skill instructions alone are
  insufficient. Check requested-question coverage and tool-produced results;
  verified facts do not automatically validate arbitrary prose.
- **Measure useful completion.** Evaluate supported answers, false certainty,
  unnecessary unknowns, latency and cost on held-out logs and multi-turn
  investigations. Tool-call and rejection counts are not success metrics.

Preserve proven code and focused regression tests. Prefer removing caller
obligations to adding workflow machinery. Refactor incrementally, testing through
the interface users call and proving each improvement under matched evaluation
budgets without weakening correctness checks.
