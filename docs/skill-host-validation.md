# Portable skill validation

Checked on 2026-10-09 on macOS with Python 3.13.5. The Rust executable was the
locally built `log-analyzer 0.3.0`, base revision `9b35994`, dirty worktree. This
record describes specific compatibility checks, not a minimum host version or
a claim that every provider/model supports the workflow equally well.

## Deterministic checks

`python3 -m unittest discover -s scripts -p 'test_install_skill.py'` checks:

- All three hosts with project and user scope, including legacy no-flags,
  `--global` and `-g` invocations and conflicting/invalid options.
- Complete standalone resources, relative Markdown links, repeated installs and
  source-checkout no-ops, with spaces in both source and destination paths.
- Existing directory symlinks, symlink overlap, dangling destinations, ancestor
  and descendant overlap rejection, and child symlink overwrite prevention.
- Missing executable guidance and discovery without claiming compatibility.
- Generated Markdown tolerates Windows CRLF checkout conversion while rejecting
  actual workflow-content drift.
- Portable name/description metadata, optional Codex display metadata, Pi package
  paths and Claude plugin paths, and no drift in the generated Claude bundle.

`python3 scripts/sync-skills.py --check` checks the materialized Claude bundle.
`quick_validate.py` from Codex's skill-creator validated the canonical skill.
The maintained `scripts/check-examples.py` checks the same executable fixtures:
all documented commands and eight investigations passed, including unsupported
inputs/profiles, reused IDs, incomplete captures, budget stops and citations.
`cargo test --locked` also validates those pages and findings against advertised
schemas and runs the credential-free evaluation corpus. Scripted checks are not
real-model evaluations and establish no model accuracy or token savings.

## CLI and skill refresh (2026-10-10)

The PR #84 follow-up uses canonical `profile`/`evidence` commands, JSON defaults,
and the shared `--summary` option. The canonical skill and generated Claude bundle
were synchronized and validated. The current deterministic checks cover maintained
command examples, eight workflow cases, and nine profile-discovery/recovery cases
without injected profiles, including summary-schema validation and retained evidence.
Relative links and README anchors were checked locally. Python checks passed
(47 script tests and 63 evaluation tests).

The native host observations below predate these CLI/skill changes. They have not
been rerun for this revision, so they do not demonstrate current host activation,
model adherence, accuracy, reliability gains, or token savings. See the
[discovery evaluation protocol](../evals/README.md#profile-discovery-and-recovery)
for the separate model trials needed to measure those outcomes.

## Native host smoke checks

Each smoke installed the skill in a temporary project outside the source
repository. Only synthetic `examples/investigations/failure.jsonl`,
`failure.expected.json` and `profile.toml` were supplied. The question asked what
failed, its lifecycle duration, and whether the capture established a cause.
The executable was copied to a temporary PATH directory. Invocations used
`--report-max-items 3` and `--report-max-bytes 18000` for bounded reports.
No session transcripts, credentials or customer data are committed. Hosted review
subsequently replaced the long bundled command catalog with focused retrieval
and citation notes; command syntax now routes to actual CLI help and the README.
The smoke observations below precede this reference-only cleanup.

- **Codex CLI 0.160.0:** `.agents/skills` discovery and explicit `$analyze-logs`
  invocation worked. The agent resolved the installed skill, host setup,
  reference and failure example; checked capabilities and parse/profile
  suitability; followed validation and performance cursors; cited lines 1 and 3
  for 2000 ms elapsed; distinguished the `request-70` substring hit; and reported
  the cause as unknown. Initial 100,000-byte global-budget trials exceeded that
  ceiling through preflight/reference reads despite bounded report pages. The
  skill now explicitly charges preflight/reference reads and recommends retaining
  large capability schemas locally. This instruction is guidance, not enforced
  metering by the skill; consuming hosts must enforce their own ceilings. A final
  trial with an explicit 300,000-byte ceiling completed in 75.6 seconds using
  seven shell tool calls and 218,657 bytes of aggregated tool output; reports
  retained the 3-item/18,000-byte page limits.
- **Pi 1.1.0:** native resource loading discovered the installed project
  `.agents/skills/analyze-logs/SKILL.md`. Explicit `/skill:analyze-logs` expanded
  the full workflow and its installed resource base into a user message. The
  provider then rejected inference because its usage allowance was exhausted.
  Exit code 0 did not mean an investigation passed: the assistant event had
  `stopReason: error` and zero model tokens. A separate native
  `DefaultResourceLoader` check with the local repository as a package discovered
  the canonical skill through `pi.skills` with no diagnostics. End-to-end Pi
  inference and Git transport installation remain unverified.
- **Claude Code 2.1.271:** project `.claude/skills` discovery and explicit
  `/analyze-logs` invocation worked with project setting sources enabled. A
  separate `--plugin-dir` run discovered `/log-analyzer:analyze-logs` through the
  explicit plugin `skills` path. Both forked wrappers read the shared workflow
  and completed the synthetic investigation: failure, 2000 ms boundaries,
  unrelated substring hit, and unknown cause. Startup skill lists and final
  outputs were inspected. Fork-internal tool accounting was not independently
  available in the outer transcript, so overall budget compliance is unverified.

The smoke runs used each host's configured default model. These individual
observations do not measure model quality or prove injection resistance. Native
host checks are optional and require available hosts/provider access; CI does
not silently replace them with credential-free scripted checks.

## Reproduce the native checks

Build the binary, create an isolated project, copy only the three synthetic
fixtures, and run the installer from there for the desired host. Put the built
executable on that process's PATH. Supply absolute fixture/profile paths and an
explicit call/output/time budget. For Codex, use:

```sh
codex exec --ignore-user-config --skip-git-repo-check --ephemeral \
  --sandbox read-only --json '$analyze-logs <synthetic question and absolute paths>'
```

For Pi, use its installed shared project skill (grant project trust for this
known synthetic fixture) and inspect the JSON event stream for provider errors:

```sh
pi --print --no-session --no-extensions --no-mcp --no-prompt-templates \
  --no-themes --offline --approve --tools read,bash --mode json \
  '/skill:analyze-logs <synthetic question and absolute paths>'
```

For Claude, load project skills and restrict MCP/configuration to the test:

```sh
claude --print --no-session-persistence --setting-sources project \
  --strict-mcp-config --mcp-config '{"mcpServers":{}}' \
  --allowedTools 'Read,Bash(log-analyzer:*)' --output-format stream-json --verbose \
  '/analyze-logs <synthetic question and absolute paths>'
```

For the plugin variant, add `--plugin-dir /absolute/log-analyzer-checkout` and use
`/log-analyzer:analyze-logs`. Inspect actual discovery, invocation expansion,
capability/profile checks, pages and citations; process exit alone is insufficient.

## Remaining gaps

Native user-scope discovery, Codex desktop skill-selector display, Pi Git
installation, marketplace network installation, Windows host execution and
repeated reloads in already-running sessions were not exercised. Unit tests
cover user-scope destinations and packaged paths; documentation describes the
host discovery paths verified against current primary sources. The materialized
bundle uses regular files so symlink support is not required for distribution.
