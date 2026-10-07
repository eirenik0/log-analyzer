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

Enable the commit-message hook in each checkout (Python 3 required):

```bash
git config --local core.hooksPath .githooks
```

If you already use a hooks manager, call `python3 scripts/check_commits.py
--message-file "$1"` from its commit-msg hook instead of replacing your setup.
CI validates both the PR title and all non-merge commits introduced by the PR.
Fix an invalid commit with amend/rebase before requesting review.

## Quality checks

Run before opening or updating a PR:

```bash
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked -- -D warnings
cargo test --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
```

When the evaluation corpus is available, also run its documented checks after
parser, reporting, or CLI changes. Add small synthetic regression cases for bug
fixes. Do not remove assertions or broaden expected failures to hide regressions.
Keep customer logs, identifying session details, and secrets out of public files.

Update README/help for user-facing changes and add a `.changeset/` entry for
releasable changes. Update the analysis skill when its user workflow changes.

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
