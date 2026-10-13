#!/usr/bin/env python3
"""Measure yielding parser RSS on real JSONL records, using a fresh worker per size."""
import argparse
import hashlib
import json
import os
import platform
import resource
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def measure(binary, records, investigate=False):
    row = b'{"timestamp":"2026-01-01T00:00:00Z","message":"synthetic record"}\n'
    with tempfile.TemporaryDirectory(prefix="streaming-memory-") as directory:
        source = Path(directory) / "records.jsonl"
        digest = hashlib.sha256()
        with source.open("wb") as stream:
            for _ in range(records):
                stream.write(row)
                digest.update(row)
        started = time.monotonic()
        command = [str(binary), str(source)]
        if investigate:
            command = [str(binary), "--json", "--profile", "base", "--filter", "t:absent",
                       "investigate", str(source), "--artifact", str(Path(directory) / "evidence.json"),
                       "--processing-max-memory-bytes", "8388608"]
        env = {key: value for key, value in os.environ.items() if not key.startswith("LOG_ANALYZER_")}
        env.update(TZ="UTC", NO_COLOR="1")
        output = subprocess.run(command, check=True, capture_output=True, env=env)
        elapsed = time.monotonic() - started
        value = json.loads(output.stdout)
        if investigate:
            assert value["processing"]["status"] == "complete", value
            assert value["artifact"]["status"] == "complete", value
            observed = value["report_metadata"]["evidence"]["inputs"][0]
            assert observed["selected_entries"] == 0, value
            coverage = observed["coverage"]
        else:
            assert value["records"] == records, value
            coverage = value["coverage"]
        assert coverage["parsed_entries"] == records, value
        assert coverage["rejected_candidates"] == 0, value
        assert coverage["snapshot_sha256"] == digest.hexdigest(), value
        rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        return {"records": records, "input_bytes": source.stat().st_size,
                "peak_rss_bytes": rss if sys.platform == "darwin" else rss * 1024,
                "elapsed_seconds": round(elapsed, 4), "sha256": digest.hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--records", nargs="+", type=int, default=[1000, 100000, 1000000])
    parser.add_argument("--worker", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--investigate", action="store_true",
                        help="Use the CLI with an excluding filter and fixed 8 MiB accounting allowance")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.worker is not None:
        print(json.dumps(measure(binary, args.worker, args.investigate)))
        return
    if any(count < 1 for count in args.records):
        parser.error("record counts must be positive")
    measurements = []
    for count in args.records:
        command = [sys.executable, str(Path(__file__).resolve()), str(binary), "--worker", str(count)]
        if args.investigate:
            command.append("--investigate")
        output = subprocess.run(command, check=True, capture_output=True)
        measurements.append(json.loads(output.stdout))
    scope = ("filtered investigation with zero selected records; includes source capture and artifact publication; excludes growing correlation state and artifact retrieval"
             if args.investigate else "yielding parser with a non-retaining consumer; excludes investigation correlation and artifact retrieval")
    print(json.dumps({"scope": scope,
                      "platform": platform.platform(),
                      "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                      "record_max_bytes": 8 * 1024 * 1024 if args.investigate else 256 * 1024,
                      "processing_memory_bytes": 8 * 1024 * 1024 if args.investigate else None,
                      "measurements": measurements}, indent=2))


if __name__ == "__main__":
    main()
