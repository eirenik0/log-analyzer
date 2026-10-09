#!/usr/bin/env python3
"""Dependency-free CLI evaluations. No shell commands or model judge required."""

import argparse
from collections import Counter
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parent


def pointer(value, path):
    if not path:
        return value
    if not path.startswith("/"):
        raise ValueError("JSON pointers must start with /")
    for part in path[1:].split("/"):
        part = part.replace("~1", "/").replace("~0", "~")
        value = value[int(part)] if isinstance(value, list) else value[part]
    return value


def check_result(check, output):
    try:
        if check["target"] == "json":
            value = pointer(json.loads(output["stdout"]), check.get("path", ""))
            if 'select' in check:
                matches = [row for row in value if all(type(pointer(row, key)) is type(expected) and pointer(row, key) == expected for key, expected in check['select'].items())]
                if len(matches) != 1:
                    raise ValueError('scoped expectation requires exactly one match')
                value = pointer(matches[0], check.get('item_path', ''))
        else:
            value = output[check["target"]]
        expected = check["value"]
        op = check["op"]
        if op == "eq":
            passed = type(value) is type(expected) and value == expected
        elif op == "length":
            passed = len(value) == expected
        elif op == "contains":
            passed = expected in value
        elif op == "not_contains":
            passed = expected not in value
        else:
            raise ValueError(f"unknown check operator: {op}")
        return {"id": check["id"], "passed": passed,
                "actual": value, "expected": expected}
    except (KeyError, IndexError, TypeError, ValueError) as error:
        return {"id": check["id"], "passed": False, "error": str(error)}


def classify(case, output):
    checks = [check_result(check, output) for check in case["expect"]]
    failures = {check["id"] for check in checks if not check["passed"]}
    known = case.get("known_failure")
    if not failures:
        return ("XPASS" if known else "PASS"), checks
    if known and failures == set(known["failed_checks"]):
        signature = [check_result(check, output) for check in known["signature"]]
        if all(check["passed"] for check in signature):
            return "XFAIL", checks
    return "FAIL", checks


def validate_checks(checks):
    if not isinstance(checks, list) or not checks:
        raise ValueError("checks must be a nonempty list")
    ids = set()
    for check in checks:
        if not isinstance(check.get("id"), str) or check["id"] in ids:
            raise ValueError("check IDs must be unique strings")
        ids.add(check["id"])
        if check.get("target") not in {"exit", "stdout", "stderr", "json"}:
            raise ValueError("unknown check target")
        if check.get("op") not in {"eq", "length", "contains", "not_contains"}:
            raise ValueError("unknown check operator")
        if 'select' in check and (check['target'] != 'json' or not isinstance(check['select'], dict) or not check['select'] or not all(isinstance(k, str) and k.startswith('/') for k in check['select'])):
            raise ValueError('invalid scoped selection')
        if 'item_path' in check and not (isinstance(check['item_path'], str) and check['item_path'].startswith('/')):
            raise ValueError('invalid selected item pointer')
        if "value" not in check:
            raise ValueError("missing expected value")
        if "path" in check and not (isinstance(check["path"], str)
                                    and check["path"].startswith("/")):
            raise ValueError("invalid JSON pointer")
    return ids


def load_cases(path):
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("version") != 1 or not isinstance(manifest.get("cases"), list):
        raise ValueError("expected a version 1 cases manifest")
    ids = set()
    for case in manifest["cases"]:
        if not isinstance(case.get("id"), str) or case["id"] in ids:
            raise ValueError("case IDs must be unique strings")
        ids.add(case["id"])
        if not case.get("args") or not all(isinstance(a, str) for a in case["args"]):
            raise ValueError("case arguments must be nonempty string arrays")
        if not case.get("description") or not case.get("tags"):
            raise ValueError("each case needs a description and tags")
        check_ids = validate_checks(case["expect"])
        if not any(c["target"] == "exit" and c["op"] == "eq" for c in case["expect"]):
            raise ValueError("each case needs an explicit expected exit code")
        if "known_failure" in case:
            known = case["known_failure"]
            if not known.get("issue", "").startswith("https://github.com/"):
                raise ValueError("known failures need a GitHub issue URL")
            if not known.get("failed_checks") or not set(known["failed_checks"]) <= check_ids:
                raise ValueError("known failure check IDs must reference expectations")
            validate_checks(known["signature"])
    return manifest["cases"]


def run_case(case, binary, timeout):
    args = [arg.replace("{fixtures}", str(ROOT / "fixtures"))
            .replace("{profiles}", str(ROOT / "profiles")) for arg in case["args"]]
    env = {k: v for k, v in os.environ.items() if not k.startswith("LOG_ANALYZER_")}
    env.update({"TZ": case.get("timezone", "UTC"), "NO_COLOR": "1"})
    started = time.monotonic()
    result = {"id": case["id"], "tags": case["tags"], "command": [str(binary), *args],
              "timezone": env["TZ"], "issue": case.get("known_failure", {}).get("issue")}
    try:
        with tempfile.TemporaryDirectory(prefix="log-analyzer-eval-") as cwd:
            proc = subprocess.run([str(binary), *args], cwd=cwd, env=env,
                                  capture_output=True, encoding="utf-8", errors="replace",
                                  timeout=timeout, check=False)
        output = {"exit": proc.returncode, "stdout": proc.stdout,
                  "stderr": proc.stderr, "json": None}
        result["status"], result["checks"] = classify(case, output)
        result.update({"exit": proc.returncode, "stdout": proc.stdout[:8000],
                       "stderr": proc.stderr[:8000],
                       "capture_truncated": len(proc.stdout) > 8000 or len(proc.stderr) > 8000})
    except (OSError, subprocess.TimeoutExpired) as error:
        result.update({"status": "ERROR", "error": str(error), "checks": []})
    result["duration_ms"] = round((time.monotonic() - started) * 1000)
    return result


def fingerprint(paths):
    digest = hashlib.sha256()
    for path in sorted(paths):
        digest.update(str(path.relative_to(ROOT)).encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def failed_run(results, strict=False):
    return any(r["status"] in {"FAIL", "ERROR", "XPASS"}
               or (strict and r["status"] == "XFAIL") for r in results)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="built log-analyzer executable")
    parser.add_argument("--case", action="append", default=[], help="case ID glob; repeatable")
    parser.add_argument("--report", type=Path, default=ROOT.parent / "target/evals/report.json")
    parser.add_argument("--strict", action="store_true", help="also fail for known product defects")
    parser.add_argument("--timeout", type=float, default=20, help="seconds per case")
    parser.add_argument("--list", action="store_true", help="list selected cases without running")
    args = parser.parse_args(argv)
    try:
        if args.timeout <= 0:
            raise ValueError("timeout must be positive")
        cases = load_cases(ROOT / "cases.json")
        cases = [c for c in cases if not args.case or any(fnmatch.fnmatchcase(c["id"], pattern)
                                                       for pattern in args.case)]
        if not cases:
            raise ValueError("no cases selected")
        if args.list:
            for case in cases:
                print(f'{case["id"]}: {case["description"]}')
            return 0
        binary = args.binary.resolve(strict=True)
        version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                                 timeout=args.timeout, check=True).stdout.strip()
        revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                  capture_output=True, text=True, check=False).stdout.strip()
        inputs = [ROOT / "cases.json", ROOT / "run.py"]
        inputs += [p for folder in ("fixtures", "profiles") for p in (ROOT / folder).rglob("*")
                   if p.is_file()]
        results = []
        for case in cases:
            result = run_case(case, binary, args.timeout)
            results.append(result)
            print(f'{result["status"]:5} {case["id"]}')
            if result["status"] in {"FAIL", "ERROR"}:
                print(json.dumps(result.get("checks") or result.get("error"), ensure_ascii=False))
        counts = {status: sum(r["status"] == status for r in results) for status in ("PASS", "FAIL", "ERROR", "XFAIL", "XPASS")}
        report = {"schema_version": 1, "binary": str(binary), "binary_version": version,
                  "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                  "corpus_sha256": fingerprint(inputs), "checkout_revision": revision,
                  "platform": sys.platform, "strict": args.strict,
                  "counts": counts, "cases": results}
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"{counts} — report: {args.report}")
        return int(failed_run(results, args.strict))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"Evaluation setup error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
