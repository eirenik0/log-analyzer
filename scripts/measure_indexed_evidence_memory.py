#!/usr/bin/env python3
"""Measure exact-ID SQLite retrieval RSS; synthetic unqueried storage only.

Run after cargo build --release. This isolates retrieval growth, not parser or
correlator memory. Extra SQLite pages do not change any evidence facts. Uses the
OS time utility for per-process peak RSS, never estimated allocation accounting.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(65536), b""):
            result.update(chunk)
    return result.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/log-analyzer"))
    parser.add_argument("--sizes-mib", type=int, nargs="+", default=[0, 8, 32, 128])
    parser.add_argument("--ceiling-mib", type=int, default=96)
    parser.add_argument("--tolerance-mib", type=int, default=16)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    root = Path(__file__).resolve().parent.parent
    rows = []
    with tempfile.TemporaryDirectory(prefix="indexed-evidence-memory-") as directory:
        directory = Path(directory)
        seed = directory / "seed.db"
        command = [binary, "--json", "--profile", "examples/investigations/profile.toml",
                   "investigate", "examples/investigations/slow.jsonl", "--artifact",
                   str(seed), "--artifact-storage", "sqlite"]
        report = json.loads(subprocess.check_output(command, cwd=root, text=True, encoding="utf-8"))
        finding_id = report["findings"][0]["id"]
        for size in args.sizes_mib:
            path = directory / f"size-{size}.db"
            shutil.copyfile(seed, path)
            with sqlite3.connect(path) as connection:
                connection.execute("CREATE TABLE memory_sweep_padding (payload BLOB NOT NULL)")
                for _ in range(size):
                    connection.execute("INSERT INTO memory_sweep_padding VALUES (zeroblob(1048576))")
            command = [binary, "--json", "evidence", str(path), "--expected-sha256", digest(path),
                       "--id", finding_id, "--artifact-max-bytes", "1073741824",
                       "--evidence-cache-max-bytes", "8388608", "--evidence-record-max-bytes", "8388608"]
            if sys.platform == "darwin":
                time_command = ["/usr/bin/time", "-l"]
                pattern = r"(\d+)\s+maximum resident set size"
                multiplier = 1
            else:
                time_command = ["/usr/bin/time", "-v"]
                pattern = r"Maximum resident set size \(kbytes\):\s+(\d+)"
                multiplier = 1024
            completed = subprocess.run(time_command + command, cwd=root, text=True,
                                       encoding="utf-8", capture_output=True, check=True)
            result = json.loads(completed.stdout)["artifact_retrieval"]
            assert result["total"] == result["displayed"] == 1
            assert result["items"][0]["id"] == finding_id
            match = re.search(pattern, completed.stderr)
            if match is None:
                raise RuntimeError("OS time did not report peak RSS")
            rows.append({"artifact_bytes": path.stat().st_size,
                         "peak_rss_bytes": int(match.group(1)) * multiplier})
    peaks = [row["peak_rss_bytes"] for row in rows]
    passed = max(peaks) <= args.ceiling_mib * 1048576 and max(peaks) - min(peaks) <= args.tolerance_mib * 1048576
    print(json.dumps({"scope": "exact_id_retrieval_with_unqueried_storage_padding",
                      "platform": sys.platform, "cache_bytes": 8388608, "record_bytes": 8388608,
                      "ceiling_bytes": args.ceiling_mib * 1048576,
                      "growth_tolerance_bytes": args.tolerance_mib * 1048576,
                      "measurements": rows, "passed": passed}, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
