import copy
import unittest
from native_workflow import fixture, score, metrics, final_json, ANSWER_SCHEMA


class NativeWorkflowTests(unittest.TestCase):
    def test_final_tool_burst_cannot_bypass_trial_limit(self):
        import io, json, tempfile
        from pathlib import Path
        from unittest.mock import Mock, patch
        from native_workflow import prepare_inputs, trial
        answer, _ = self.answer()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepare_inputs(root, ['development'])
            skill = root / 'candidate/.agents/skills/analyze-logs/SKILL.md'
            skill.parent.mkdir(parents=True)
            skill.write_text('Synthetic skill', encoding='utf-8')
            (root / 'candidate/log-analyzer').write_bytes(b'synthetic binary')
            for count, overrides, stop in [(30, {}, None), (31, {}, 'tool_limit'),
                                           (30, {'output_bytes':1}, 'output_limit'),
                                           (30, {'wall_seconds':0}, 'wall_limit')]:
                def exited_process(*args, **kwargs):
                    events = [{'type':'assistant','message':{'content':[{'type':'tool_use','name':'Read'}] * count}},
                              {'type':'result','structured_output':answer}]
                    kwargs['stdout'].write(('\n'.join(map(json.dumps, events)) + '\n').encode())
                    return Mock(stdin=io.BytesIO(), poll=Mock(return_value=0), returncode=0)
                limits = {'wall_seconds':60,'output_bytes':2_000_000,'tool_calls':30,'api_equivalent_usd':1}
                limits.update(overrides)
                with patch('native_workflow.subprocess.Popen', side_effect=exited_process), patch('native_workflow.time.sleep') as sleep:
                    result = trial(root, 'candidate', 'development', 'haiku', count, limits)
                sleep.assert_not_called()
                self.assertEqual(result['tool_calls'], count)
                self.assertEqual(result['execution'], stop or 'complete')
                self.assertEqual(result['score']['status'], 'unavailable' if stop else 'passed')

    def answer(self, held_out=False):
        _, expected=fixture(held_out)
        answer=copy.deepcopy(expected)
        answer['summary']={'completed_subjects':[expected['capture']['subject'],'close'],
                           'unresolved_subjects':[expected['other']['subject']],
                           'send_count':1,'response_count':1}
        return answer,expected

    def test_matched_negative_and_positive_controls(self):
        for held_out in [False,True]:
            positive,expected=self.answer(held_out)
            self.assertEqual(score(positive,expected)['status'],'passed')
            mutations=[('capture','subject','wrong'),('capture','scope','other-zone'),
                       ('capture','elapsed_ms',49),('close','completion','unknown'),
                       ('other','completion',True),('capture','refs',[2,6]),('capture','refs',[1,6,10])]
            for subject,field,value in mutations:
                wrong=copy.deepcopy(positive);wrong[subject][field]=value
                self.assertEqual(score(wrong,expected)['status'],'failed')
                self.assertEqual(score(positive,expected)['status'],'passed')
            for key,value in [('cause','cached error caused the close'),('send_count',2)]:
                wrong=copy.deepcopy(positive);wrong[key]=value
                self.assertEqual(score(wrong,expected)['status'],'failed')
                self.assertEqual(score(positive,expected)['status'],'passed')
            wrong=copy.deepcopy(positive);wrong['summary']['completed_subjects'].append(expected['other']['subject'])
            self.assertGreater(score(wrong,expected)['summary_contradictions'],0)
            self.assertEqual(score(positive,expected)['status'],'passed')

    def test_calls_do_not_improve_factual_score(self):
        answer,expected=self.answer()
        events=[{'type':'assistant','message':{'content':[{'type':'tool_use','name':'Bash','input':{'command':'log-analyzer evidence f --collection /chain-bundles --claims claims.json'}}]}}]
        self.assertEqual(metrics(events)['chain_bundle_commands'],1)
        self.assertEqual(metrics(events)['claim_verification_commands'],1)
        self.assertEqual(score({},expected)['status'],'failed')
        self.assertEqual(score(answer,expected)['status'],'passed')

    def test_missing_answer_and_usage_are_unavailable(self):
        answer,result=final_json([])
        self.assertIsNone(answer);self.assertEqual(result,{})
        self.assertEqual(score(answer,fixture()[1])['status'],'unavailable')
        self.assertIsNone(score(answer,fixture()[1])['required_relationship_coverage'])

    def test_format_errors_do_not_report_wrong_subjects(self):
        positive, expected = self.answer()
        wrong = copy.deepcopy(positive)
        wrong['capture']['scope'] = {'session_id': expected['capture']['scope']}
        result = score(wrong, expected)
        self.assertEqual(result['status'], 'failed')
        self.assertEqual(result['identity_format_errors'], 1)
        self.assertEqual(result['wrong_subject_claims'], 0)
        wrong['capture']['scope'] = 'wrong-session'
        result = score(wrong, expected)
        self.assertEqual(result['identity_format_errors'], 0)
        self.assertEqual(result['wrong_subject_claims'], 1)
        self.assertEqual(score(positive, expected)['status'], 'passed')


class NativeStructuredDeliveryTests(unittest.TestCase):
    def test_native_structured_output_is_not_prose_extraction(self):
        response = {'capture': {'subject': 'example'}}
        event = {'type': 'result', 'result': 'A warning before JSON', 'structured_output': response}
        self.assertEqual(final_json([event], structured=True)[0], response)
        self.assertIsNone(final_json([event])[0])
        for value in [None, [], 'a string']:
            event['structured_output'] = value
            self.assertIsNone(final_json([event], structured=True)[0])
        event['structured_output'] = response
        event['is_error'] = True
        self.assertIsNone(final_json([event], structured=True)[0])
        # Even syntactically valid prose JSON cannot replace missing native output.
        self.assertIsNone(final_json([{'type': 'result', 'result': '{}'}], structured=True)[0])
        event['is_error'] = False
        self.assertEqual(final_json([event], structured=True)[0], response)

    def test_schema_discloses_structure_without_hidden_fixture_answers(self):
        import json
        text = json.dumps(ANSWER_SCHEMA)
        for held_out in [False, True]:
            _, expected = fixture(held_out)
            self.assertNotIn(expected['capture']['subject'], text)
            self.assertNotIn(expected['capture']['scope'], text)
        self.assertEqual(ANSWER_SCHEMA['properties']['capture']['properties']['subject'], {'type': 'string'})
        self.assertEqual(ANSWER_SCHEMA['properties']['cause'], {'type': ['string', 'null']})


class NativeInputPublicationTests(unittest.TestCase):
    def test_workers_only_read_fully_published_inputs_and_changes_fail(self):
        from concurrent.futures import ThreadPoolExecutor
        from pathlib import Path
        import tempfile
        from native_workflow import prepare_inputs, input_content
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepare_inputs(root, ['development', 'held-out'])
            paths = [root / 'inputs' / (case + '.jsonl') for case in ['development', 'held-out']]
            original = [p.read_bytes() for p in paths]
            with ThreadPoolExecutor(max_workers=2) as executor:
                checks = list(executor.map(lambda case: (root / 'inputs' / (case + '.jsonl')).read_text(encoding='utf-8') == input_content(case), ['development', 'held-out'] * 8))
            self.assertTrue(all(checks))
            prepare_inputs(root, ['development', 'held-out'])
            self.assertEqual([p.read_bytes() for p in paths], original)
            paths[0].write_text('', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'frozen input changed'):
                prepare_inputs(root, ['development'])
            self.assertEqual(paths[0].read_bytes(), b'')
            paths[0].write_bytes(original[0])
            prepare_inputs(root, ['development'])
            self.assertEqual([p.read_bytes() for p in paths], original)
