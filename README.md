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

## Documentation

Project documentation lives in the [Lat knowledge graph](lat.md/lat.md).

- [Architecture](lat.md/architecture.md), [domain concepts](lat.md/domain.md) and [design decisions](lat.md/design-decisions.md)
- [Investigation guide](lat.md/usage.md) and [worked workflows](lat.md/investigation-workflow.md)
- [CLI reference](lat.md/cli-reference.md) and [profiles](lat.md/profiles.md)
- [Agent integration](lat.md/agents.md)
- [Evidence and analysis contracts](lat.md/contracts/contracts.md)
- [Evaluation](lat.md/evaluation.md) and [contributing](lat.md/contributing.md)
- [Design philosophy](lat.md/philosophy.md) and [proposed checked analysis](lat.md/checked-analysis.md)

The [changelog](CHANGELOG.md) records releases; [AGENTS.md](AGENTS.md) defines
repository working rules. Use [Lat](https://www.lat.md/) to navigate and validate
the graph: `lat locate "topic"`, `lat section "usage"`, and `lat check`.
