---
default: minor
---

Add Claude Code plugin support for cross-project skill installation:

- Add `.claude-plugin/plugin.json` manifest to enable plugin distribution
- Add `.claude-plugin/marketplace.json` for plugin marketplace discovery
- Create `skills/` symlink to support both plugin and project-level usage
- Users can install with `/plugin marketplace add` then `/plugin install log-analyzer`
- Update documentation with plugin installation instructions in README.md and CLAUDE.md
