# Event classification contract (version 1)

Delivery owner: #33. Module/configuration: #34; commands: #35; requests/events
and shared reporting: #36. This document owns the technical contract.

## Boundary and interfaces

`record parsing -> event rules -> normalized lifecycle events -> scoped correlation -> reporting`

`AnalyzerConfig.event_rules` is an optional `CompiledEventRules`, exposed through
`config` together with `EventRuleConfig`. TOML deserialization validates and
compiles the schema once. Cloning a loaded configuration shares immutable rules
via `Arc`; serializing it writes the schema only. Programmatic users construct
rules with `CompiledEventRules::compile` and call
`AnalyzerConfig::validate_event_rules` after assembling their configuration.
There is no mutable schema/cache pair that can drift out of sync.

`classify(profile_name, RecordInput)` is pure. The input borrows the existing
`LogEntry`, the **original message before payload/display cleanup**, and either a
JSON field map or the parser's flat string field map. Callers must supply that
original message at the parsing seam; passing `entry.message` after cleanup is
not equivalent. This PR deliberately does not change the parser or performance
analysis. Loading rules alone does not enable a new CLI analysis path; production
integration is delivered by #35/#36. Existing configurations and shipped profiles
retain their current behavior.

`EventSemantics` contains kind (`command`, `request`, `event`), nonempty name,
optional phase (`start`, `end`), optional outcome (`success`, `failure`), optional
correlation ID and an ordered scope vector. Outcomes are valid only for `end`.
No phase means identity only, never lifecycle evidence. A configured ID/scope
mapping is required to resolve; an absent mapping is explicitly absent evidence,
not a fabricated ID or global scope. Integration must apply the existing
correlation completeness/ambiguity safeguards before pairing. No duration is
computed by classification.

`NormalizedEvent` borrows the existing record for timestamp, explicit original
offset (`source_timestamp`), inferred-year flag, component, source file/line/row,
raw line, and normalized record. It also retains profile name and **all** matching
rule IDs. Local/naive timestamps remain local/naive evidence; an inferred year is
not promoted to a known year. This preserves provenance without copying a second
log record or making an invented completion claim.

## Grammar and adapters

The optional `[event_rules]` table requires `version = 1` and a `rules` array.
Each rule requires unique `id`, `adapter`, and `mapping`. Unknown keys in this
schema are errors. Rule IDs are 1..128 ASCII letters, digits, `_`, `-`, or `.`.

A text adapter has `type = "text"` and a Rust regex `pattern`. The compiler wraps
the pattern in `\A(?:pattern)\z`; inline flags cannot relax the outer absolute
anchors. A keyword embedded in prose, a trailing newline, or an extra clause
cannot establish a boundary unless the profile **explicitly** includes it in its
whole-message grammar. Profiles should enumerate real producer messages and
avoid permissive wildcards around lifecycle words. Matching contains no English
negation, clause, parenthetical, or adverb heuristics. Backreferences/lookaround
remain unsupported by the existing regex crate.

A structured adapter has `type = "structured"` and 1..16 `conditions`. Every
condition has an exact top-level `field` key and scalar `equals`. Conditions are
ANDed; field names are case-sensitive literal keys, not JSON pointers. Typed JSON
strings, booleans and numbers match by `serde_json::Value` equality; missing,
null, array, object or wrong-type fields do not match. Flat parser fields match
only string conditions: `"true"` is not boolean `true`. No implicit coercion or
payload fallback occurs.

Mappings use one of:

- `{ from = "literal", value = "end" }`
- `{ from = "field", field = "operation" }` (requires a string field)
- `{ from = "capture", capture = "name", decode = "raw" }`
- `{ from = "capture", capture = "name", decode = "json_string" }`

Capture mappings require a named capture present in the compiled text regex.
Optional groups must actually participate at runtime. `raw` is the default;
`json_string` explicitly decodes a complete JSON string token, including its
quotes and escapes. Empty, whitespace-only and oversized results are invalid.
Names, IDs and scopes are preserved exactly, without trimming/case folding.
`kind` is static; name, phase, outcome, ID and scope may use mappings. Phase and
outcome string mappings must yield their exact enum spellings.

Payload decoding remains separate. The classifier neither searches embedded
JSON nor recursively parses arbitrary message contents. Existing normalization
`decode_paths`/`row_decode_paths` can explicitly decode envelopes upstream; the
caller then supplies the resulting typed field map. Text captures decode only
when explicitly configured as `json_string`. Envelope and embedded payloads in
the borrowed record remain available and unchanged, including unclassified data.

Example configuration (module contract only, not an enabled CLI workflow):

```toml
profile_name = "synthetic"

[event_rules]
version = 1

[[event_rules.rules]]
id = "command-end"
[event_rules.rules.adapter]
type = "text"
pattern = 'Operation (?P<name>"(?:[^"\\]|\\.)*") completed'
[event_rules.rules.mapping]
kind = "command"
name = { from = "capture", capture = "name", decode = "json_string" }
phase = { from = "literal", value = "end" }
outcome = { from = "literal", value = "success" }
correlation_id = { from = "field", field = "trace_id" }
scope = [{ from = "field", field = "session" }]
```

## Outcomes and match policy

- `Unclassified`: no adapter matched; retain the searchable record.
- `Recognized`: one unique lifecycle interpretation; retain matching rule IDs.
- `IdentityOnly`: one unique identity with no phase; never send to lifecycle pairing.
- `Conflict`: matched rules disagree on any normalized semantic field, including
  missing versus present phase/outcome/ID or scope order; retain every match ID,
  emit no usable event. Rule order is not priority.
- `Invalid`: a matching adapter has malformed mapped data, or the message exceeds
  the input limit; retain bounded diagnostics and emit no usable event.

Multiple equivalent matches are accepted once and retain every rule ID in
configuration order. Identity and lifecycle matches conflict rather than allowing
a lifecycle rule to silently override identity-only evidence. If a matching rule
is invalid, `Invalid` takes precedence over other valid or conflicting matches;
its diagnostics identify the invalid rule and semantic target. No valid sibling
can hide malformed matching evidence. Static enum values are checked at compile
time; dynamic values are checked per record. Condition mismatch is unclassified,
not malformed mapped data, because the rule has not asserted an event identity.

Integration must keep these outcomes visible in text and JSON and must not report
unavailable analysis as measured zero. Those reporting changes belong to #35/#36.

## Bounds and performance

Version 1 fixes these limits: 128 rules; 16 conditions per structured rule;
16 scope mappings; 8192 UTF-8 bytes per regex; 4096 UTF-8 bytes per mapping source,
condition key/string and resolved value; 1 MiB per original message. IDs have the
separate 128-byte limit. Regex compiled size and DFA cache size are each limited
to 1 MiB per rule. Invalid syntax or compiled-size failure rejects the profile.
There is no truncation of semantic values or partial matching of oversized input.

Classification checks the message limit before scanning. It visits at most 128
rules, calls each regex once, accesses only explicitly named fields and copies
only bounded mapped strings. Regex execution has the regex crate's linear-input
bound for a fixed pattern; total work is bounded by rule/pattern sizes times
message size. Structured conditions compare scalars, with no container traversal.
Diagnostics use fixed reason/target strings and borrowed validated IDs: at most
one per matching invalid rule (128), or one message-limit diagnostic. Input text
and payloads are never embedded in diagnostics.

Limits apply to the compiled event-rule schema and classification. The existing
TOML loader parses a caller-provided config before semantic validation; these
limits do not claim to bound TOML parsing, upstream record/payload decoding, or
memory already owned by the caller. Decoding/resource policy for those layers
is separate. JSON field maps are borrowed without serialization or recursive
validation, so unrelated large payload objects are not visited. UTF-8 byte limits
use lengths, never unsafe byte slicing; regex/JSON string decoding preserve
Unicode. No regex compilation occurs per record.

## Versioning and legacy migration

Missing `event_rules` means legacy mode. Existing custom parser/performance
marker settings continue with their existing meaning; no automatic translation,
substring reinterpretation, or implicit rule generation is permitted.
Unsupported versions fail loading instead of falling back to legacy behavior.
Changing version-1 semantics or grammar requires a new schema version and an
explicit migration description.

A file containing explicit rules **and** any active legacy event emit/receive,
command prefix/start, request prefix/send/receive, or performance command
start/completion markers is rejected. Empty marker vectors/strings assert no
legacy semantics. Parser format, module rules, payload extraction indicators,
endpoint/payload separators, normalization, correlation keys/scopes, session hints
and known-name lists are orthogonal and may coexist. Loading does not select a
winner. Programmatic configurations must run the same mixed-mode validation.

Before #35 enables command integration, shipped command profiles must get explicit
rules with synthetic producer fixtures. A custom-profile author who opts in must
remove the active legacy identity/phase markers and write full-message or
structured rules preserving their intended semantics. Retain legacy-only custom
profiles until their authors opt in; neither generated profiles nor templates may
silently change modes. Any future removal of legacy mode requires a separately
announced breaking change and migration tooling/documentation. Request/event
integration and complete shipped-profile mappings follow in #36.

## Validation and review

Interface tests cover equivalent adapters, full-message rejection, Unicode and
escaped names, field types, explicit decoding, malformed data, optional captures,
identity-only evidence, missing scope, equivalent/conflicting matches, invalid
precedence, config round trips and shared compilation, validation errors and
resource limits. Regression wording from the superseded PR #32 is tested as
unclassified under a precise grammar; no heuristic implementation is copied.
Run repository formatting, locked check, Clippy with denied warnings and locked
full tests. Private-corpus evaluation is unavailable in a clean checkout.

Review findings are triaged as supported-behavior bugs, regressions, design
problems or extensions. After two unsuccessful hosted review rounds, reassess this
contract before another patch cycle. Merge requires passing checks, resolved
actionable findings, and hosted review evaluated on the current revision, followed
by maintainer judgment. #19 stays open until #35 merges; #32 stays closed without
merging and its branch/history are preserved.
