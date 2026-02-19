---
default: minor
---

Add configurable profiles and starter templates for custom log formats:

- Added runtime profile loading via `--config <path>` or `LOG_ANALYZER_CONFIG`.
- Parser markers, pairing markers, and correlation keys are now configurable through profiles.
- Added profile-aware `info` insights for unknown components, commands, requests, and session prefixes.
- Added reusable templates: `base`, `custom-start`, `service-api`, and `event-pipeline`.
