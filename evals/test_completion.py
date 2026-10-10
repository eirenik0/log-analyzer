import copy
import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from agents import BASE_PROMPT, external
from attempt import Attempt
from layers import frozen_cases, participant_prompt, run, verified_packet
from investigation import ROOT, digest, score
from scripted import interpret


class CompletionEvaluationTests(unittest.TestCase):
    def answer(self, operations, extra_rows=()):
        task = {'group': 'run', 'subject': 'lookup@east', 'predicate': 'completion'}
        rows = [{'fields': {'operation': 'lookup', 'phase': phase, 'session': 'east'}, 'ref': {'line': line}}
                for phase, line in [('start', 1), ('end', 8)]] + list(extra_rows)
        return interpret({'tasks': [task]}, {'run': (rows, operations)})

    def operation(self, outcome='success', scope='east'):
        return {'name': 'lookup', 'scope': scope, 'outcome': outcome,
                'start': {'line': 1}, 'end': {'line': 8}}

    def test_success_and_failure_both_establish_observed_lifecycle_end(self):
        for outcome in ('success', 'failure'):
            answer = self.answer([self.operation(outcome)])
            self.assertEqual(answer['status'], {'run': 'supported'})
            self.assertIs(answer['findings'][0]['value'], True)
            self.assertEqual(answer['findings'][0]['refs'], [{'line': 1}, {'line': 8}])

    def test_absent_wrong_scope_and_multiple_lifecycles_remain_unknown(self):
        for operations in ([], [self.operation(scope='west')], [self.operation(), self.operation('failure')]):
            answer = self.answer(operations)
            self.assertEqual(answer['status'], {'run': 'insufficient_evidence'})
            self.assertEqual(answer['findings'][0]['kind'], 'unknown')
            self.assertEqual(answer['findings'][0]['value'], 'unknown')

    def test_later_unmatched_start_does_not_inherit_earlier_completion(self):
        later = {'fields': {'operation': 'lookup', 'phase': 'start', 'session': 'east'}, 'ref': {'line': 9}}
        answer = self.answer([self.operation()], [later])
        self.assertEqual(answer['findings'][0]['value'], 'unknown')
        self.assertEqual(answer['status'], {'run': 'insufficient_evidence'})

    def test_completion_scorer_rejects_false_completion_and_blanket_abstention(self):
        manifest, packets = frozen_cases()
        for name in ('completion-late-success', 'completion-failed-terminal', 'incomplete-capture', 'heldout-incomplete'):
            scenario = next(c['scenario'] for c in manifest['cases'] if c['id'] == name)
            inputs = []
            for label in scenario['groups']['run']:
                path = (ROOT / label).resolve()
                sha = hashlib.sha256(path.read_bytes()).hexdigest()
                inputs.append({'label': label, 'file': str(path), 'sha256': sha, 'input_id': digest([str(path), sha])})
            contexts = {'run': {'inputs': inputs}}
            observations = verified_packet(packets[name], contexts)
            answer = interpret(scenario, observations)
            revealed = {'run': {r['ref']['reference_id'] for r in observations['run'][0]}}
            self.assertEqual(score(scenario, answer, contexts, revealed)['status'], 'PASS', name)
            wrong = copy.deepcopy(answer)
            claim = next(f for f in wrong['findings'] if f['predicate'] == 'completion')
            if claim['value'] is True:
                claim.update(kind='unknown', value='unknown')
            else:
                claim.update(kind='observation', value=True)
            self.assertEqual(score(scenario, wrong, contexts, revealed)['status'], 'FAIL', name)
            wrong = copy.deepcopy(answer)
            next(f for f in wrong['findings'] if f['predicate'] == 'completion')['refs'] = []
            self.assertEqual(score(scenario, wrong, contexts, revealed)['status'], 'FAIL', name)

    def test_supplied_entrypoint_changes_exact_prompt_and_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'SKILL.md'
            raw = 'Check the final outcome — cite its source.\r\n'.encode()
            path.write_bytes(raw)
            prompt, identity = participant_prompt(path)
            self.assertTrue(prompt.endswith(raw.decode()))
            self.assertEqual(identity['sha256'], hashlib.sha256(raw).hexdigest())
            self.assertEqual(identity['bytes'], len(raw))
            self.assertEqual(participant_prompt(), (BASE_PROMPT, None))
            path.write_text('Different instructions')
            updated, changed = participant_prompt(path)
            self.assertNotEqual(updated, prompt)
            self.assertNotEqual(changed['sha256'], identity['sha256'])
            for invalid in (b'', b' \n', b'x' * 65537, b'\xff'):
                path.write_bytes(invalid)
                with self.assertRaises((AssertionError, UnicodeDecodeError)):
                    participant_prompt(path)

    def test_scripted_run_cannot_claim_to_evaluate_skill_instructions(self):
        with self.assertRaisesRegex(AssertionError, 'scripted participants do not read'):
            run('unused', skill_file='unused')

    def test_adapter_receives_same_supplied_prompt_on_every_turn(self):
        class Tools:
            arm = 'unified'
            budgets = {'tool_calls': 2, 'output_bytes': 1000}
            output_bytes = 0
            calls = []
            def remaining(self): return 10
            def invoke(self, request):
                self.calls.append(request)
                return {'evidence': 'observed'}
        payloads = []
        def adapter(argv, payload, timeout, on_frame, on_started):
            payloads.append(copy.deepcopy(payload))
            on_started()
            response = {'tool_call': {'group': 'run', 'tool': 'investigate'}} if len(payloads) == 1 else {'final': {'findings': []}}
            response['usage'] = {'tokens': 1, 'provider_cost_usd': 0}
            on_frame(response)
            return response
        prompt = BASE_PROMPT + '\nA specific completion checklist.'
        with patch('agents.adapter_call', side_effect=adapter):
            external(Tools(), {}, ['adapter'], 'fixed-model', {}, Attempt(), 1, base_prompt=prompt)
        self.assertEqual([p['base_prompt'] for p in payloads], [prompt, prompt])
        self.assertEqual(len(payloads[1]['history']), 1)
        self.assertEqual(payloads[1]['remaining']['tool_calls'], 1)
