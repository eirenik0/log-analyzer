# Contributing

Use a focused branch and pull request. Keep unrelated changes separate and
describe the behavior change, its reason, and the checks you ran.

## Commit messages

Every authored commit and PR title must use Conventional Commits:

```text
fix(parser): preserve timezone offsets
feat(cli)!: change the JSON report schema
docs: explain custom profiles
```

Allowed types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`,
`ci`, `chore`, `revert`. Scope is optional. A breaking change needs `!` or a
`BREAKING CHANGE:` footer explaining migration. Generated merge commits are
excluded from the commit check. Squash commits must retain the valid PR title.

Install [pre-commit](https://pre-commit.com/#install), then enable the configured
commit-message hook in your clone:

```bash
pre-commit install --install-hooks
```

The configuration uses the standard
[conventional-pre-commit](https://github.com/compilerla/conventional-pre-commit)
hook at the `commit-msg` stage. It checks local commit messages; keep PR titles
conventional when editing them on GitHub too.

## Pull request format

Use [[.github/PULL_REQUEST_TEMPLATE.md]] for every PR, including descriptions created with the CLI or an agent. Keep the same section order and scale detail to the change.

1. **Summary:** the outcome and why it matters, in one or two sentences.
2. **Problem:** the concrete limitation or failure being addressed.
3. **Solution:** the final approach, with relevant ADR, NFR or contract links.
4. **Scope:** affected behavior and material limits or migration risks.
5. **Validation:** actual checks, outcomes and remaining gaps; distinguish local
   results from CI and avoid claims beyond the tested scope.

Use a concise Conventional Commit title. Under Solution, include a small Mermaid
infographic when it clarifies flow, ownership or relationships. Use short labels,
a clear direction and restrained colors with readable contrast. Replace template
examples; omit decorative diagrams for trivial changes. Keep prose concise and
rewrite the description if the implementation's scope changes.

## Quality checks

The maintained example integration test requires Python 3.10+ on PATH (standard
library only). Run before opening or updating a PR:

```bash
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked -- -D warnings
cargo test --locked
python3 -X warn_default_encoding -W error::EncodingWarning -m unittest discover -s scripts -p 'test_*.py'
python3 -X warn_default_encoding -W error::EncodingWarning -m unittest discover -s evals -p 'test_*.py'
```

When the evaluation corpus is available, also run its documented checks after
parser, reporting, or CLI changes. Add small synthetic regression cases for bug
fixes. Do not remove assertions or broaden expected failures to hide regressions.
Keep customer logs, identifying session details, and secrets out of public files.

Update the canonical Lat guides and command help for user-facing changes and add a `.changeset/` entry for
releasable changes. Update the analysis skill when its user workflow changes.

## Architecture and quality records

Use [[architecture-decisions]] for costly architectural choices and [[non-functional-requirements]] for measurable quality targets. Follow their lifecycle rules and keep implementation and regression links current.

Update the corresponding indexes, use `@lat:` source backlinks, and run `lat check`.
A documentation check validates structure and links; execute the referenced tests
or measurements before claiming a requirement is met.

## Review

The hosted Codex reviewer follows the `Code Review Rules` in `AGENTS.md`.

Enable automatic reviews for this repository in ChatGPT's code review settings;
after connection, `@codex review` in a PR comment requests a review manually.
See the [official setup guide](https://learn.chatgpt.com/docs/third-party/github).
Address actionable findings and keep CI green before merge. A bot response is
not a substitute for maintainer judgment.

Repository administrators manage required checks in GitHub settings. The release
workflow currently pushes its version commit directly to `main`; preserve an
explicit administrator release exception until releases use pull requests.

## Prepared releases

The single-package Knope configuration uses `default:` as its changeset key;
prefer `knope document-change` to generate the file.

`knope prepare-release`
updates Cargo.toml/Cargo.lock, plugin versions and CHANGELOG.md, and consumes the
pending changesets. Review and merge this prepared version before publication.

To publish it later, dispatch the Release workflow on main with `prepared-version`
set to the exact stable version (for example `0.3.0`). Use `dry-run` to validate
without publishing. This path checks synchronized metadata and release notes,
skips another version bump, and pins tests, builds and publication to one captured
commit. Leave `prepared-version` empty for the ordinary prepare-and-release path.
