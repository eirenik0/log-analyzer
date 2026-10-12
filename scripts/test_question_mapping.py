import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('question_mapping', Path(__file__).resolve().parents[1] / '.agents/skills/analyze-logs/scripts/map_questions.py')
mapping = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mapping)


class QuestionMappingTests(unittest.TestCase):
    def setUp(self):
        self.artifact = {'findings': [{'id': 'root-a', 'details': {'event_chain': {
            'root': {'line': 1}, 'root_event_name': 'Start', 'identity': [{'value': 'a'}]}}}],
            'report_metadata': {'evidence': {'query': {'command': {'Investigate': {
                'required_relationship': ['duration', 'count', 'cause']}}}}}}
        self.draft = {'claims': [
            {'id': 'interval-a', 'kind': 'interval', 'pointer': '/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval',
             'expected_ms': 5, 'subject': {'source': {'line': 1}, 'start_operation': 'Start', 'end_operation': 'End'}},
            {'id': 'count-a', 'kind': 'distinct_records', 'pointer': '/records', 'expected': 1}]}
        self.plan = {'questions': [{'id': 'duration', 'claim_id': 'interval-a'},
                                  {'id': 'count', 'claim_id': 'count-a', 'population': {'pointer': '/records', 'operation': None}},
                                  {'id': 'cause', 'kind': 'fact', 'finding_id': 'root-a', 'reason': 'No mechanism established'}],
                     'reviewed_checks': mapping.CHECKS}

    def positive(self):
        result = mapping.compile_plan(self.draft, self.artifact, self.plan)
        self.assertEqual(result['requirements'][0], {'id': 'duration', 'kind': 'interval', 'finding_id': 'root-a', 'start_operation': 'Start', 'end_operation': 'End'})
        self.assertEqual(result['unresolved'], [{'id': 'cause', 'reason': 'No mechanism established'}])
        self.assertEqual(result['final_review']['unresolved_question_ids'], ['cause'])
        self.assertEqual(result['claims'][0]['id'], 'duration')
        self.assertEqual(self.draft['claims'][0]['id'], 'interval-a')
        return result

    def test_mappings_preserve_all_ids_and_explicit_unknowns(self):
        self.positive()
        for mutate in [lambda p: p['questions'].pop(),
                       lambda p: p['questions'][0].update(claim_id='missing'),
                       lambda p: p['questions'][2].update(finding_id='wrong-root'),
                       lambda p: p.update(reviewed_checks=[]),
                       lambda p: p['questions'][2].update(reason=''),
                       lambda p: p['questions'][2].update(id='count'),
                       lambda p: p.update(supporting_claim_ids=['interval-a'])]:
            bad = copy.deepcopy(self.plan)
            mutate(bad)
            with self.assertRaises((ValueError, KeyError)):
                mapping.compile_plan(self.draft, self.artifact, bad)
            self.positive()

    def test_unknown_intervals_require_explicit_expected_ending(self):
        plan = copy.deepcopy(self.plan)
        plan['questions'][0] = {'id': 'duration', 'kind': 'interval', 'finding_id': 'root-a', 'reason': 'No supported ending'}
        with self.assertRaises(KeyError):
            mapping.compile_plan(self.draft, self.artifact, plan)
        plan['questions'][0]['end_operation'] = 'End'
        result = mapping.compile_plan(self.draft, self.artifact, plan)
        self.assertEqual(result['requirements'][0]['start_operation'], 'Start')
        self.assertEqual(result['final_review']['unresolved_question_ids'], ['duration', 'cause'])
        self.assertEqual(len(result['claims']), 1)

    def test_count_selection_must_match_independent_population(self):
        self.positive()
        for operation in ({'kind': 'request', 'name': 'send'}, {'kind': 'request', 'name': 'response'}):
            draft = copy.deepcopy(self.draft)
            draft['claims'][1]['operation'] = operation
            with self.assertRaisesRegex(ValueError, 'population'):
                mapping.compile_plan(draft, self.artifact, self.plan)
            plan = copy.deepcopy(self.plan)
            plan['questions'][1]['population']['operation'] = operation
            result = mapping.compile_plan(draft, self.artifact, plan)
            self.assertEqual(result['requirements'][1]['population'], plan['questions'][1]['population'])
            draft['claims'][1]['pointer'] = '/other-scope'
            with self.assertRaisesRegex(ValueError, 'population'):
                mapping.compile_plan(draft, self.artifact, plan)
        draft = copy.deepcopy(self.draft)
        draft['claims'].append({'id': 'response-a', 'kind': 'distinct_records', 'pointer': '/responses', 'expected': 2})
        plan = copy.deepcopy(self.plan)
        plan['questions'].append({'id': 'response-count', 'claim_id': 'response-a',
                                  'population': {'pointer': '/responses', 'operation': None}})
        mapping.compile_plan(draft, self.artifact, plan)
        plan['questions'][1]['claim_id'], plan['questions'][-1]['claim_id'] = 'response-a', 'count-a'
        with self.assertRaisesRegex(ValueError, 'population'):
            mapping.compile_plan(draft, self.artifact, plan)
        plan = copy.deepcopy(self.plan)
        del plan['questions'][1]['population']
        with self.assertRaisesRegex(ValueError, 'population'):
            mapping.compile_plan(self.draft, self.artifact, plan)
        self.positive()

    def test_nonroot_interval_is_not_a_question_duration(self):
        wrong = copy.deepcopy(self.draft)
        wrong['claims'][0]['subject']['source'] = {'line': 2}
        with self.assertRaises(ValueError):
            mapping.compile_plan(wrong, self.artifact, self.plan)
        self.positive()


    def test_bounded_preview_keeps_all_selectors_without_full_address_duplication(self):
        import json
        artifact = copy.deepcopy(self.artifact)
        occurrence = {'input_ordinal': 0, 'snapshot_id': 's' * 64,
                      'evidence_ref': {'input_id': 'i' * 64, 'reference_id': 'r' * 64, 'line': 1, 'row_path': None}}
        artifact['findings'][0]['details']['event_chain']['root'] = occurrence
        claims = []
        for index in range(6):
            claims.extend([
                {'id': f'link-{index}', 'kind': 'link', 'finding_id': 'root-a',
                 'subject': {'source': occurrence, 'target': {**occurrence, 'evidence_ref': {**occurrence['evidence_ref'], 'line': index + 2}}}},
                {'id': f'interval-{index}', 'kind': 'interval', 'pointer': '/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval',
                 'expected_ms': index, 'subject': {'source': occurrence, 'target': occurrence, 'start_operation': 'Start', 'end_operation': 'End'}},
                {'id': f'count-{index}', 'kind': 'distinct_records', 'pointer': '/findings/0/details/event_chain/steps/0/evidence', 'expected': 1}])
        inventory = mapping.inventory({'claims': claims}, artifact)
        text = json.dumps(inventory, ensure_ascii=False, separators=(',', ':')).encode('utf-8')
        preview = json.loads(text[:5000])
        self.assertEqual({r['claim_id'] for r in preview['supported_options']}, {c['id'] for c in claims})
        self.assertEqual(preview['required_initial_ids'], ['duration', 'count', 'cause'])
        self.assertEqual(preview['unknown_anchors'][0]['root']['line'], 1)
        self.assertEqual(preview['required_review_checks'], mapping.CHECKS)
        self.assertEqual(claims[0]['subject']['source']['snapshot_id'], 's' * 64)
