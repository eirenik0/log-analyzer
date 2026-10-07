#!/usr/bin/env python3
"""Validate Conventional Commit headers locally and on pull requests."""

import argparse
import json
import re
import subprocess
from pathlib import Path


HEADER = re.compile(
    r"(?:feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)"
    r"(?:\([^()\s]+\))?!?: \S[^\r\n]*"
)


def valid_header(message):
    lines = message.splitlines()
    return bool(lines and HEADER.fullmatch(lines[0]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--message-file", type=Path)
    source.add_argument("--event-file", type=Path)
    args = parser.parse_args()
    messages = []
    if args.message_file:
        messages.append(("Commit", args.message_file.read_text(encoding="utf-8")))
    else:
        pr = json.loads(args.event_file.read_text(encoding="utf-8"))["pull_request"]
        messages.append(("PR title", pr["title"]))
        base, head = pr["base"]["sha"], pr["head"]["sha"]
        for sha in (base, head):
            if not re.fullmatch(r"[0-9a-f]{40}", sha):
                parser.error("Expected a full Git commit SHA")
        commits = subprocess.check_output(
            ["git", "log", "--no-merges", "--format=%H", f"{base}..{head}"],
            text=True,
        ).splitlines()
        for sha in commits:
            subject = subprocess.check_output(
                ["git", "show", "-s", "--format=%s", sha], text=True
            )
            messages.append((sha[:12], subject))
    failed = [label for label, message in messages if not valid_header(message)]
    if failed:
        print("Invalid Conventional Commit header: " + ", ".join(failed))
        print("Use type(scope): description, e.g. fix(parser): preserve offsets")
        return 1
    print(f"Validated {len(messages)} Conventional Commit header(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
