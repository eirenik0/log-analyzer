#!/usr/bin/env python3
"""Run maintained CLI examples against the supplied built/released binary."""
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
binary = Path(sys.argv[1]).resolve()
fixture = str(root / "examples" / "synthetic.jsonl")
environment = {key: value for key, value in os.environ.items() if not key.startswith("LOG_ANALYZER_")}
for example in json.loads((root / "examples" / "commands.json").read_text()):
    args = [arg.replace("{fixture}", fixture) for arg in example["args"]]
    result = subprocess.run([str(binary), *args], capture_output=True, text=True, check=True, env=environment)
    if example.get("type") == "toml":
        assert 'profile_name = "example-profile"' in result.stdout
        assert "# Build: log-analyzer" in result.stdout
    elif example.get("type") == "version":
        assert result.stdout.startswith("log-analyzer ") and len(result.stdout) < 100
    else:
        value = json.loads(result.stdout)
        for part in example["pointer"].strip("/").split("/"):
            value = value[part]
    print("Passed:", " ".join(example["args"]))
