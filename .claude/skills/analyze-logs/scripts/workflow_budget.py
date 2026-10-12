#!/usr/bin/env python3
"""Observe elapsed workflow time; never infer an exhausted budget from effort."""
import argparse
import json
import math
from pathlib import Path
import time


def start(path, seconds, now):
    if not math.isfinite(seconds) or seconds <= 0:
        raise ValueError('seconds must be positive and finite')
    state = {'version': 1, 'started_monotonic': now, 'seconds': seconds}
    with path.open('x', encoding='utf-8') as output:
        json.dump(state, output)
    return state


def check(path, now):
    with path.open(encoding='utf-8') as source:
        text = source.read(4097)
    if len(text) > 4096:
        raise ValueError('budget state exceeds 4096 characters')
    state = json.loads(text)
    if state.get('version') != 1:
        raise ValueError('unsupported budget state')
    began, seconds = state['started_monotonic'], state['seconds']
    if any(type(n) not in (int, float) or not math.isfinite(n) for n in (began, seconds)) or seconds <= 0 or began > now:
        raise ValueError('invalid state or monotonic clock changed; elapsed time unavailable')
    elapsed = now - began
    return {'version': 1, 'elapsed_seconds': round(elapsed, 3),
            'remaining_seconds': round(max(0, seconds - elapsed), 3),
            'exhausted': elapsed >= seconds,
            'basis': 'observed_since_first_tool_call; external runtime may have started earlier'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['start', 'check'])
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--seconds', type=float)
    args = parser.parse_args()
    try:
        now = time.monotonic()
        if args.command == 'start':
            if args.seconds is None:
                parser.error('start requires --seconds')
            start(args.state, args.seconds, now)
        print(json.dumps(check(args.state, now)))
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f'Budget observation unavailable: {error}\n')


if __name__ == '__main__':
    main()
