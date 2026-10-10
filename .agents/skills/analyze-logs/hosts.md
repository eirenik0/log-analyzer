# Locate and install the current skill

The skill supplies instructions and templates; it does not install the Rust
`log-analyzer` executable or add an MCP server. Python is needed only for repository
checks. Resolve bundled resources relative to this skill directory.

## Executable

Use the user-supplied executable, otherwise locate `log-analyzer` on the host's
PATH. Remote machines, desktop apps and terminals may have different paths.
If missing, explain how to install it with `cargo install log-analyzer --locked`,
build this checkout with `cargo build --release`, or supply an absolute path.

Check `--version` and `capabilities`; retain build identity. This skill requires
`investigate`, `investigation-evidence`, automatic profile detection and
`profiles.option: "profile"`. If missing, obtain a current executable; do not
substitute another investigation workflow. Use its own help for supported flags.
Input and profile paths must be accessible to that executable. Access failures
are not empty captures.

## Install and invoke

From the destination project, use the repository installer:

```sh
"/path/to/log-analyzer/scripts/install-skill.sh" --host codex --scope project
"/path/to/log-analyzer/scripts/install-skill.sh" --host pi --scope user
"/path/to/log-analyzer/scripts/install-skill.sh" --host claude --scope project
```

The installer requires Bash and Unix utilities; native Windows execution remains
untested. Direct bundle copies and Pi Git packages do not require the installer.
Project scope means the current directory. Repeat installs update bundled files
and preserve unrelated destination files; overlapping destinations are rejected.

Codex and Pi share `.agents/skills/analyze-logs/` in the project or home directory.
Claude uses `.claude/skills/analyze-logs/`. Avoid duplicate installs across scopes.

- Codex: confirm discovery through `/skills`, then use
  `$analyze-logs Investigate /absolute/capture.jsonl`.
- Pi: confirm discovery, reload after changes with `/reload`, then use
  `/skill:analyze-logs Investigate /absolute/capture.jsonl`.
- Claude standalone: `/analyze-logs Investigate /absolute/capture.jsonl`.
  The plugin invocation is `/log-analyzer:analyze-logs` with the same request.

Let the analyzer detect a profile; supply `--profile NAME_OR_FILE` when an explicit
choice is needed. Templates are in `templates/`; copy and validate an edited
candidate against actual capture semantics. Bundled examples are synthetic
repository fixtures, never implicit inputs for the user's investigation.

Pi Git-package installation:

```sh
pi install git:github.com/eirenik0/log-analyzer
pi install git:github.com/eirenik0/log-analyzer --local
```

Append `@<tag-or-commit>` for a reproducible version. Claude marketplace installation:

```text
/plugin marketplace add https://github.com/eirenik0/log-analyzer
/plugin install log-analyzer
```

See the [host validation record](https://github.com/eirenik0/log-analyzer/blob/main/docs/skill-host-validation.md)
for tested versions and limits. Host discovery and fixture checks do not measure
model quality or injection resistance.
