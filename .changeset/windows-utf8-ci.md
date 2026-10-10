---
default: patch
---

Read and write Python evaluation evidence, profiles, skill metadata and reports as
UTF-8 rather than using the host locale. Decode analyzer text output as UTF-8.
Add regressions under a simulated Windows ANSI locale and let CI platform jobs
finish independently instead of cancelling other systems after one failure.
