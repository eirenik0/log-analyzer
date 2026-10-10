#!/usr/bin/env python3
"""Compare release binaries on synthetic search/context and long tracing inputs."""
import argparse
import hashlib
import json
import os
import platform
import statistics
import subprocess
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
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--repeats", type=positive, default=3)
    parser.add_argument("--records", type=positive, default=20000)
    parser.add_argument("--context", type=positive, default=2000)
    parser.add_argument("--words", type=positive, default=20000)
    args = parser.parse_args()
    binaries = {"candidate": args.binary.resolve()}
    if args.baseline:
        binaries["baseline"] = args.baseline.resolve()
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("LOG_ANALYZER_")}
    env.update(TZ="UTC", NO_COLOR="1")
    report = {"platform": platform.platform(), "parameters": {
        "repeats": args.repeats, "records": args.records,
        "context": args.context, "words": args.words,
    }, "binaries": {name: hashlib.sha256(path.read_bytes()).hexdigest()
                    for name, path in binaries.items()}, "cases": {}}
    with tempfile.TemporaryDirectory(prefix="search-performance-") as directory:
        root = Path(directory)
        classic = "worker | 2026-01-01T00:00:00Z [INFO ] synthetic message\n"
        prose = "word " * args.words
        tracing = "2026-01-01T00:00:00Z INFO app::worker: "
        cases = {
            "dense_context": (classic * args.records, ["--context", str(args.context)]),
            "dense_no_context": (classic * args.records, []),
            "tracing_prose": ((tracing + prose + "\n") * 20, ["--count-by", "matches"]),
            "tracing_fields": ((tracing + prose + "trace_id=abc123\n") * 20,
                               ["--count-by", "matches"]),
        }
        for name, (content, flags) in cases.items():
            source = root / f"{name}.log"
            source.write_text(content, encoding="utf-8")
            durations = {label: [] for label in binaries}
            build_footers = {}
            expected = None
            # Warm both binaries once; alternate execution order across measured rounds.
            for iteration in range(args.repeats + 1):
                labels = list(binaries)
                if iteration % 2:
                    labels.reverse()
                for label in labels:
                    start = time.perf_counter()
                    result = subprocess.run(
                        [str(binaries[label]), "--color", "never", "search", str(source), *flags],
                        cwd=root, env=env, capture_output=True, check=True, timeout=120,
                    )
                    elapsed = time.perf_counter() - start
                    lines = result.stdout.splitlines(keepends=True)
                    # Build provenance intentionally differs between clean/edited binaries.
                    if lines and lines[-1].startswith(b"Build: log-analyzer "):
                        build_footers[label] = lines.pop().decode().strip()
                    digest = hashlib.sha256(b"".join(lines)).hexdigest()
                    if expected is not None and digest != expected:
                        raise AssertionError(f"{name}: output changed for {label}")
                    expected = digest
                    if iteration:
                        durations[label].append(elapsed)
            medians = {label: statistics.median(values) for label, values in durations.items()}
            case = {"input_bytes": source.stat().st_size, "seconds": durations,
                    "median_seconds": medians, "report_sha256": expected,
                    "build_footers": build_footers,
                    "identical_report_excluding_build_footer": True}
            if args.baseline:
                case["speedup"] = medians["baseline"] / medians["candidate"]
            report["cases"][name] = case
    report["limitations"] = (
        "Local synthetic end-to-end wall times, including startup, parsing and output; "
        "not production throughput or a memory benchmark. Count output equality does "
        "not validate parsed fields; parser regression tests cover those semantics."
    )
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
