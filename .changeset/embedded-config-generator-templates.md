---
default: minor
---

Add generated profile scaffolding and embedded built-in templates:

- Added `generate-config` command (alias: `gen-config`) to analyze a log file and output a TOML profile.
  - Collects known components, commands, requests, and session prefix hints from parsed logs.
  - Supports `--profile-name` and `--template`, and writes to stdout or `-o/--output`.
- Added config generation module and tests for collection logic, inheritance behavior, prefix detection, and TOML roundtrip.
- Embedded profile/template TOML assets into the binary:
  - `config/profiles/base.toml`
  - `config/templates/custom-start.toml`
  - `config/templates/service-api.toml`
  - `config/templates/event-pipeline.toml`
- Updated default configuration loading to use embedded `base.toml`, improving portability when external files are unavailable.
- Extended template resolution in `generate-config` so `--template` accepts either:
  - a filesystem path, or
  - a built-in template name (`base`, `custom-start`, `service-api`, `event-pipeline`).
- Added matching `base.toml` in skill templates and updated skill/reference/docs to reflect built-in template usage and profile generation flow.
