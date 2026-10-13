#!/usr/bin/env python3
"""Measure end-to-end investigation RSS with growing, entirely selected inputs."""
import argparse
import hashlib
import json
import platform
import resource
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from importlib.util import module_from_spec, spec_from_file_location

_SPEC = spec_from_file_location("retrieval_probe", Path(__file__).with_name("measure-artifact-retrieval-memory.py"))
_SHARED = module_from_spec(_SPEC)
_SPEC.loader.exec_module(_SHARED)


def worker(binary, count, complete_output):
    profile = Path(__file__).resolve().parents[1] / "examples/investigations/profile.toml"
    with tempfile.TemporaryDirectory(prefix="selected-investigation-") as directory:
        source = Path(directory) / "selected.jsonl"
        artifact = Path(directory) / "evidence.json"
        with source.open("w", encoding="utf-8") as output:
            for phase, timestamp in (("start", "2026-01-01T02:00:00+02:00"), ("end", "2026-01-02T00:00:00Z")):
                for index in range(count // 2):
                    output.write(json.dumps({"ts": timestamp, "level": "INFO", "phase": phase, "id": str(index),
                                             "operation": "fetch", "session": "selected-run", "outcome": "success",
                                             "message": "selected synthetic boundary é"}, ensure_ascii=False, separators=(",", ":")) + "\n")
        source_hash = _SHARED.file_hash(source)
        command = [str(binary), "--json", "--config", str(profile), "investigate", str(source),
                   "--artifact", str(artifact), "--artifact-max-bytes", str(2 * 1024**3),
                   "--input-max-bytes", str(1024**3), "--processing-max-memory-bytes", str(4 * 1024**3),
                   "--processing-max-records", "1000000", "--processing-max-work", "500000000",
                   "--processing-max-ms", "600000", "--record-max-bytes", str(256 * 1024),
                   "--correlation-max-index-bytes", str(1024**2),
                   "--correlation-max-disk-bytes", str(2 * 1024**3)]
        if complete_output:
            command.append("--complete-output")
        # No child has run in this worker before this invocation. Parse expected
        # data only afterwards so it cannot be inherited by a forked CLI child.
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
        manifest = report["report_metadata"]["evidence"]
        assert manifest["scope"]["selected_entries"] == count
        assert manifest["inputs"][0]["sha256"] == source_hash
        assert report["artifact"]["stored_sha256"] == _SHARED.file_hash(artifact)
        retained = json.loads(artifact.read_text(encoding="utf-8"))
        records = retained["records"]
        assert len(records) == count
        assert [r["occurrence"]["evidence_ref"]["line"] for r in records] == list(range(1, count + 1))
        with source.open(encoding="utf-8") as original:
            for record, line in zip(records, original, strict=True):
                assert json.loads(record["raw_text"]) == json.loads(line)
        measurements = [f for f in retained["findings"] if f["kind"] == "measurement"]
        assert len(measurements) == count // 2
        assert all(f["details"]["value"] == 86400000 for f in measurements)
        pairs = set()
        for finding in measurements:
            boundaries = finding["details"]["boundaries"]
            start = boundaries["start"]["occurrence"]["evidence_ref"]["line"]
            end = boundaries["end"]["occurrence"]["evidence_ref"]["line"]
            assert 1 <= start <= count // 2 and end == start + count // 2
            assert boundaries["start"]["timestamp"] == "2026-01-01T02:00:00+02:00"
            assert boundaries["end"]["timestamp"] == "2026-01-02T00:00:00+00:00"
            pairs.add((start, end))
        assert len(pairs) == count // 2
        distributions = [f for f in retained["findings"] if f["details"].get("calculation") == "distribution"]
        assert {f["details"]["statistic"] for f in distributions} == {"minimum", "maximum", "p50", "p95", "p99"}
        assert all(f["details"]["sample_count"] == count // 2 and f["details"]["value"] == 86400000 for f in distributions)
        assert len(retained["sequences"][0]["events"]) == count
        assert report["presentation"]["total_findings"] == len(retained["findings"])
        if complete_output:
            assert report["findings"] == retained["findings"]
            assert report["presentation"]["omitted_findings"] == 0
        return {"selected_records": count, "paired_measurements": len(measurements),
                "delivery": "complete" if complete_output else "default_page",
                "source_bytes": source.stat().st_size, "source_sha256": source_hash,
                "artifact_bytes": artifact.stat().st_size,
                "peak_rss_bytes": rss if sys.platform == "darwin" else rss * 1024,
                "elapsed_seconds": round(elapsed, 4)}


def check_rss(peaks, ceiling, growth):
    assert max(peaks) <= ceiling, "absolute RSS ceiling exceeded"
    assert max(peaks) - min(peaks) <= growth, "RSS growth tolerance exceeded"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--records", type=int, nargs="+", default=[1000, 5000, 20000])
    parser.add_argument("--worker", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--complete-output", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--rss-ceiling-mib", type=int, default=96)
    parser.add_argument("--rss-growth-mib", type=int, default=24)
    parser.add_argument("--baseline", action="store_true", help="Record the old implementation without enforcing the RSS bounds")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.worker is not None:
        if args.worker <= 0 or args.worker % 2:
            parser.error("worker record count must be positive and even")
        print(json.dumps(worker(binary, args.worker, args.complete_output)))
        return
    if any(n <= 0 or n % 2 for n in args.records):
        parser.error("record counts must be positive and even")
    measurements = []
    for complete in (False, True):
        for count in args.records:
            command = [sys.executable, str(Path(__file__).resolve()), str(binary), "--worker", str(count)]
            if complete:
                command.append("--complete-output")
            result = subprocess.run(command, capture_output=True, check=True, timeout=700)
            measured = json.loads(result.stdout)
            measurements.append(measured)
            print(f"{count} selected ({measured['delivery']}): {measured['peak_rss_bytes']} bytes RSS, {measured['elapsed_seconds']}s", file=sys.stderr)
    peaks = [row["peak_rss_bytes"] for row in measurements]
    result = {"scope": "end-to-end CLI: capture, parse, select, correlate, construct, validate, publish artifact and deliver report; every record selected; every start precedes all completions",
              "platform": platform.platform(), "binary_sha256": _SHARED.file_hash(binary),
              "baseline": args.baseline, "record_bytes": 256 * 1024, "correlation_index_bytes": 1024**2,
              "processing_memory_bytes": 4 * 1024**3, "processing_records": 1000000,
              "rss_ceiling_bytes": args.rss_ceiling_mib * 1024**2,
              "rss_growth_tolerance_bytes": args.rss_growth_mib * 1024**2, "measurements": measurements}
    print(json.dumps(result, indent=2))
    if not args.baseline:
        check_rss(peaks, result["rss_ceiling_bytes"], result["rss_growth_tolerance_bytes"])


if __name__ == "__main__":
    main()
