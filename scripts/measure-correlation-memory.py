#!/usr/bin/env python3
"""Measure selected-record correlation RSS; all starts precede every completion."""
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


def measure(binary, pairs):
    with tempfile.TemporaryDirectory(prefix="correlation-memory-") as directory:
        source = Path(directory) / "selected.jsonl"
        digest = hashlib.sha256()
        with source.open("wb") as output:
            for phase, timestamp in (("start", "2026-01-01T02:00:00+02:00"),
                                     ("end", "2026-01-02T00:00:00Z")):
                for index in range(pairs):
                    row = json.dumps({"ts": timestamp, "phase": phase, "id": str(index),
                                      "operation": "fetch", "session": "run-a", "outcome": "success",
                                      "message": "synthetic selected boundary"}, separators=(",", ":")).encode() + b"\n"
                    output.write(row)
                    digest.update(row)
        env = {key: value for key, value in os.environ.items() if not key.startswith("LOG_ANALYZER_")}
        env.update(TZ="UTC", NO_COLOR="1")
        started = time.monotonic()
        result = subprocess.run([str(binary), str(source)], capture_output=True, check=True, env=env)
        elapsed = time.monotonic() - started
        data = json.loads(result.stdout)
        assert data["selected_records"] == pairs * 2, data
        assert data["emitted_records"] == pairs * 2, data
        assert data["pairs"] == pairs and data["unmatched"] == 0, data
        assert data["elapsed_sum_ms"] == pairs * 86400000, data
        assert data["coverage"]["parsed_entries"] == pairs * 2, data
        assert data["coverage"]["rejected_candidates"] == 0, data
        assert data["coverage"]["snapshot_sha256"] == digest.hexdigest(), data
        rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        return {"selected_records": pairs * 2, "pairs": pairs, "input_bytes": source.stat().st_size,
                "peak_rss_bytes": rss if sys.platform == "darwin" else rss * 1024,
                "elapsed_seconds": round(elapsed, 4), "spilled": data["spilled"], "sha256": digest.hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--pairs", type=int, nargs="+", default=[1000, 10000, 100000])
    parser.add_argument("--worker", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--rss-ceiling-mib", type=int, default=64)
    parser.add_argument("--rss-growth-mib", type=int, default=24)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.worker is not None:
        print(json.dumps(measure(binary, args.worker)))
        return
    if any(count <= 0 for count in args.pairs):
        parser.error("pair counts must be positive")
    measurements = []
    for count in args.pairs:
        output = subprocess.run([sys.executable, str(Path(__file__).resolve()), str(binary), "--worker", str(count)],
                                capture_output=True, check=True)
        measurements.append(json.loads(output.stdout))
    peaks = [value["peak_rss_bytes"] for value in measurements]
    result = {"scope": "yielding parser and spilling correlator; every record selected; all starts precede every completion; excludes investigation evidence construction and retrieval",
              "platform": platform.platform(), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "index_bytes": 1024 * 1024, "sqlite_cache_bytes_per_store": 2 * 1024 * 1024,
              "record_max_bytes": 256 * 1024, "disk_max_bytes": 4 * 1024 * 1024 * 1024,
              "rss_ceiling_bytes": args.rss_ceiling_mib * 1024 * 1024,
              "rss_growth_tolerance_bytes": args.rss_growth_mib * 1024 * 1024,
              "measurements": measurements}
    print(json.dumps(result, indent=2))
    assert max(peaks) <= result["rss_ceiling_bytes"], "absolute RSS ceiling exceeded"
    assert max(peaks) - min(peaks) <= result["rss_growth_tolerance_bytes"], "RSS growth tolerance exceeded"
    assert measurements[-1]["spilled"], "largest workload did not spill"


if __name__ == "__main__":
    main()
