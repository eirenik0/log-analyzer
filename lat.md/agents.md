# Agent integration

Install and use the portable analysis skill with supported agent hosts. See [[skill-host-validation]] for historical compatibility checks and their limits.

## Agent Skills: Codex, Pi and Claude Code

The canonical [analyze-logs skill](../.agents/skills/analyze-logs/SKILL.md) supplies
one investigation workflow and shared references, examples and profile templates.

Claude's wrapper and bundle are generated from it; Codex and Pi use portable
frontmatter without Claude-specific permissions or fork settings. Install the
[Rust executable](usage.md#installation) separately; a skill or Pi package does not include it.

Run the installer from the destination project:

```bash
/path/to/log-analyzer/scripts/install-skill.sh --host codex --scope project
/path/to/log-analyzer/scripts/install-skill.sh --host pi --scope user
/path/to/log-analyzer/scripts/install-skill.sh --host claude --scope project
```

Defaults remain `--host claude --scope project`; `--global`/`-g` means user scope.
Codex/Pi share `.agents/skills/` (project) and `~/.agents/skills/` (user); Claude
uses `.claude/skills/` and `~/.claude/skills/`. Project means the current directory.
An identical source installation is a safe no-op; overlapping destinations and
destination child symlinks are rejected before copying. Repeat installs update
bundle files and retain unrelated destination files.

Explicit invocation in each host:

```text
$analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
/skill:analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
/analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
```

The lines above are for Codex, Pi and Claude standalone respectively. Confirm
Codex discovery with `/skills`; confirm Pi startup discovery and use `/reload`
after edits. Pi project resources may require project trust. Install the Pi Git
package with `pi install git:github.com/eirenik0/log-analyzer` (add `--local` for
project scope; pin a tag/commit containing the skill for reproducibility).

Claude plugin paths and invocation remain:

```text
/plugin marketplace add https://github.com/eirenik0/log-analyzer
/plugin install log-analyzer
/log-analyzer:analyze-logs What failed in /logs/capture.jsonl? --profile /profiles/my-team.toml
```

Supply an absolute executable path if `log-analyzer` is unavailable on the host's
PATH. The skill checks `capabilities` for schema/evidence contract 1 and version-1
bounded retrieval/profile validation, then checks coverage and profile suitability.
Missing binaries or unsupported contracts stop with installation/upgrade guidance.
Input and profile paths must be accessible to the agent and executable.
See [host setup](../.agents/skills/analyze-logs/hosts.md) for discovery and compatibility,
the checked [failure](../.agents/skills/analyze-logs/examples/debug-failure.md) and
[performance](../.agents/skills/analyze-logs/examples/performance.md) examples,
and the [host validation record](skill-host-validation.md) for tested versions
and explicit gaps. External data handling follows the agent's configuration; see
[local data flow](usage.md#local-data-flow-and-external-agents).

To change the skill, edit `.agents/skills/analyze-logs/`, run
`python3 scripts/sync-skills.py`, and commit the generated Claude bundle too.
Tests reject drift. Scripted workflow checks do not measure model accuracy or
claim token savings.
