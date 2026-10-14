#!/usr/bin/env python3
"""Check numeric source replay using a normal build without test-only features."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile


# @lat: [[regression-tests#Evidence regressions#Numeric artifact replay]]
def check(binary):
    environment = {k: v for k, v in os.environ.items() if not k.startswith('LOG_ANALYZER_')}

    def invoke(*args):
        return subprocess.run(
            [str(binary), *map(str, args)], capture_output=True, text=True,
            encoding='utf-8', env=environment, timeout=30,
        )

    # The default JSON decoder changes the first decimal on artifact round-trip.
    payload = {'timing': 0.24324324324324326, 'nested': [1.2345678901234567, -0.0],
               'integer': 9007199254740993}
    encoded = json.dumps(payload)
    inputs = {
        'classic': f'resource (session-a) | 2026-01-01T00:00:00Z [INFO ] resource response with body {encoded}\n',
        'json': json.dumps({'timestamp': '2026-01-01T00:00:00Z', 'level': 'INFO',
                            'component': 'resource', 'message': 'resource response',
                            'payload': payload}) + '\n',
    }
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for name, source in inputs.items():
            input_path = root / f'{name}.log'
            artifact_path = root / f'{name}.json'
            input_path.write_text(source, encoding='utf-8')
            result = invoke('investigate', input_path, '--profile', 'eyes', '--json',
                            '--complete-output', '--artifact', artifact_path)
            assert result.returncode == 0, result.stderr
            report = json.loads(result.stdout)
            artifact = json.loads(artifact_path.read_text(encoding='utf-8'))
            fields = artifact['records'][0]['fields']
            assert fields['payload'] == payload, fields
            if name == 'json':
                assert fields['envelope_payload'] == payload, fields

            def retrieve(path, digest):
                return invoke('evidence', path, '--expected-sha256', digest,
                              '--collection', '/records', '--complete-output')

            valid = retrieve(artifact_path, report['artifact']['stored_sha256'])
            assert valid.returncode == 0, valid.stderr
            # Refresh the digest so rejection must come from source replay, not hashing.
            for field in ('payload', 'envelope_payload') if name == 'json' else ('payload',):
                altered = json.loads(artifact_path.read_text(encoding='utf-8'))
                altered['records'][0]['fields'][field]['timing'] = math.nextafter(payload['timing'], math.inf)
                data = json.dumps(altered).encode('utf-8')
                altered_path = root / f'{name}-{field}-altered.json'
                altered_path.write_bytes(data)
                rejected = retrieve(altered_path, hashlib.sha256(data).hexdigest())
                assert rejected.returncode != 0, 'altered numeric evidence was accepted'
                assert 'retained fields differs from parsed record' in rejected.stderr, rejected.stderr
    print('Numeric replay: valid classic/JSON payloads pass; adjacent-float alterations fail.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    check(parser.parse_args().binary.resolve())
