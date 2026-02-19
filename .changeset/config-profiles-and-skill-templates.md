---
default: minor
---

Add configurable analyzer profiles and skill templates for custom log formats:

- Added runtime profile loading via `--config <path>` and `LOG_ANALYZER_CONFIG`
  - Parser rules (event/command/request markers) are now configurable
  - Performance pairing markers and correlation keys are configurable
  - Added profile-aware `info` insights for unknown components/commands/requests and session prefixes
- Added reusable profile files under `config/profiles/`
  - `base.toml` for generic defaults
- Added copy-and-customize templates under `config/templates/`
  - `custom-start.toml`
  - `service-api.toml`
  - `event-pipeline.toml`
- Added matching templates inside the installed skill package at `.claude/skills/analyze-logs/templates/`
  - Enables users to install the skill and immediately scaffold their own profile
- Updated skill and CLI docs to support template-based onboarding
  - README profile-template workflow
  - `analyze-logs` skill docs/reference template instructions
  - `scripts/install-skill.sh` now prints profile bootstrap commands
