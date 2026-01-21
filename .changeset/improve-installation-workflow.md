---
default: patch
---

Improve installation workflow and documentation:

- Fix `scripts/install-skill.sh` to use repository directory instead of current working directory
- Change default install location from `/usr/local/bin` to `$HOME/bin` (no sudo required)
- Add automatic PATH setup instructions for zsh, bash, and fish shells
- Recommend WSL for Windows users instead of native binary
- Rewrite README.md to be more compact and user-friendly (~50% smaller)
- Add Claude Code Integration section with `/analyze-logs` skill examples
