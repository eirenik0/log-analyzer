---
default: minor
---

Add profile generation command and built-in template support:

- Added `generate-config` (`gen-config`) to create a TOML profile from a log file.
- Generated profiles include discovered components, commands, requests, and session prefix hints.
- Supports `--profile-name`, `--template`, and `-o, --output`.
- `--template` now accepts either a file path or built-in template name (`base`, `custom-start`, `service-api`, `event-pipeline`).
- Embedded built-in templates into the binary and use embedded `base` as default for better portability.
