# Log Analyzer

[![CI](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/ci.yml)
[![Release](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml/badge.svg)](https://github.com/eirenik0/log-analyzer/actions/workflows/release.yml)

Log Analyzer is a local Rust CLI for investigating log failures and performance.
It parses, correlates and calculates; people and agents choose questions and
explain findings. The binary does not call a model or prove a root cause.

## Installation

Build from source with Rust 1.89 or newer:

```sh
cargo install --path .
log-analyzer investigate application.log
```

See the [installation guide](lat.md/usage.md#installation) for the release installer.
Use `log-analyzer --help` and subcommand help for supported syntax.

Investigation findings show their scope, source assurance, citations and limits.
Use `--verbose` for recognition and join diagnostics, or `--summary` for bounded
JSON with coverage and explicit omissions. An existing link does not verify the
intended subject. See the [synthetic checked findings example](examples/investigations/checked/README.md).
Configured text groups count assurance across all members and label representative
subjects and limits separately; independent input scopes remain separate groups.
Checked JSON reports identify their presentation version and require uncertainty
on every page; decision briefs also require the omitted-finding count.
Retained numeric payloads round-trip exactly through JSON; source replay rejects altered values.

## Documentation

Project documentation lives in the [Lat knowledge graph](lat.md/lat.md).

- [Architecture](lat.md/architecture.md), [domain concepts](lat.md/domain.md) and [ADRs](lat.md/architecture-decisions/architecture-decisions.md) and [quality requirements](lat.md/non-functional-requirements/non-functional-requirements.md)
- [Investigation guide](lat.md/usage.md) and [worked workflows](lat.md/investigation-workflow.md)
- [CLI reference](lat.md/cli-reference.md) and [profiles](lat.md/profiles.md)
- [Agent integration](lat.md/agents.md)
- [Evidence and analysis contracts](lat.md/contracts/contracts.md)
- [Evaluation](lat.md/evaluation.md) and [contributing](lat.md/contributing.md)
- [Design philosophy](lat.md/philosophy.md) and [proposed checked analysis](lat.md/checked-analysis.md)

The [changelog](CHANGELOG.md) records releases; [AGENTS.md](AGENTS.md) defines
repository working rules. Use [Lat](https://www.lat.md/) to navigate and validate
the graph: `lat locate "topic"`, `lat section "usage"`, and `lat check`.
