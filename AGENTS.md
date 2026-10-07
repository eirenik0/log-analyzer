# AGENTS.md

This is the canonical agent guidance for this repository.

If `CLAUDE.md` also exists, it should stay minimal and point here:
- [`CLAUDE.md`](CLAUDE.md)

## Scope

Keep agent docs focused on **how to work** (style/process/policy), not on
project-specific command catalogs, CLI option lists, architecture writeups, or
release manuals.

Project behavior/details should be discovered from:
- the codebase (`src/`, tests)
- CLI help (`cargo run -- --help` and subcommand `--help`)
- [`README.md`](README.md)
- CI/workflow files when relevant

## Source of Truth Rules

- Do not duplicate long user-facing docs in agent files.
- Prefer current code and CLI output over prose that can become stale.
- Update `README.md` and command help for product behavior changes.
- Update this file only when agent workflow/policy guidance changes.

## Working Style

- Inspect first, then edit.
- Make the smallest correct change that solves the task.
- Preserve unrelated user changes; do not revert them.
- Follow existing repo patterns before introducing new abstractions.
- Validate with targeted tests/commands when practical.
- State assumptions and verification gaps explicitly.

## Communication Style

- Be concise, direct, and technical.
- Explain what changed and why with file references.
- Call out risks/regressions and missing validation explicitly.
- Avoid repeating project details that are easy to inspect via CLI/code.

## Code Change Hygiene

- Follow existing formatting and naming conventions in touched files.
- Add tests when changing behavior or fixing bugs (when a test path exists).
- Keep comments minimal and useful (intent over mechanics).
- Update docs where users look first (`README.md`, command help, examples).

## Development Rules

- Use Conventional Commits for every authored commit and PR title:
  `type(scope): description`. Scope is optional; use `!` for breaking changes.
  Allowed types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
  `build`, `ci`, `chore`, `revert`. Example: `fix(parser): preserve timezone offsets`.
- Follow [CONTRIBUTING.md](CONTRIBUTING.md) for local hooks and quality checks.
- Keep formatting, lint, tests, and applicable evaluation checks passing. Do not
  silence failures or weaken assertions to make CI green.
- Add a focused regression test for a bug fix. Use synthetic, generic data;
  never commit private logs, customer details, credentials, or session transcripts.
- Resolve actionable review findings before merging. Explain disagreements with
  evidence; an automated review does not replace tests or maintainer judgment.

## Code Review Rules

- Flag changes that report clean results for nonempty unparsed input, hide parse
  coverage, or treat unavailable analysis as a measured zero. Empty filtered
  results are valid when the report distinguishes them from parsing failure.
- Check that correlation remains scoped correctly and timestamps preserve their
  offsets. Ambiguous or incomplete events must not create invented durations or
  completion claims. Require regression coverage when these paths change.
- Check CLI text/JSON agreement for filters, sorting, limits, and diagnostics;
  ensure arbitrary UTF-8 log content cannot panic during parsing or truncation.

## Repo-Specific Checklist

After feature implementation:
- add details into `.changeset/` and add it to git
- update `README.md`
- update `.claude/skills/` (if user-facing behavior or workflows changed)
