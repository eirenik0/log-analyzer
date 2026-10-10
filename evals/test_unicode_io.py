"""Keep UTF-8 evidence independent of the host's default text encoding."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import investigation
import layers


class UnicodeIOTests(unittest.TestCase):
    def test_profiles_sources_and_fact_packets_survive_an_ansi_locale(self):
        text = 'Žluťoučký — “資料” 🧪'
        original_open = io.open
        def ansi_open(file, mode='r', buffering=-1, encoding=None, errors=None, newline=None, **kwargs):
            if 'b' not in mode and encoding in (None, 'locale'):
                encoding = 'cp1252'
            return original_open(file, mode, buffering, encoding, errors, newline, **kwargs)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            parent = root / '資料.toml'
            parent.write_bytes(f'profile_name = "{text}"\n'.encode('utf-8'))
            child = root / 'child.toml'
            child.write_bytes('extends = "資料.toml"\n'.encode('utf-8'))
            capture = root / 'žluťoučký.jsonl'
            fields = {'message': text, 'operation': 'lookup', 'phase': 'end'}
            raw = json.dumps(fields, ensure_ascii=False)
            capture.write_bytes((raw + '\n').encode('utf-8'))
            sha = hashlib.sha256(capture.read_bytes()).hexdigest()
            source = {'file': str(capture), 'label': capture.name, 'sha256': sha,
                      'input_id': investigation.digest([str(capture), sha])}
            (root / 'evals').mkdir()
            manifest = {'version': 1, 'files': {}, 'description': text}
            facts = {'version': 1, 'cases': {'unicode': text}}
            for name, document in [('layer-cases.json', manifest), ('verified-facts.json', facts)]:
                (root / 'evals' / name).write_bytes(json.dumps(document, ensure_ascii=False).encode('utf-8'))

            with patch('io.open', side_effect=ansi_open), patch.object(investigation, 'ROOT', root), patch.object(layers, 'ROOT', root):
                # This locale really cannot read the fixture without UTF-8.
                with self.assertRaises(UnicodeDecodeError):
                    capture.read_text(encoding='cp1252')
                profiles = investigation.profile_sources('child.toml')
                self.assertEqual([Path(p['file']).name for p in profiles], ['child.toml', '資料.toml'])
                contexts = {'run': {'inputs': [source], 'profile_sources': profiles}}
                tools = investigation.Tools('unused', {}, 'scripts', contexts,
                                            {'tool_calls': 3, 'output_bytes': 10000, 'wall_seconds': 30})
                records = tools.records(source)
                self.assertEqual(records[0]['fields'], fields)
                self.assertEqual(records[0]['raw'], raw)
                self.assertEqual(investigation.address(records[0]['ref'], contexts['run']), (capture.name, 1, None))
                response = tools.invoke({'tool': 'profile', 'group': 'run'})
                self.assertEqual(response['sources'][1]['text'], parent.read_bytes().decode('utf-8'))
                self.assertEqual(layers.frozen_cases(), (manifest, facts['cases']))
