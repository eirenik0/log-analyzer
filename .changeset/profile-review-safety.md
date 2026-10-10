---
default: patch
---

Keep automatic profile detection usable with blank inputs, preserve sanitized
selection and coverage diagnostics under redaction, and protect unread profile
files when directory discovery stops at a limit. Require the advertised profile
selector and default detection contracts in the capabilities schema.

Apply shared discovery/detection budget cutoffs to every declared input scope,
even when generic parsing can finish after probe allocations are released.
