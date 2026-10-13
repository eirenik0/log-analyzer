#!/usr/bin/env python3
"""Measure complete selected chain construction, checking independently known links."""
import argparse
import json
import platform
import resource
import subprocess
import sys
import tempfile
import time
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path

_SPEC = spec_from_file_location("selected_probe", Path(__file__).with_name("measure-selected-investigation-memory.py"))
_SELECTED = module_from_spec(_SPEC)
_SPEC.loader.exec_module(_SELECTED)
_SHARED = _SELECTED._SHARED

PROFILE = '''extends = "base"
profile_name = "synthetic-chain-memory"
[parser]
format = "json-lines"
[event_rules]
version = 2
[[event_rules.rules]]
id = "event"
[event_rules.rules.adapter]
type = "structured"
conditions = [{field = "record_type", equals = "synthetic"}]
[event_rules.rules.mapping]
kind = "event"
name = {from = "field", field = "phase"}
correlation_id = {from = "field", field = "id"}
scope = [{from = "field", field = "session"}]
[investigation]
version = 1
'''
for _role in ("start", "step", "end"):
    PROFILE += f'''[[investigation.roles]]
id = "{_role}"
rule_ids = ["event"]
select = [{{field = "event.name", equals = "{_role}"}}]
'''
PROFILE += '''[[investigation.populations]]
id = "attempts"
entity = "attempts"
roles = ["start", "step", "end"]
identity_fields = ["id"]
grouping = "identity"
[[investigation.relationships]]
id = "start-end"
source_role = "start"
target_role = "end"
join_fields = ["id"]
required_scope_fields = ["session"]
cardinality = "one_to_one"
[[investigation.chains]]
id = "flow"
title = "Synthetic flow"
root_role = "start"
identity_fields = ["id"]
scope_fields = ["session"]
context_fields = ["session"]
'''
for _role, _source in (("step", "$root"), ("end", "step")):
    PROFILE += f'''[[investigation.chains.steps]]
id = "{_role}"
label = "{_role} observed"
role = "{_role}"
from = "{_source}"
join_fields = ["id"]
cardinality = "one_to_one"
event_type = "{_role}"
'''


def worker(binary, roots):
    with tempfile.TemporaryDirectory(prefix="chain-construction-") as directory:
        source = Path(directory) / "selected.jsonl"
        profile = Path(directory) / "profile.toml"
        artifact = Path(directory) / "evidence.json"
        profile.write_text(PROFILE, encoding="utf-8")
        timestamps = ("2026-01-01T02:00:00+02:00", "2026-01-01T02:00:01+02:00", "2026-01-01T00:00:02Z")
        with source.open("w", encoding="utf-8") as output:
            for phase, timestamp in zip(("start", "step", "end"), timestamps, strict=True):
                for index in range(roots):
                    event = {"timestamp": timestamp, "level": "INFO", "message": "synthetic selected chain é",
                             "record_type": "synthetic", "phase": phase, "id": str(index // 2),
                             "session": f"run-{index % 2}"}
                    output.write(json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n")
        source_hash = _SHARED.file_hash(source)
        command = [str(binary), "--json", "--config", str(profile), "investigate", str(source),
                   "--artifact", str(artifact), "--complete-output",
                   "--artifact-max-bytes", str(2 * 1024**3), "--input-max-bytes", str(1024**3),
                   "--construction-max-disk-bytes", str(4 * 1024**3),
                   "--processing-max-memory-bytes", str(4 * 1024**3),
                   "--processing-max-records", "1000000", "--processing-max-work", "500000000",
                   "--processing-max-ms", "600000", "--record-max-bytes", str(256 * 1024),
                   "--correlation-max-index-bytes", str(1024**2),
                   "--correlation-max-disk-bytes", str(2 * 1024**3)]
        # Each worker starts exactly one analyzer child before measuring its RSS.
        # Expected report trees are loaded only after the measured child exits.
        with tempfile.TemporaryFile() as output:
            started = time.monotonic()
            result = subprocess.run(command, stdout=output, stderr=subprocess.PIPE, env=_SHARED.environment(), timeout=660)
            elapsed = time.monotonic() - started
            rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            assert result.returncode == 0, result.stderr.decode("utf-8")
            output.seek(0)
            report = json.load(output)
        assert report["processing"]["status"] == "complete", report["processing"]
        assert report["artifact"]["status"] == "complete", report["artifact"]
        assert report["presentation"]["omitted_findings"] == 0
        assert report["report_metadata"]["evidence"]["scope"]["selected_entries"] == roots * 3
        assert report["report_metadata"]["evidence"]["inputs"][0]["sha256"] == source_hash
        assert report["artifact"]["stored_sha256"] == _SHARED.file_hash(artifact)
        retained = json.loads(artifact.read_text(encoding="utf-8"))
        assert report["findings"] == retained["findings"]
        records = retained["records"]
        assert len(records) == roots * 3
        assert len(retained["sequences"][0]["events"]) == roots * 3
        assert [r["occurrence"]["evidence_ref"]["line"] for r in records] == list(range(1, roots * 3 + 1))
        with source.open(encoding="utf-8") as original:
            for record, line in zip(records, original, strict=True):
                assert json.loads(record["raw_text"]) == json.loads(line)
        chains = [f["details"]["event_chain"] for f in report["findings"] if "event_chain" in f["details"]]
        assert len(chains) == roots
        line_of = lambda occurrence: occurrence["evidence_ref"]["line"]
        for index, chain in enumerate(chains):
            assert chain["completeness"] == "complete", chain
            assert chain["root_occurrences"] == 1
            assert line_of(chain["root"]) == index + 1
            assert chain["identity"] == [{"field": "id", "value": str(index // 2)}, {"field": "session", "value": f"run-{index % 2}"}]
            assert len(chain["steps"]) == 2
            for phase, step in enumerate(chain["steps"]):
                assert step["status"] == "observed" and step["count_completeness"] == "complete"
                assert step["observed_count"] == step["linked_count"] == 1
                assert step["omitted_occurrences"] == 0
                join, = step["join_samples"]
                assert line_of(join["source"]) == phase * roots + index + 1
                assert line_of(join["target"]) == (phase + 1) * roots + index + 1
                interval = join["endpoint_interval"]
                assert interval["status"] == "measured" and interval["value"] == 1000
                assert interval["unit"] == "ms"
                assert interval["start_timestamp"] == timestamps[phase]
                assert interval["end_timestamp"] == timestamps[phase + 1].replace("Z", "+00:00")
                assert interval["semantics"] == "selected_endpoint_interval_not_causality_or_operation_duration"
            later = list(range(index + 2, roots, 2))
            assert chain["later_activity_observed_count"] == len(later)
            assert chain["omitted_later_activity"] == max(0, len(later) - 3)
            assert [line_of(w["occurrence"]) for w in chain["later_activity"]] == [2 * roots + i + 1 for i in later[-3:]]
        relationships = [f for f in report["findings"] if f["id"].startswith("scope-0-relationship-0-")]
        assert len(relationships) == roots
        assert all(f["kind"] == "observation" for f in relationships)
        assert {(line_of(f["details"]["supporting_occurrences"][0]), line_of(f["details"]["supporting_occurrences"][1])) for f in relationships} == {(i + 1, 2 * roots + i + 1) for i in range(roots)}
        population, = [p for p in retained["populations"] if p["id"] == "scope-0-policy-0"]
        assert population["count"] == roots and population["completeness"] == "complete"
        membership, = [m for m in retained["memberships"] if m["population_id"] == population["id"]]
        assert len(membership["members"]) == roots
        assert {tuple(line_of(o) for o in member["source_occurrences"]) for member in membership["members"]} == {(i + 1, roots + i + 1, 2 * roots + i + 1) for i in range(roots)}
        return {"roots": roots, "selected_records": roots * 3, "supported_chain_links": roots * 2,
                "supported_relationships": roots, "delivery": "complete",
                "source_bytes": source.stat().st_size, "source_sha256": source_hash,
                "artifact_bytes": artifact.stat().st_size,
                "peak_rss_bytes": rss if sys.platform == "darwin" else rss * 1024,
                "elapsed_seconds": round(elapsed, 4)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--roots", type=int, nargs="+", default=[100, 500, 2000])
    parser.add_argument("--worker", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--rss-ceiling-mib", type=int, default=96)
    parser.add_argument("--rss-growth-mib", type=int, default=24)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.worker is not None:
        if args.worker <= 0:
            parser.error("worker root count must be positive")
        print(json.dumps(worker(binary, args.worker)))
        return
    if any(n <= 0 for n in args.roots):
        parser.error("root counts must be positive")
    measurements = []
    for count in args.roots:
        result = subprocess.run([sys.executable, str(Path(__file__).resolve()), str(binary), "--worker", str(count)], capture_output=True, check=True, timeout=700)
        measured = json.loads(result.stdout)
        measurements.append(measured)
        print(f"{count} selected roots: {measured['peak_rss_bytes']} bytes RSS, {measured['elapsed_seconds']}s", file=sys.stderr)
    result = {"scope": "end-to-end complete CLI output; every record selected; all declared chain steps and scoped relationships supported; two interleaved sessions reuse IDs; independent source-line and exact interval assertions",
              "platform": platform.platform(), "binary_sha256": _SHARED.file_hash(binary),
              "record_bytes": 256 * 1024, "correlation_index_bytes": 1024**2,
              "processing_memory_bytes": 4 * 1024**3, "processing_records": 1000000,
              "construction_disk_bytes": 4 * 1024**3,
              "rss_ceiling_bytes": args.rss_ceiling_mib * 1024**2,
              "rss_growth_tolerance_bytes": args.rss_growth_mib * 1024**2, "measurements": measurements}
    print(json.dumps(result, indent=2))
    _SELECTED.check_rss([m["peak_rss_bytes"] for m in measurements], result["rss_ceiling_bytes"], result["rss_growth_tolerance_bytes"])


if __name__ == "__main__":
    main()
