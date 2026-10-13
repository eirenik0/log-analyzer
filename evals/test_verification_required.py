import copy
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from native_workflow import DOMAIN_SEMANTICS, fixture, prepare_inputs, metrics
from verification_required import Budget, REQUIRED_SCHEMA, PROSE_SCHEMA, trial_required
from schema import validate, InvalidValue


class RequiredRunnerControls(unittest.TestCase):
    def setUp(self):
        probe = patch('verification_required.subprocess.check_output', return_value='--required-relationship --draft-claims')
        probe.start()
        self.addCleanup(probe.stop)

    def test_unsupported_baseline_is_ineligible_without_model_calls(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); self.setup(root)
            with patch('verification_required.subprocess.check_output', return_value='older help'), \
                 patch('verification_required.run_phase') as phase:
                result = trial_required(root, 'candidate', 'development', 'haiku', 0, self.limits())
            phase.assert_not_called()
            self.assertEqual(result['execution'], 'ineligible')
            self.assertIn('--required-relationship', result['eligibility']['reason'])
            self.assertEqual(result['api_equivalent_cost_usd'], 0)
            self.assertEqual(result['tool_calls'], 0)

    def setup(self, root):
        prepare_inputs(root, ['development'])
        skill = root / 'candidate/.agents/skills/analyze-logs/SKILL.md'
        skill.parent.mkdir(parents=True)
        skill.write_text('Synthetic offline skill', encoding='utf-8')
        (root / 'candidate/log-analyzer').write_bytes(b'offline binary stub')
        _, expected = fixture()
        answer = copy.deepcopy(expected)
        answer.update(explanation='Observed facts; cause unknown.',
                      summary={'completed_subjects': [expected['capture']['subject'], 'close'],
                               'unresolved_subjects': [expected['other']['subject']], 'send_count': 1, 'response_count': 1})
        return answer

    def limits(self):
        return {'wall_seconds': 60, 'api_equivalent_usd': 1, 'tool_calls': 30, 'output_bytes': 2_000_000}

    def phase(self, answers, costs=None):
        queue = list(answers); costs = list(costs or [.1] * len(queue))
        def execute(directory, model, prompt, schema, budget, name, tools=True):
            value = queue.pop(0); budget.cost += costs.pop(0)
            return {'phase': name, 'answer': value, 'result': {'usage': {'input_tokens': 1, 'output_tokens': 2}},
                    'metrics': {**metrics([]), 'turns': 1},
                    'stop': budget.stop(), 'exit_code': 0, 'trace': str(directory / (name + '.jsonl'))}
        return execute

    def gate(self, accepted, issues=None):
        return {'accepted': accepted, 'issues': issues or [], 'tool_calls': 1,
                'output_bytes': 100, 'verified_facts': {'status': 'verified'},
                'verified_profile': {'investigation': {'roles': []}}}

    def test_missing_verification_feedback_allows_correction_in_shared_budget(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); answer = self.setup(root)
            with patch('verification_required.run_phase', side_effect=self.phase([None, answer, {'status': 'pass', 'issues': []}])) as phases, \
                 patch('verification_required.acceptance_gate', side_effect=[self.gate(False, ['verification report missing']), self.gate(True)]):
                result = trial_required(root, 'candidate', 'development', 'haiku', 0, self.limits())
            self.assertEqual(result['execution'], 'complete')
            self.assertEqual(result['variant'], 'verification-required')
            from native_workflow import answer_schema_hashes
            self.assertEqual(result['answer_schema_sha256'], answer_schema_hashes(['verification-required'])['verification-required'])
            self.assertEqual(result['correction_attempts'], 1)
            self.assertEqual(result['runner_verification_calls'], 2)
            self.assertAlmostEqual(result['api_equivalent_cost_usd'], .3)
            self.assertIn('verification report missing', phases.call_args_list[1].args[2])
            self.assertEqual(phases.call_args_list[-1].kwargs, {'tools': False})
            review_prompt = phases.call_args_list[-1].args[2]
            import json
            review_context = json.loads(review_prompt.split('\n', 1)[1])
            self.assertEqual(review_context['domain_semantics'], DOMAIN_SEMANTICS)
            self.assertEqual(review_context['requested_subjects']['capture'], ['Checkpoint alpha', 'zone-a'])
            self.assertIn('Passed independent', review_context['profile_validation'])
            self.assertEqual(review_context['verified_profile'], {'investigation': {'roles': []}})
            self.assertIn('task semantics as authoritative', review_prompt)

    def test_verified_claims_do_not_bypass_separate_prose_review(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); answer = self.setup(root)
            unsupported = copy.deepcopy(answer); unsupported['explanation'] = 'Timing proves the cache caused completion.'
            phases_data = [unsupported, {'status': 'revise', 'issues': ['unsupported causality in explanation']},
                           answer, {'status': 'pass', 'issues': []}]
            with patch('verification_required.run_phase', side_effect=self.phase(phases_data)) as phases, \
                 patch('verification_required.acceptance_gate', return_value=self.gate(True)):
                result = trial_required(root, 'candidate', 'development', 'opus', 0, self.limits())
            self.assertEqual(result['execution'], 'complete')
            self.assertIn('unsupported causality', phases.call_args_list[2].args[2])
            self.assertEqual(result['correction_attempts'], 1)
            self.assertAlmostEqual(result['api_equivalent_cost_usd'], .4)
            self.assertEqual(result['answer']['explanation'], answer['explanation'])

    def test_empty_prose_rejection_has_feedback_and_failed_correction_is_counted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); answer = self.setup(root)
            with patch('verification_required.run_phase', side_effect=self.phase(
                    [answer, {'status': 'revise', 'issues': []}, answer], [.1, .1, 1.1])) as phases, \
                 patch('verification_required.acceptance_gate', return_value=self.gate(True)):
                result = trial_required(root, 'candidate', 'development', 'haiku', 0, self.limits())
            self.assertEqual(result['execution'], 'cost_limit')
            self.assertEqual(result['correction_attempts'], 1)
            self.assertIn('Independent prose review did not pass', phases.call_args_list[-1].args[2])
            self.assertEqual(result['score']['status'], 'unavailable')

    def test_budget_exhaustion_never_accepts_a_structurally_correct_answer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); answer = self.setup(root)
            with patch('verification_required.run_phase', side_effect=self.phase([answer], [1.1])), \
                 patch('verification_required.acceptance_gate') as gate:
                result = trial_required(root, 'candidate', 'development', 'opus', 0, self.limits())
            gate.assert_not_called()
            self.assertEqual(result['execution'], 'cost_limit')
            self.assertEqual(result['score']['status'], 'unavailable')

    def test_missing_usage_and_all_final_burst_limits_are_not_measured_zero(self):
        for field, value, stop in [('tools', 31, 'tool_limit'), ('output_bytes', 2_000_001, 'output_limit'),
                                   ('usage_known', False, 'usage_unavailable'), ('cost', 1, 'cost_limit')]:
            budget = Budget(self.limits()); setattr(budget, field, value)
            self.assertEqual(budget.stop(), stop)
        with patch('verification_required.time.monotonic', side_effect=[0, 61]):
            self.assertEqual(Budget(self.limits()).stop(), 'wall_limit')

    def test_three_failed_proposals_are_retained_and_never_scored_as_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); answer = self.setup(root)
            with patch('verification_required.run_phase', side_effect=self.phase([answer] * 3)) as phases, \
                 patch('verification_required.acceptance_gate', return_value=self.gate(False, ['capture: contradicted'])):
                result = trial_required(root, 'candidate', 'development', 'haiku', 0, self.limits())
            self.assertEqual(phases.call_count, 3)
            self.assertEqual(result['execution'], 'verification_rejected')
            self.assertEqual(result['score']['status'], 'unavailable')
            self.assertEqual(len(result['acceptance_checks']), 3)

    def test_required_answer_and_reviewer_schemas_are_explicit(self):
        validate({'status': 'pass', 'issues': []}, PROSE_SCHEMA)
        with self.assertRaises(InvalidValue): validate({'status': 'pass', 'issues': 'not reviewed'}, PROSE_SCHEMA)
        self.assertEqual(REQUIRED_SCHEMA['properties']['claim_ids']['properties']['coverage']['minItems'], 1)
        self.assertIn('verification', REQUIRED_SCHEMA['required'])


if __name__ == '__main__': unittest.main()
