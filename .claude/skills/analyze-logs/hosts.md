# Host setup and executable compatibility

This skill supplies instructions, references, examples and profile templates. It
requires a separately installed Rust `log-analyzer` executable. It adds no MCP
server or Pi tool extension. Python is only needed for repository checks, not for
using the installed skill.

## Install and invoke

The installer requires Bash and Unix copy/path utilities (macOS, Linux, or a
compatible shell environment). Native Windows installer execution is untested;
Pi Git packages and direct bundle copies do not require the installer.
Run it from the destination project (quote paths with spaces):

```sh
"/path/to/log-analyzer/scripts/install-skill.sh" --host codex --scope project
"/path/to/log-analyzer/scripts/install-skill.sh" --host pi --scope user
"/path/to/log-analyzer/scripts/install-skill.sh" --host claude --scope project
```

`--host` accepts `claude`, `codex`, or `pi`; `--scope` accepts `project` or `user`.
No flags preserves the Claude project default. `--global`/`-g` remains an alias
for user scope. Conflicting scope options are rejected. Project scope means the
current directory, not an inferred repository root. Repeat installs update
bundled files. Other files already in the destination are retained; obsolete
files from older versions may be removed manually. Source/destination overlap
is rejected before copying, except an identical source installation is a no-op.
Existing directory symlinks are resolved; destination child symlinks are rejected
so a reinstall cannot overwrite another location.

Codex and Pi share project `.agents/skills/analyze-logs/` and user
`~/.agents/skills/analyze-logs/`. Install once when both hosts use the same scope.
Claude standalone uses `.claude/skills/analyze-logs/` or
`~/.claude/skills/analyze-logs/`. Do not install duplicate copies in several scopes
unless you intend the host's precedence/collision behavior.

- Codex: use `/skills` or the skill selector to confirm discovery, then
  `$analyze-logs What failed in /absolute/capture.jsonl? --profile /absolute/profile.toml`.
  Codex scans `.agents/skills` from its working directory through the repository
  root. Restart if a change has not appeared. `agents/openai.yaml` supplies optional
  display metadata; it grants no tool permissions.
- Pi: confirm startup discovery, then
  `/skill:analyze-logs What failed in /absolute/capture.jsonl? --profile /absolute/profile.toml`.
  Grant project trust when prompted; use `/reload` after changes. Pi also discovers
  shared `.agents/skills` locations. Skill commands may be hidden by
  `enableSkillCommands`, but manually entered `/skill:analyze-logs` still works.
- Claude standalone: `/analyze-logs What failed in /absolute/capture.jsonl? --profile /absolute/profile.toml`.
  Claude plugin: `/log-analyzer:analyze-logs` with the same arguments. The generated
  Claude wrapper alone retains `context: fork` and Claude Bash permission metadata.

Pi can instead install the Git package (personal by default; `--local` for project):

```sh
pi install git:github.com/eirenik0/log-analyzer
pi install git:github.com/eirenik0/log-analyzer --local
```

For reproducibility append `@<tag-or-commit-containing-the-portable-skill>` to the
Git source. The root `package.json` exposes only `pi.skills`; package installation
still does not install the Rust executable. Claude marketplace installation:

```text
/plugin marketplace add https://github.com/eirenik0/log-analyzer
/plugin install log-analyzer
```

## Locate and verify the executable

Use the executable path supplied by the user, otherwise discover `log-analyzer`
on the host's PATH. A desktop app, terminal host, remote machine and sandbox may
have different PATHs and accessible files. Never assume a binary from this
repository is available in a standalone installation. If missing, stop and say:
“The skill is installed but log-analyzer is unavailable. Install it with
`cargo install log-analyzer --locked`, add its directory to this host's PATH,
or provide an absolute executable path.” Building from a checkout with
`cargo build --release` is another option. Quote the path as one literal argv item.

Run that executable's `--version` and `capabilities`. Retain its build identity;
do not use version ordering alone to infer compatibility. Require capability
`schema_version: 1`, `report_schemas.evidence_contract_version: 1`,
`bounded_reports.version: 1`, `profile_validation.version: 1`, and commands
`info`, `errors`, `validate-profile`, `perf`, `trace`, `search`, `compare`.
If a command or version is missing, stop with the exact missing contract and ask
for a compatible executable from a release or checkout. Do not fall back to
unbounded output or substitute an error span for lifecycle timing. Use actual
binary help for flags; capability checks do not promise every future schema is
compatible.

Use stable absolute input and profile paths accessible to the host and executable.
A permission or missing-file error is an access limitation, not an empty capture.
For an explicit selection, use `--profile NAME_OR_FILE` when capabilities advertise
`profiles.option`; older binaries use `--preset NAME` or `--config FILE`. Copy a suitable template from this
installed skill's `templates/` into the user's project when needed, edit it for
the actual grammar, and validate it against the capture and independent facts.
Bundled examples use repository synthetic fixtures; they are not implicit inputs
for the user's investigation. Resolve `reference.md`, `examples/` and `templates/`
relative to the installed skill folder. The shared workflow governs coverage,
identity, pagination, evidence citations and uncertainty in every host.

## Compatibility evidence

Current documentation was checked on 2026-10-09:
[Codex discovery and metadata](https://learn.chatgpt.com/docs/build-skills),
[Pi skills](https://pi.dev/docs/latest/skills),
[Pi packages](https://pi.dev/docs/latest/packages), and
[Claude skills](https://code.claude.com/docs/en/skills).
See the repository's [host validation record](https://github.com/eirenik0/log-analyzer/blob/main/docs/skill-host-validation.md)
for exact host versions, completed smoke checks and untested behavior. Installer,
metadata and executable fixture checks are deterministic checks, not real-model
quality, injection-resistance or token-savings measurements.
