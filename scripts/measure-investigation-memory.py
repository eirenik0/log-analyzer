#!/usr/bin/env python3
"""Measure one investigation's accounting and peak child RSS on synthetic paired events."""
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


def positive(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--records", type=positive, default=8018)
    parser.add_argument("--memory-bytes", type=positive,
                        help="Override accounting only to diagnose an older binary; default is unchanged")
    args = parser.parse_args()
    if args.records % 2:
        parser.error("--records must be even for complete pairs")
    binary = args.binary.resolve()
    profile = Path(__file__).resolve().parents[1] / "examples/investigations/profile.toml"
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("LOG_ANALYZER_")}
    env.update(TZ="UTC", NO_COLOR="1")
    with tempfile.TemporaryDirectory(prefix="investigation-memory-") as directory:
        source = Path(directory) / "synthetic.jsonl"
        artifact = Path(directory) / "evidence.json"
        with source.open("w", encoding="utf-8") as stream:
            for index in range(args.records):
                row = {"ts": f"2026-01-01T00:00:0{index % 2}Z", "level": "INFO",
                       "component": "worker", "component_id": "synthetic",
                       "session": "synthetic", "message": "synthetic boundary",
                       "phase": "start" if index % 2 == 0 else "end",
                       "operation": "run", "id": f"op-{index // 2}", "outcome": "success"}
                stream.write(json.dumps(row, separators=(",", ":")) + "\n")
        command = [str(binary), "--config", str(profile), "investigate", str(source),
                   "--artifact", str(artifact)]
        if args.memory_bytes is not None:
            command += ["--processing-max-memory-bytes", str(args.memory_bytes)]
        start = time.perf_counter()
        result = subprocess.run(command, cwd=directory, env=env, capture_output=True,
                                check=True, timeout=180)
        elapsed = time.perf_counter() - start
        # This process launches exactly one child, so ru_maxrss belongs to this run.
        rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        rss_bytes = rss if sys.platform == "darwin" else rss * 1024
        report = json.loads(result.stdout)
        counts = {item["entity"]: item["count"] for item in report["populations"]}
        if report["processing"]["status"] == "complete":
            if report["processing"]["usage"]["records"] != args.records:
                raise AssertionError("Complete investigation omitted records")
            paired = next(p["count"] for p in report["populations"]
                          if p["id"].endswith("paired-lifecycles"))
            if paired != args.records // 2:
                raise AssertionError("Complete investigation lost paired operations")
        print(json.dumps({
            "platform": platform.platform(), "records": args.records,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "profile_sha256": hashlib.sha256(profile.read_bytes()).hexdigest(),
            "input_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "input_bytes": source.stat().st_size, "elapsed_seconds": elapsed,
            "peak_rss_bytes": rss_bytes, "processing": report["processing"],
            "population_counts": counts, "artifact_status": report["artifact"]["status"],
            "artifact_bytes": artifact.stat().st_size if artifact.exists() else None,
            "limitations": "One local synthetic run; child RSS differs from conservative accounting. "
                           "No production logs or claim of an enforced RSS ceiling.",
        }, indent=2))


if __name__ == "__main__":
    main()
