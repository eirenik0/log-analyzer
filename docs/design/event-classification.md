# Event classification contract (versions 1 and 2)

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
not equivalent. The parser caches owned `ClassifiedRecord` evidence for all three
kinds before cleanup. Performance consumes normalized kind/name/phase/ID/scope
only; it never derives phases or identities from raw text, payload keys or parser
direction. Shipped profiles use global version-2 `event_rules`. Version-1 rules
remain supported without changing their mapping grammar. `command_rules` remains
a deprecated command-only wrapper with legacy request/event compatibility.

`EventSemantics` contains kind (`command`, `request`, `event`), nonempty name,
optional phase (`start`, `end`), optional outcome (`success`, `failure`), optional
correlation ID, transport direction, endpoint and an ordered scope vector. Outcomes are valid only for `end`.
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

The optional `[event_rules]` table requires `version = 1` or `2` and a `rules` array.
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
payload fallback occurs in version 1. Version 2 additionally reserves `payload.KEY`
for an exact top-level key in the explicitly decoded embedded payload, or the
normalized envelope payload when no embedded payload was decoded. This namespace
never overrides a direct field silently in version 1; the parser supplies the
original field map to version-1 classification. Dotted keys otherwise remain
literal, with no recursive traversal or JSON-string decoding.

Mappings use one of:

- `{ from = "literal", value = "end" }`
- `{ from = "field", field = "operation" }` (requires a string field)
- `{ from = "capture", capture = "name", decode = "raw" }`
- `{ from = "capture", capture = "name", decode = "json_string" }`
- Version 2: `{ from = "first_field", fields = ["payload.key", "payload.traceId"] }`
  tries the first **present** field in the declared order. Wrong types, empty or
  oversized values are invalid; they cannot fall through to another key. At most
  16 bounded field names are allowed. For correlation identity only, absence of
  every alternative means no ID, so correlation reports missing identity.`

Capture mappings require a named capture present in the compiled text regex.
Optional groups must actually participate at runtime, except the version-2 optional endpoint capture: an absent group means no endpoint. `raw` is the default;
`json_string` explicitly decodes a complete JSON string token, including its
quotes and escapes. Empty, whitespace-only and oversized results are invalid.
Names, IDs and scopes are preserved exactly, without trimming/case folding.
`kind` is static; name, phase, outcome, ID and scope may use mappings. Phase and
outcome string mappings must yield their exact enum spellings. Version 2 adds
optional `direction` and `endpoint` mappings. Directions are `send`/`receive` for
requests and `emit`/`receive` for events; commands cannot have a direction. Absent
direction remains `Unknown` in the record and never establishes a phase. Profiles
must map phase independently; direction is transport metadata only. Version 1
rejects these new fields and `first_field` rather than accepting new grammar under
an old contract.

Payload decoding remains separate. The classifier neither searches embedded
JSON nor recursively parses arbitrary message contents. Existing normalization
`decode_paths`/`row_decode_paths` can explicitly decode envelopes upstream; the
caller then supplies the resulting typed field map. Text captures decode only
when explicitly configured as `json_string`. Envelope and embedded payloads in
the borrowed record remain available and unchanged, including unclassified data.

Example global command-only configuration (requires the mapped trace/session fields):

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

All integrations retain classifications and outcomes in text/JSON. Identity-only
records report `identity_only`; conflicts `conflicting_event_rules`; malformed
mappings/limits `invalid_event_data`. Conflicts/invalid data retain possible kinds
and rule/profile provenance. Operation type filtering suppresses evidence only
when its declared kinds exclude the requested kind; evidence of unknown kind
remains visible. Measured pairs include `start_classification` and
`end_classification`; unmatched/orphan records retain classification and sources.

`operation_coverage.classification` partitions selected parsed records into
classified, unclassified, conflicting, invalid and unavailable evidence; identity
only and legacy are subsets of classified. Counts precede operation type filtering
and display limits. Record filters define the selected set, while pre-filter parse
coverage is unchanged. Unknown records remain searchable. The additive report
fields retain report schema version 1; `unclassified_command_records` remains a
deprecated compatibility count of unclassified records. No duration/statistic is
fabricated for unavailable classification or incomplete pairing.

## Bounds and performance

Version 1 fixes these limits: 128 rules; 16 conditions per structured rule;
16 scope mappings; 8192 UTF-8 bytes per regex; 4096 UTF-8 bytes per mapping source,
condition key/string and **decoded** resolved value; 1 MiB per original message.
JSON-string capture tokens have a separate 24,578-byte pre-decoding limit
(`6 * 4096 + 2`), accommodating the worst-case six-byte Unicode escape for every
ASCII byte plus quotes. The 4096-byte semantic limit is enforced after decoding,
so raw and escaped representations of a valid boundary-sized name agree. Any
extra token whitespace also counts against the encoded-input bound. IDs have the
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

### Final integration and migration

Shipped `eyes`, `custom-start`, `service-api`, `event-pipeline` and synchronized
skill templates use global version-2 `event_rules`; `base` declares an empty rule
set and retains generic parsing. `generate-config` shares compiled rules and
preserves explicit/legacy mode. It does not mine wording or translate directions.
Exact text grammar and canonical structured-field forms are documented in README.
Request directions/phases and event receive/start, emit/end mappings are declared
by each profile; they are application contracts, not universal defaults.

Version-1 custom rules remain supported. Legacy custom marker configurations keep
their parsing semantics through `attach_legacy_event_evidence` at the parsing seam.
The old command helper remains an alias. The deprecated `command_rules` wrapper
continues to reject request/event rules, active legacy command fields and global
`event_rules`, but may retain legacy request/event markers. Migrating to global
rules requires removing **all** legacy lifecycle fields named in validation errors,
authoring exact phase/identity mappings and using version 2 for new mapping forms.
No silent reinterpretation or removal of legacy mode occurs.

Commands map their names as IDs explicitly. Requests map an immediately following
bracketed ID (no whitespace or closing bracket); missing-ID forms retain recognized
phase evidence with no fabricated ID. Event rules declare ordered top-level
payload key alternatives; missing keys remain missing evidence. Empty scopes never
establish global correlation. Explicit mapped scope takes precedence; otherwise
configured scope fields (at most 16 names, each at most 4096 bytes) are resolved
and cached during parsing. Missing fields
leave empty scope. All kind/scope values remain bounded and perf cannot substitute
new analysis configuration, display text, directions or payload keys for cached
semantics. Legacy scope lookup retains its established compatibility behavior.
ID reuse, overlap, source ordering, offsets and inferred-year safeguards remain
in the existing correlation engine. Explicit session creation requires a start;
completion requires a nonfailed end. Legacy session hints keep their semantics.

The explicit payload decoder considers at most 16 command markers, 16 request
markers and one event separator, each at most 4096 bytes. Message size is bounded
at 1 MiB; a contiguous Aho-Corasick NFA bounds marker construction memory and scans
linearly plus reported matches (at most 33 per position). Quoted-name eligibility
uses one byte per message byte; cached whitespace runs avoid repeated suffix
scans. Earliest-position/configured-marker priority remains deterministic. A real
object/array opener is required, with balanced typed delimiters, quotes/comments
and maximum nesting depth 128 before JSON5 decoding. Only whitespace and complete
comments may follow the root. Malformed, too-deep or trailing noncomment content
stays opaque without decoding inner fragments. Header lifecycle evidence remains
available; malformed payloads cannot supply IDs. JSON5 `undefined` compatibility
converts only unquoted value tokens, preserving string identities, property names
and comments. Generic/legacy extraction and
upstream normalization retain existing behavior.

Library API: constructors leave `LogEntry.classification` absent. Manually built
operations must attach explicit normalized evidence or use the legacy adapter.
Uncached records produce `unclassified_operation_record` diagnostics; directions
alone cannot create a duration. Pair/orphan provenance and new Unknown directions
are additive pre-1.0 API changes. Redaction preserves analytic counts and typed
classification labels, while source IDs/names/scopes/endpoints and rule/profile
provenance pass through normal redaction in both text and JSON.

## Validation and review

Interface tests cover equivalent adapters, full-message rejection, Unicode and
escaped names, field types, explicit decoding, malformed data, optional captures,
identity-only evidence, missing scope, equivalent/conflicting matches, invalid
precedence, config round trips and shared compilation, validation errors and
resource limits. Regression wording from the superseded PR #32 is tested as
unclassified under a precise grammar; no heuristic implementation is copied.
Run repository formatting, locked check, Clippy with denied warnings and locked
full tests. Private-corpus evaluation is unavailable in a clean checkout. The local synthetic
evaluation corpus is compared against the previous build; stale known-failure
markers and ordering assertions are recorded separately from required checks.

Review findings are triaged as supported-behavior bugs, regressions, design
problems or extensions. After two unsuccessful hosted review rounds, reassess this
contract before another patch cycle. Merge requires passing checks, resolved
actionable findings, and hosted review evaluated on the current revision, followed
by maintainer judgment. #19 stays open until #35 merges; #32 stays closed without
merging and its branch/history are preserved.
