# Domain concepts

These terms describe the implemented evidence model. They keep source identity, declared event meaning and conclusions separate; detailed field contracts live in [[contracts]].

## Records and coverage

A [[src/parser/entities.rs#LogEntry|record]] is a parsed entry with source location, payload and timestamp provenance. It can span physical lines or be one normalized row inside a JSON entry.

[[src/parser.rs#ParseCoverage]] counts parsing outcomes and retains rejected or
unsupported structure. Parsed records, filter-selected records and displayed
records are different populations. No matches, no recognized input and a measured
zero are distinct results. Parsing success alone does not establish lifecycle
coverage or upstream capture completeness.

## Events and operations

An [[src/event_rules/mod.rs#EventSemantics|event]] is a profile's interpretation of a record: kind, name, optional phase, identity, scope and outcome.

[[src/event_rules/mod.rs#ClassifiedRecord]] retains recognized, unclassified,
conflicting or invalid evidence. An identity-only event has no lifecycle boundary.
A [[src/perf_analyzer/entities.rs#TimedOperation|timed operation]] requires a valid
start/end pair. The [[src/perf_analyzer/mod.rs#CorrelationKey|pairing key]] includes
kind, name, correlation ID and scope, so a reused ID alone cannot connect events.
An intentional start-only event is neither an orphan nor a completed operation.
See [[contracts/event-classification]].

## References and occurrences

An [[src/evidence.rs#EvidenceReference|evidence reference]] locates a source line and optional normalized row. An [[src/investigation.rs#OccurrenceId|occurrence]] additionally binds it to a snapshot and declared input ordinal.

The same file supplied twice can yield two occurrences without becoming two
physical sources. [[src/evidence.rs#Context]] tracks input, effective-profile and
query identities. Hashes identify consumed content; they do not authenticate it,
prove capture completeness or establish correlation. See
[[contracts/evidence-contract#Evidence contract (version 1)#Input and profile identity]].

## Findings and populations

[[src/investigation_runtime/findings.rs#build|Findings]] report observations, measurements, calculated facts and unknowns, with scope, evidence and limitations.

A population defines what is counted and retains its membership. A distribution
names contributing measurements and its method. Records, paired operations,
logical operations and distinct resources are not interchangeable count units.
Assessments describe support for a goal within a scope. Hypotheses remain authored
interpretations; deterministic findings do not manufacture causal explanations.
See [[contracts/investigation-contract#Investigation report contract version 1#Facts and support]].

## Chains and intervals

[[src/investigation_runtime/chains.rs#calculate|Event chains]] connect a root to configured steps through declared identities, scope and cardinality. Missing join identity remains incomplete evidence.

[[src/investigation_runtime/chains.rs#endpoint_interval|Endpoint intervals]] measure
elapsed time only for unambiguous endpoints with explicit source timestamps and
nonnegative elapsed time. They preserve offsets and fractional precision; they do
not establish operation duration, synchronized clocks or causality.

## Reports and artifacts

A report is a presentation of findings and coverage. A retained artifact stores evidence, effective rules, populations and processing state for historical retrieval.

Processing limits affect what was analyzed; presentation limits affect what is
shown. Artifact integrity, arithmetic reproducibility and independent source/rule
checking are separate guarantees. Redaction can leave arithmetic checkable while
removing the sources needed for independent checking. See
[[contracts/investigation-contract#Investigation report contract version 1#Retained artifact and historical retrieval]].
