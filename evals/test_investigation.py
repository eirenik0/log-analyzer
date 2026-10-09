import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from investigation import address, digest, reference, score, Tools
from agents import adapter_call


class GroundedScoringTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.sources = []
        for label in ('a', 'b'):
            path = Path(self.directory.name) / (label + '.jsonl')
            path.write_text('{"rows":[{"scope":"a"},{"scope":"b"}]}\n{}\n{}\n')
            sha = hashlib.sha256(path.read_bytes()).hexdigest()
            self.sources.append({'file': str(path), 'sha256': sha, 'label': label, 'input_id': digest([str(path), sha])})
        self.contexts = {'run': {'inputs': [self.sources[0]], 'snapshot_id': 'snapshot-a', 'profile_sha256': 'profile'}, 'other': {'inputs': [self.sources[1]], 'snapshot_id': 'snapshot-b', 'profile_sha256': 'profile'}}
        start, end = [reference(self.sources[0], n) for n in (1, 3)]
        self.scenario = {'status': {'run': 'insufficient_evidence'}, 'expected': [
            {'group': 'run', 'subject': 'lookup@a', 'predicate': 'elapsed_ms', 'kind': 'measurement', 'value': 2000, 'support': [['a', 1, None], ['a', 3, None]], 'boundaries': [['a', 1, None], ['a', 3, None]]},
            {'group': 'run', 'subject': 'lookup@a', 'predicate': 'cause', 'kind': 'unknown', 'value': 'unknown', 'support': [['a', 1, None], ['a', 3, None]]}]}
        self.response = {'status': {'run': 'insufficient_evidence'}, 'findings': [
            {'group': 'run', 'subject': 'lookup@a', 'predicate': 'elapsed_ms', 'kind': 'measurement', 'value': 2000, 'refs': [start, end], 'boundaries': {'start': start, 'end': end}},
            {'group': 'run', 'subject': 'lookup@a', 'predicate': 'cause', 'kind': 'unknown', 'value': 'unknown', 'refs': [start, end]}]}
        self.revealed = {'run': {start['reference_id'], end['reference_id']}, 'other': set()}

    def evaluate(self, response=None, revealed=None):
        return score(self.scenario, response or self.response, self.contexts, revealed or self.revealed)

    def test_expected_abstention_and_grounded_measurement_pass(self):
        self.assertEqual(self.evaluate()['status'], 'PASS')

    def test_correct_value_with_invalid_unseen_or_wrong_record_citations_fails(self):
        for ref in [dict(self.response['findings'][0]['refs'][0], reference_id='f' * 64), reference(self.sources[0], 2), reference(self.sources[1], 1)]:
            response = copy.deepcopy(self.response)
            response['findings'][0]['refs'][0] = ref
            self.assertEqual(self.evaluate(response)['status'], 'FAIL')
        self.assertEqual(self.evaluate(revealed={'run': set(), 'other': set()})['status'], 'FAIL')

    def test_wrong_claim_scope_snapshot_boolean_or_boundary_order_fails(self):
        for field, value in [('value', 3000), ('value', True), ('subject', 'lookup@b'), ('group', 'other')]:
            response = copy.deepcopy(self.response)
            response['findings'][0][field] = value
            self.assertEqual(self.evaluate(response)['status'], 'FAIL')
        response = copy.deepcopy(self.response)
        boundaries = response['findings'][0]['boundaries']
        boundaries['start'], boundaries['end'] = boundaries['end'], boundaries['start']
        self.assertEqual(self.evaluate(response)['status'], 'FAIL')

    def test_invented_cause_is_penalized_and_important_unknown_omission_fails(self):
        response = copy.deepcopy(self.response)
        response['findings'][1].update(value='network', kind='observation')
        result = self.evaluate(response)
        self.assertEqual(result['status'], 'FAIL')
        self.assertEqual(result['unsupported_causal_claims'], 1)
        response = copy.deepcopy(self.response)
        response['findings'].pop()
        self.assertTrue(self.evaluate(response)['omissions'])

    def test_extra_prose_predicates_duplicates_and_false_confidence_fail(self):
        for mutation in ('prose', 'predicate', 'duplicate', 'duplicate-cite', 'supported'):
            response = copy.deepcopy(self.response)
            if mutation == 'prose': response['findings'][0]['claim'] = 'Database blocking caused this.'
            elif mutation == 'predicate': response['findings'][0]['predicate'] = 'database_blocked'
            elif mutation == 'duplicate': response['findings'].append(response['findings'][0])
            elif mutation == 'duplicate-cite': response['findings'][0]['refs'].append(response['findings'][0]['refs'][0])
            else: response['status']['run'] = 'supported'
            self.assertEqual(self.evaluate(response)['status'], 'FAIL')

    def test_normalized_row_resolves_and_cannot_be_forged(self):
        address(reference(self.sources[0], 1, '/rows/0'), self.contexts['run'])
        for row in ('/rows/9', '/absent'):
            with self.assertRaises((AssertionError, IndexError, KeyError)):
                address(reference(self.sources[0], 1, row), self.contexts['run'])
        truth = self.scenario['expected'][0]
        truth.update(support=[['a', 1, '/rows/0'], ['a', 3, None]], boundaries=[['a', 1, '/rows/0'], ['a', 3, None]])
        wrong_row = reference(self.sources[0], 1, '/rows/1')
        response = copy.deepcopy(self.response)
        response['findings'][0]['refs'][0] = wrong_row
        response['findings'][0]['boundaries']['start'] = wrong_row
        self.revealed['run'].add(wrong_row['reference_id'])
        self.assertEqual(self.evaluate(response)['status'], 'FAIL')

    def test_location_loss_is_not_an_excuse_for_omissions_or_guesses(self):
        response = copy.deepcopy(self.response)
        response['findings'][0]['refs'][0]['location_redacted'] = True
        self.assertEqual(self.evaluate(response)['status'], 'FAIL')
        scenario = {'status': {'run': 'insufficient_evidence'}, 'expected': [{'group': 'run', 'subject': 'capture', 'predicate': 'location_resolvable', 'kind': 'unknown', 'value': False, 'support': []}]}
        response = {'status': scenario['status'], 'findings': [{k: v for k, v in scenario['expected'][0].items() if k != 'support'} | {'refs': []}]}
        self.assertEqual(score(scenario, response, self.contexts, self.revealed)['status'], 'FAIL')
        self.revealed['run'].add('__location_loss__')
        self.assertEqual(score(scenario, response, self.contexts, self.revealed)['status'], 'PASS')

    def test_harness_fingerprint_includes_imported_runner_and_nested_secrets_are_rejected(self):
        import agents
        from unittest.mock import patch
        paths = ('evals/agents.py', 'evals/investigation.py', 'evals/scripted.py', 'evals/schema.py', 'scripts/check-examples.py')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in paths:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)
            with patch.object(agents, 'ROOT', root):
                before = agents.harness_fingerprint()
                (root / 'scripts/check-examples.py').write_text('changed dependency')
                self.assertNotEqual(before, agents.harness_fingerprint())
        agents.nonsecret_configuration({'temperature': 0, 'max_tokens': 100})
        with self.assertRaises(AssertionError): agents.nonsecret_configuration({'provider': {'apiKey': 'do-not-record'}})

    def test_missing_reference_fields_and_expansion_null_fail_schema_and_score(self):
        for mutation in ('missing-row', 'null-expansion'):
            response = copy.deepcopy(self.response)
            for fact in response['findings']:
                for ref in fact['refs']:
                    if mutation == 'missing-row': ref.pop('row_path', None)
                    else: ref['expansion'] = None
            self.assertEqual(self.evaluate(response)['status'], 'FAIL')
        from schema import validate, InvalidValue
        schema = json.loads((Path(__file__).resolve().parents[1] / 'schemas/investigation.schema.json').read_text())
        from agents import contracts
        contexts = copy.deepcopy(self.contexts)
        for context in contexts.values():
            context.update(snapshot_id='a' * 64, profile_sha256='b' * 64)
        valid = contracts(self.response, contexts)[0]
        validate(valid, schema)
        invalid = copy.deepcopy(valid)
        invalid['findings'][0]['supporting_refs'][0].pop('row_path')
        with self.assertRaises(InvalidValue): validate(invalid, schema)
        with self.assertRaises(ValueError): validate(valid, {'unknownKeyword': True})

    def test_migration_proofs_match_each_case_and_preserve_original_hash(self):
        root = Path(__file__).resolve().parent
        migrations = json.loads((root / 'migrations.json').read_text())
        self.assertEqual(migrations['original_sha256'], hashlib.sha256((root / 'original-cases.json').read_bytes()).hexdigest())
        cases = {case['id']: case for case in json.loads((root / 'cases.json').read_text())['cases']}
        for migration in migrations['migrations']:
            wanted = {check['id']: check['value'] for check in cases[migration['case']]['expect']}
            self.assertEqual({check['id']: check['expected'] for check in migration['checks']}, wanted)
            self.assertTrue(all(check['passed'] for check in migration['checks']))

    def test_nonreading_adapter_large_stdin_obeys_wall_deadline(self):
        import sys, time, subprocess
        started = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired):
            adapter_call([sys.executable, '-c', 'import time; time.sleep(1)'], {'payload': 'x' * 1_000_000}, 0.05)
        self.assertLess(time.monotonic() - started, 0.5)

    def test_descendant_held_pipes_never_extend_adapter_deadline(self):
        import sys, time, subprocess
        for reply in ('print("{}", flush=True)', 'pass'):
            program = 'import subprocess,sys; subprocess.Popen([sys.executable,"-c","import time; time.sleep(1)"],stdin=subprocess.DEVNULL); ' + reply
            started = time.monotonic()
            try:
                result = adapter_call([sys.executable, '-c', program], {}, 0.15)
                self.assertEqual(result, {})
            except (AssertionError, subprocess.TimeoutExpired):
                pass
            self.assertLess(time.monotonic() - started, 0.6)

    def test_unsupported_input_requires_observed_coverage_witness(self):
        scenario = {'status': {'run': 'unsupported_input'}, 'expected': [{'group': 'run', 'subject': 'capture', 'predicate': 'parsing_supported', 'kind': 'unknown', 'value': False, 'support': []}]}
        response = {'status': scenario['status'], 'findings': [{k: v for k, v in scenario['expected'][0].items() if k != 'support'} | {'refs': []}]}
        self.assertEqual(score(scenario, response, self.contexts, self.revealed)['status'], 'FAIL')
        self.revealed['run'].add('__unsupported__')
        self.assertEqual(score(scenario, response, self.contexts, self.revealed)['status'], 'PASS')

    def test_changed_bytes_fail_even_with_previously_valid_reference(self):
        Path(self.sources[0]['file']).write_text('{}\n')
        self.assertEqual(self.evaluate()['status'], 'FAIL')

    def test_read_only_tool_allowlist_and_budgets_reject_mutations(self):
        tools = Tools(Path(__file__), {'profile': 'unused'}, 'analyzer', self.contexts, {'tool_calls': 1, 'output_bytes': 100, 'wall_seconds': 1})
        for request in [{'group': 'run', 'tool': 'analyzer', 'command': 'generate-profile'}, {'group': 'run', 'tool': 'analyzer', 'command': 'info', 'options': ['--output', '/tmp/forbidden']}, {'group': 'undeclared', 'tool': 'read'}]:
            with self.assertRaises(AssertionError): tools.invoke(request)

    def test_external_adapter_protocol_size_timeout_and_no_shell(self):
        import sys
        self.assertEqual(adapter_call([sys.executable, '-c', 'import json,sys; json.load(sys.stdin); print("{}")'], {'public': True}, 2), {})
        with self.assertRaises(AssertionError):
            adapter_call([sys.executable, '-c', 'print("x"*2000)'], {}, 2, limit=100)
        import subprocess
        with self.assertRaises(subprocess.TimeoutExpired):
            adapter_call([sys.executable, '-c', 'import time; time.sleep(3)'], {}, 0.05)


if __name__ == '__main__': unittest.main()
