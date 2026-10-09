#!/usr/bin/env python3
"""Validate synchronized stable release metadata before publishing a prepared version."""
import argparse
from datetime import date
import json
from pathlib import Path
import re


def version_field(table):
    values = re.findall(r'^version\s*=\s*"([^"]+)"\s*$', table, re.M)
    if len(values) != 1:
        raise ValueError('expected one version field')
    return values[0]


def validate(root, expected):
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', expected):
        raise ValueError('prepared version must be stable MAJOR.MINOR.PATCH')
    cargo = (root / 'Cargo.toml').read_text(encoding='utf-8')
    package = re.findall(r'^\[package\]\s*\n(.*?)(?=^\[|\Z)', cargo, re.M | re.S)
    if len(package) != 1 or version_field(package[0]) != expected:
        raise ValueError('Cargo.toml version does not match prepared version')
    lock = (root / 'Cargo.lock').read_text(encoding='utf-8')
    packages = re.split(r'^\[\[package\]\]\s*$', lock, flags=re.M)[1:]
    selected = [p for p in packages if re.search(r'^name\s*=\s*"log-analyzer"\s*$', p, re.M)]
    if len(selected) != 1 or version_field(selected[0]) != expected:
        raise ValueError('Cargo.lock version does not match prepared version')
    plugin = json.loads((root / '.claude-plugin/plugin.json').read_text(encoding='utf-8'))
    marketplace = json.loads((root / '.claude-plugin/marketplace.json').read_text(encoding='utf-8'))
    entries = [p for p in marketplace['plugins'] if p.get('name') == 'log-analyzer']
    if plugin.get('name') != 'log-analyzer' or plugin.get('version') != expected or len(entries) != 1 or entries[0].get('version') != expected:
        raise ValueError('plugin versions do not match prepared version')
    changelog = (root / 'CHANGELOG.md').read_text(encoding='utf-8')
    headings = list(re.finditer(r'^## (\d+\.\d+\.\d+) \((\d{4}-\d{2}-\d{2})\)\s*$', changelog, re.M))
    if not headings or headings[0].group(1) != expected or sum(h.group(1) == expected for h in headings) != 1:
        raise ValueError('prepared version must be the latest unique changelog release')
    date.fromisoformat(headings[0].group(2))
    notes = changelog[headings[0].end():].split('\n## ', 1)[0]
    if not notes.strip():
        raise ValueError('prepared release must have changelog notes')
    pending = list((root / '.changeset').glob('*.md'))
    if pending:
        raise ValueError('prepared release must consume pending changesets')
    return expected


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version')
    args = parser.parse_args()
    try:
        print(validate(Path(__file__).resolve().parents[1], args.version))
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, str(error) + '\n')
