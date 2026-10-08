#!/usr/bin/env python3
"""Measure stateless retrieval on generated public data; no logs/model credentials."""
import argparse
import datetime
import hashlib
import json
import platform
import resource
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--records", type=int, default=10000)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.records < 1:
        parser.error("--records must be positive")
    binary = args.binary.resolve()
    with tempfile.TemporaryDirectory(prefix="report-retrieval-") as directory:
        file = Path(directory) / "synthetic.jsonl"
        with file.open("w", encoding="utf-8") as stream:
            for index in range(args.records):
                row = {"timestamp": "2026-01-01T00:00:00+02:00", "level": "INFO",
                       "component": f"worker{index % 8}", "component_id": f"scope/{index}",
                       "message": f"synthetic operation {index} " + "界🙂" * 80,
                       "payload": {"ordinal": index, "items": list(range(16))}}
                stream.write(json.dumps(row, ensure_ascii=False) + "\n")
        corpus = file.read_bytes()
        base = [str(binary), "process", str(file), "--no-sanitize"]

        def invoke(flags):
            start = time.perf_counter()
            result = subprocess.run(base + flags, capture_output=True, check=True)
            elapsed = time.perf_counter() - start
            return json.loads(result.stdout), len(result.stdout), elapsed

        complete, complete_bytes, complete_latency = invoke(["--complete-output"])
        expected = [r["evidence_ref"]["reference_id"] for r in complete["evidence_records"]]
        received = []
        cursor = None
        pages = 0
        output_bytes = 0
        traversal = 0.0
        first_latency = None
        while True:
            flags = ["--report-max-items", "1000", "--report-max-bytes", str(2 * 1024 * 1024)]
            if cursor:
                flags += ["--report-cursor", cursor]
            report, size, elapsed = invoke(flags)
            if report["retrieval"]["status"] not in ("page", "complete"):
                raise RuntimeError(f"Traversal stopped: {report['retrieval']['status']}")
            if size > 2 * 1024 * 1024:
                raise AssertionError("Serialized page exceeded byte budget")
            pages += 1
            output_bytes += size
            traversal += elapsed
            first_latency = elapsed if first_latency is None else first_latency
            received.extend(r["evidence_ref"]["reference_id"] for r in report["evidence_records"])
            cursor = report["retrieval"]["next_cursor"]
            if not cursor:
                break
        assert received == expected and len(set(received)) == args.records
        peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        peak_bytes = peak if sys.platform == "darwin" else peak * 1024
        result = {"measured_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "corpus": "generated_generic_jsonl_tied_timestamps", "records": args.records,
                  "input_bytes": len(corpus), "input_sha256": hashlib.sha256(corpus).hexdigest(),
                  "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                  "build": complete["report_metadata"]["build"], "platform": platform.platform(),
                  "page_items": 1000, "page_bytes": 2 * 1024 * 1024, "pages": pages,
                  "complete_output_bytes": complete_bytes, "all_page_output_bytes": output_bytes,
                  "first_page_seconds": round(first_latency, 4),
                  "complete_output_seconds": round(complete_latency, 4),
                  "full_traversal_seconds": round(traversal, 4), "peak_binary_rss_bytes": peak_bytes,
                  "exact_once_source_reconstruction": True,
                  "limitations": "One local run; stateless reparsing; synthetic process workload; output budgets do not bound CPU/memory; no agent/model cost measurement."}
        text = json.dumps(result, indent=2) + "\n"
        print(text, end="")
        if args.output:
            args.output.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
