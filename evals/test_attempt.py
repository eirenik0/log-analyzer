import json
import subprocess
import sys
import unittest
from attempt import Attempt, BudgetExceeded, aggregate_usage, evaluate_attempt
from agents import adapter_call


class RetainedAttemptTests(unittest.TestCase):
    def grade(self, answer):
        return {'status': 'PASS' if answer == {'finding': 'known'} else 'FAIL'}

    def assess(self, attempt, execute, validate=lambda answer, score: [], checkpoint=lambda: None):
        return evaluate_attempt(attempt, execute, self.grade, validate, checkpoint)

    def test_final_answer_before_failing_budget_check(self):
        attempt = Attempt()
        def execute():
            attempt.receive({'final': {'finding': 'known'}, 'usage': {'tokens': 9}})
            raise BudgetExceeded('final budget check')
        result = self.assess(attempt, execute)
        self.assertEqual(result['execution_status'], 'failed')
        self.assertEqual(result['budget_compliance'], 'exceeded')
        self.assertTrue(result['answer_available'])
        self.assertEqual(result['factual_quality']['status'], 'PASS')
        self.assertEqual(attempt.usage()['tokens'], 9)
        self.assertEqual(result['score']['status'], 'ERROR')

    def test_reported_usage_before_later_execution_failure_and_missing_usage(self):
        attempt = Attempt()
        def execute():
            attempt.responses_started += 1
            attempt.receive({'tool_call': {}, 'usage': {'tokens': 11, 'provider_cost_usd': 0.02}})
            attempt.responses_started += 1
            raise OSError('later adapter failed without a response')
        result = self.assess(attempt, execute)
        self.assertFalse(result['answer_available'])
        self.assertEqual(result['execution_status'], 'failed')
        usage = attempt.usage()
        self.assertIsNone(usage['tokens'])
        self.assertEqual(usage['tokens_known_total'], 11)
        self.assertEqual(usage['tokens_completeness'], 'partial')
        self.assertEqual(usage['tokens_missing_responses'], 1)
        totals = aggregate_usage([{'usage': usage}, {'usage': Attempt().usage()}])
        self.assertIsNone(totals['tokens'])
        self.assertEqual(totals['tokens_known_total'], 11)
        self.assertEqual(totals['tokens_incomplete_attempts'], 2)

    def test_answer_and_usage_survive_validation_failure(self):
        attempt = Attempt()
        def execute(): attempt.receive({'final': {'finding': 'known'}, 'usage': {'tokens': 4, 'provider_cost_usd': 0}})
        def validate(answer, score): raise ValueError('schema validation failed')
        result = self.assess(attempt, execute, validate)
        self.assertEqual(result['execution_status'], 'completed')
        self.assertEqual(result['validation_status'], 'failed')
        self.assertEqual(result['factual_quality']['status'], 'PASS')
        self.assertEqual(attempt.answer, {'finding': 'known'})
        self.assertEqual(attempt.usage()['tokens'], 4)
        self.assertEqual(attempt.usage()['provider_cost_usd'], 0)

    def test_frame_observed_before_nonzero_exit(self):
        attempt = Attempt()
        program = 'import json,sys; print(json.dumps({"final":{"finding":"known"},"usage":{"tokens":7}}), flush=True); sys.exit(3)'
        with self.assertRaisesRegex(AssertionError, 'execution failed'):
            adapter_call([sys.executable, '-c', program], {}, 2, on_frame=attempt.receive)
        self.assertEqual(attempt.answer, {'finding': 'known'})
        self.assertEqual(attempt.usage()['tokens'], 7)

    def test_frame_observed_before_deadline_failure(self):
        attempt = Attempt()
        program = 'import json,time; print(json.dumps({"final":{"finding":"known"},"usage":{"tokens":5}}), flush=True); time.sleep(2)'
        with self.assertRaises(subprocess.TimeoutExpired):
            adapter_call([sys.executable, '-c', program], {}, 0.3, on_frame=attempt.receive)
        self.assertEqual(attempt.answer, {'finding': 'known'})
        self.assertEqual(attempt.usage()['tokens'], 5)

    def test_missing_and_invalid_usage_are_unknown_not_zero(self):
        attempt = Attempt()
        self.assertIsNone(attempt.usage()['tokens_known_total'])
        attempt.receive({'final': {}, 'usage': {'tokens': True, 'provider_cost_usd': float('nan')}})
        self.assertIsNone(attempt.usage()['tokens'])
        self.assertIsNone(attempt.usage()['provider_cost_usd_known_total'])

    def test_observer_handoff_finishes_before_timeout_is_returned(self):
        import threading, time
        attempt = Attempt()
        caller = threading.get_ident()
        observed = []
        def delayed(frame):
            time.sleep(0.04)
            observed.append(threading.get_ident())
            attempt.receive(frame)
        program = 'import json,time; print(json.dumps({"final":{"finding":"known"}}),flush=True); time.sleep(2)'
        with self.assertRaises(subprocess.TimeoutExpired):
            adapter_call([sys.executable, '-c', program], {}, 0.2, on_frame=delayed)
        self.assertEqual(observed, [caller])
        self.assertEqual(attempt.answer, {'finding': 'known'})

    def test_schema_valid_wrong_fact_keeps_validation_independent(self):
        attempt = Attempt()
        result = self.assess(attempt, lambda: attempt.receive({'final': {'finding': 'incorrect'}}))
        self.assertEqual(result['execution_status'], 'completed')
        self.assertEqual(result['validation_status'], 'passed')
        self.assertEqual(result['factual_quality']['status'], 'FAIL')
        self.assertEqual(result['score']['status'], 'FAIL')

    def test_unknown_cost_stops_without_claiming_measured_overspend(self):
        from agents import external
        from unittest.mock import patch
        from attempt import BudgetUnknown
        class Tools:
            budgets = {'tool_calls': 1, 'output_bytes': 1000}
            calls, output_bytes, arm = [], 0, 'analyzer'
            def remaining(self): return 2
        attempt = Attempt()
        def reply(*args, **kwargs):
            frame = {'final': {'finding': 'known'}, 'usage': {'tokens': 3}}
            kwargs['on_frame'](frame)
            return frame
        with patch('agents.adapter_call', side_effect=reply):
            result = self.assess(attempt, lambda: external(Tools(), {}, ['trusted-adapter'], 'model', {}, attempt, 1))
        self.assertEqual(result['budget_compliance'], 'unknown')
        self.assertTrue(result['answer_available'])
        self.assertIsNone(attempt.usage()['provider_cost_usd_known_total'])

    def test_failed_analyzer_outputs_are_charged_before_validation(self):
        from investigation import Tools, ROOT
        from unittest.mock import patch
        profile = str(ROOT / 'examples/investigations/profile.toml')
        scenario = {'profile': profile}
        contexts = {'run': {'inputs': [], 'profile_sources': []}}
        for outcome in [subprocess.CompletedProcess([], 3, b'bad exit'), subprocess.CompletedProcess([], 0, b'invalid json'), subprocess.TimeoutExpired([], 1, output=b'partial stdout')]:
            tools = Tools(sys.executable, scenario, 'analyzer', contexts, {'tool_calls': 3, 'output_bytes': 1000, 'wall_seconds': 10})
            with patch('investigation.subprocess.run', **({'side_effect': outcome} if isinstance(outcome, Exception) else {'return_value': outcome})):
                with self.assertRaises(Exception): tools.invoke({'group': 'run', 'tool': 'analyzer', 'command': 'info'})
            self.assertEqual(tools.output_bytes, len(outcome.stdout))
            self.assertEqual(len(tools.calls), 1)

    def test_unknown_total_disables_later_paid_calls_with_known_partial_usage(self):
        from attempt import Allocation, BudgetUnknown
        allocation = Allocation(1)
        attempt = Attempt()
        attempt.responses_started = 2
        attempt.receive({'tool_call': {}, 'usage': {'tokens': 6, 'provider_cost_usd': 0.03}})
        allocation.record(attempt)
        self.assertEqual(allocation.spent, 0.03)
        with self.assertRaises(BudgetUnknown): allocation.remaining()
        self.assertEqual(attempt.usage()['tokens_known_total'], 6)

    def test_known_overspend_preserves_received_answer_and_cost(self):
        from agents import external
        from unittest.mock import patch
        class Tools:
            budgets = {'tool_calls': 1, 'output_bytes': 1000}
            calls, output_bytes, arm = [], 0, 'analyzer'
            def remaining(self): return 2
        attempt = Attempt()
        def reply(*args, **kwargs):
            frame = {'final': {'finding': 'known'}, 'usage': {'tokens': 3, 'provider_cost_usd': 2}}
            kwargs['on_frame'](frame)
            return frame
        with patch('agents.adapter_call',side_effect=reply):
            result = self.assess(attempt,lambda:external(Tools(),{},['trusted-adapter'],'model',{},attempt,1))
        self.assertEqual(result['budget_compliance'],'exceeded')
        self.assertTrue(result['answer_available'])
        self.assertEqual(attempt.usage()['provider_cost_usd'],2)

    def test_adapter_receives_decreasing_provider_budget(self):
        from agents import external
        from unittest.mock import patch
        class Tools:
            budgets = {'tool_calls': 2, 'output_bytes': 1000}
            output_bytes, arm = 0, 'analyzer'
            def __init__(self): self.calls = []
            def remaining(self): return 2
            def invoke(self, request):
                self.calls.append(request)
                return {}
        attempt, budgets = Attempt(), []
        def reply(command, payload, timeout, on_frame):
            budgets.append(payload['remaining']['provider_cost_usd'])
            frame = {'tool_call': {}, 'usage': {'provider_cost_usd': 0.25}} if len(budgets) == 1 else {'final': {'finding': 'known'}, 'usage': {'provider_cost_usd': 0.1}}
            on_frame(frame)
            return frame
        with patch('agents.adapter_call', side_effect=reply):
            external(Tools(), {}, ['adapter'], 'model', {}, attempt, 1)
        self.assertEqual(budgets, [1, 0.75])
        self.assertEqual(attempt.usage()['provider_cost_usd'], 0.35)

    def test_exact_allocation_exhaustion_stops_before_next_paid_response(self):
        from agents import external
        from unittest.mock import patch
        class Tools:
            budgets = {'tool_calls': 2, 'output_bytes': 1000}
            output_bytes, arm = 0, 'analyzer'
            def __init__(self): self.calls = []
            def remaining(self): return 2
            def invoke(self, request): self.calls.append(request); return {}
        attempt = Attempt()
        def reply(command, payload, timeout, on_frame):
            frame = {'tool_call': {}, 'usage': {'provider_cost_usd': 1}}
            on_frame(frame)
            return frame
        with patch('agents.adapter_call', side_effect=reply) as adapter:
            result = self.assess(attempt, lambda: external(Tools(), {}, ['adapter'], 'model', {}, attempt, 1))
        self.assertEqual(adapter.call_count, 1)
        self.assertEqual(attempt.responses_started, 1)
        self.assertEqual(result['budget_compliance'], 'exceeded')
        self.assertEqual(attempt.usage()['provider_cost_usd'], 1)
