---
"log-analyzer": minor
---

Add top-level `extends` to TOML profiles for inheriting built-in profiles or files relative to the child. Tables merge key by key with the child winning; arrays and scalars replace inherited values. Reject cycles, unreadable parents, invalid references, and chains exceeding eight profiles, including the root and any built-in parent.
