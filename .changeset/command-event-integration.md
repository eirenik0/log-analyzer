---
"log-analyzer": minor
---

BREAKING CHANGE (pre-1.0): shipped command profiles now use explicit whole-message grammar instead of incidental substring matches. Migrate custom formats by authoring command_rules and removing legacy command identity/phase fields; legacy-only custom marker grammar is retained without automatic translation. LogEntry now exposes cached classification evidence; caller-constructed command records must provide evidence (or explicitly attach legacy compatibility evidence) for timing analysis. All command pairing requires nonempty correlation scope.

Recognize independent command start/completion records across shipped profiles, fixing the service-api 1500 ms reproduction. Cache phases before payload cleanup and consume normalized evidence in the existing correlation engine. Report start-only, end-only, identity-only, conflicting, invalid and unknown evidence honestly. Preserve offsets, scoped ambiguity safeguards and text/JSON selection. Explicit session completion requires a nonfailed end; command names/starts alone do not complete sessions. Bound new command payload decoding and retain malformed/deep payloads without changing legacy payload parsing. Synchronize profile/skill templates, migration documentation, generated-profile mode and CLI help.
