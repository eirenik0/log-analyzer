#!/usr/bin/env python3
"""Measure retrieval RSS in fresh children over artifacts with every input record selected."""
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


def environment():
    env = {key: value for key, value in os.environ.items() if not key.startswith("LOG_ANALYZER_")}
    env.update(TZ="UTC", NO_COLOR="1")
    return env


def file_hash(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def worker(binary, artifact, mode, finding_id, expected_records):
    # This worker launches exactly one child. Artifact generation and Python
    # comparison allocations are excluded from the measured retrieval process.
    digest = file_hash(artifact)
    command = [str(binary), "investigation-evidence", str(artifact), "--expected-sha256", digest,
               "--artifact-max-bytes", str(2 * 1024**3), "--artifact-max-index-bytes", str(2 * 1024**3),
               "--artifact-max-state-bytes", str(2 * 1024**3), "--artifact-max-validation-ms", "600000",
               "--collection", "/records" if mode == "records" else "/findings", "--complete-output"]
    if mode != "records":
        command += ["--id", finding_id]
    with tempfile.TemporaryDirectory(prefix="retrieval-claims-") as claims_directory, tempfile.TemporaryFile() as output:
        if mode == "count":
            claims_path = Path(claims_directory) / "claims.json"
            claims_path.write_text(json.dumps({"version": 1, "required_claim_ids": ["selected"],
                                               "claims": [{"id": "selected", "kind": "distinct_records", "pointer": "/records", "expected": expected_records,
                                                           "operation": {"kind": "request", "name": "fetch"}}]}), encoding="utf-8")
            command += ["--claims", str(claims_path)]
        start = time.monotonic()
        result = subprocess.run(command, stdout=output, stderr=subprocess.PIPE, env=environment(), timeout=660)
        elapsed = time.monotonic() - start
        rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        assert result.returncode == 0, result.stderr.decode("utf-8")
        output.seek(0)
        retrieved = json.load(output)["artifact_retrieval"]
    # Load comparison data only after measuring the child, so fork-based hosts
    # cannot attribute an inherited Python artifact tree to retrieval RSS.
    retained = json.loads(artifact.read_text(encoding="utf-8"))
    measurements = [f for f in retained["findings"] if f["kind"] == "measurement"]
    expected = retained["records"] if mode == "records" else [measurements[-1]]
    if mode == "count":
        assert retrieved["claim_verification"]["status"] == "verified"
        assert retrieved["claim_verification"]["checks"][0]["observed"] == expected_records
    assert retrieved["status"] == "complete" and retrieved["remaining"] == 0, retrieved
    assert retrieved["artifact_sha256"] == digest
    assert retrieved["items"] == expected, "selected records/finding changed during bounded retrieval"
    assert retrieved["displayed"] == len(expected) == retrieved["total"]
    return {"mode": mode, "selected_records": len(retained["records"]), "paired_measurements": len(measurements),
            "artifact_bytes": artifact.stat().st_size, "artifact_sha256": digest,
            "peak_rss_bytes": rss if sys.platform == "darwin" else rss * 1024,
            "elapsed_seconds": round(elapsed, 4), "retrieved_items": len(expected)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--records", type=int, nargs="+", default=[1000, 5000, 20000])
    parser.add_argument("--worker", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--expected-records", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--finding-id", help=argparse.SUPPRESS)
    parser.add_argument("--mode", choices=["records", "finding", "count"], default="records", help=argparse.SUPPRESS)
    parser.add_argument("--rss-ceiling-mib", type=int, default=64)
    parser.add_argument("--rss-growth-mib", type=int, default=24)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.worker:
        print(json.dumps(worker(binary, args.worker, args.mode, args.finding_id, args.expected_records)))
        return
    if any(n <= 0 or n % 2 for n in args.records):
        parser.error("record counts must be positive and even")
    profile = Path(__file__).resolve().parents[1] / "examples/investigations/profile.toml"
    results = []
    for count in args.records:
        with tempfile.TemporaryDirectory(prefix="selected-retrieval-") as directory:
            source = Path(directory) / "selected.jsonl"
            artifact = Path(directory) / "evidence.json"
            with source.open("w", encoding="utf-8") as output:
                for phase, timestamp in (("start", "2026-01-01T02:00:00+02:00"), ("end", "2026-01-02T00:00:00Z")):
                    for index in range(count // 2):
                        output.write(json.dumps({"ts": timestamp, "level": "INFO", "phase": phase, "id": str(index),
                                                 "operation": "fetch", "session": "selected-run", "outcome": "success",
                                                 "message": "selected synthetic boundary é"}, ensure_ascii=False, separators=(",", ":")) + "\n")
            generated = subprocess.run([str(binary), "--json", "--config", str(profile), "investigate", str(source),
                                        "--artifact", str(artifact), "--artifact-max-bytes", str(2 * 1024**3),
                                        "--processing-max-memory-bytes", str(4 * 1024**3),
                                        "--processing-max-records", str(count + 1), "--processing-max-work", "500000000",
                                        "--processing-max-ms", "600000"], capture_output=True, env=environment(), check=True, timeout=660)
            report = json.loads(generated.stdout)
            assert report["processing"]["status"] == "complete", report["processing"]
            assert report["artifact"]["status"] == "complete"
            inputs = report["report_metadata"]["evidence"]["inputs"]
            assert inputs[0]["selected_entries"] == count
            assert inputs[0]["sha256"] == hashlib.sha256(source.read_bytes()).hexdigest()
            retained = json.loads(artifact.read_text(encoding="utf-8"))
            finding_id = [f["id"] for f in retained["findings"] if f["kind"] == "measurement"][-1]
            del retained
            for mode in ("records", "finding", "count"):
                output = subprocess.run([sys.executable, str(Path(__file__).resolve()), str(binary),
                                         "--worker", str(artifact), "--mode", mode, "--finding-id", finding_id, "--expected-records", str(count)], capture_output=True, check=True, timeout=700)
                result = json.loads(output.stdout)
                assert result["selected_records"] == count
                assert result["paired_measurements"] == count // 2
                results.append(result)
                print(f"{count} selected, {mode}: {result['peak_rss_bytes']} bytes RSS, {result['elapsed_seconds']}s", file=sys.stderr)
    peaks = [result["peak_rss_bytes"] for result in results]
    result = {"scope": "artifact retrieval only; every source record selected, all starts precede completions; compares every retrieved record and targeted measurement exactly, and verifies operation-filtered distinct-record claims; generation RSS excluded",
              "platform": platform.platform(), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "item_bytes": 8 * 1024**2, "index_bytes": 2 * 1024**3, "state_bytes": 2 * 1024**3,
              "rss_ceiling_bytes": args.rss_ceiling_mib * 1024**2,
              "rss_growth_tolerance_bytes": args.rss_growth_mib * 1024**2, "measurements": results}
    print(json.dumps(result, indent=2))
    assert max(peaks) <= result["rss_ceiling_bytes"], "absolute RSS ceiling exceeded"
    assert max(peaks) - min(peaks) <= result["rss_growth_tolerance_bytes"], "RSS growth tolerance exceeded"


if __name__ == "__main__":
    main()
