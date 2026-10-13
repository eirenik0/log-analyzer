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
        self.plan = {'questions': [{'id': 'duration', 'claim_id': 'interval-a',
                                   'binding': {'finding_id': 'root-a', 'pointer': '/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval', 'start_operation': 'Start', 'end_operation': 'End'}},
                                  {'id': 'count', 'claim_id': 'count-a', 'population': {'pointer': '/records', 'operation': None}},
                                  {'id': 'cause', 'kind': 'fact', 'finding_id': 'root-a', 'reason': 'No mechanism established'}],
                     'reviewed_checks': mapping.CHECKS}

    def positive(self):
        result = mapping.compile_plan(self.draft, self.artifact, self.plan)
        self.assertEqual(result['requirements'][0], {'id': 'duration', 'kind': 'interval', 'finding_id': 'root-a', 'pointer': '/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval', 'start_operation': 'Start', 'end_operation': 'End'})
        self.assertEqual(result['unresolved'], [{'id': 'cause', 'reason': 'No mechanism established'}])
        self.assertEqual(result['final_review']['unresolved_question_ids'], ['cause'])
        self.assertEqual(result['claims'][0]['id'], 'duration')
        self.assertEqual(self.draft['claims'][0]['id'], 'interval-a')
        return result

    def test_preparation_leaves_subject_selection_and_review_unacknowledged(self):
        prepared = mapping.prepare_plan(self.artifact)
        self.assertEqual(prepared['questions'], [{'id': q} for q in ['duration', 'count', 'cause']])
        self.assertEqual(prepared['reviewed_checks'], [])
        with self.assertRaisesRegex(ValueError, 'independently select'):
            mapping.compile_plan(self.draft, self.artifact, prepared)
        artifact = copy.deepcopy(self.artifact)
        artifact['report_metadata'] = {}
        with self.assertRaisesRegex(ValueError, 'Register'):
            mapping.prepare_plan(artifact)

    def test_operation_candidates_are_retained_counts_not_subject_selections(self):
        def record(line, name):
            return {'occurrence': {'line': line}, 'fields': {'classification': {
                'status': 'event', 'semantics': {'kind': 'event', 'name': name}}}}
        artifact = copy.deepcopy(self.artifact)
        artifact.update(records=[record(1, 'send'), record(1, 'send'), record(2, 'response')],
                        processing={'status': 'complete'})
        proposed = mapping.candidates(self.draft, artifact)
        operations = {c['operation']['name']: c for c in proposed['claims'] if 'operation' in c}
        self.assertEqual(operations['send']['expected'], 1)
        self.assertEqual(operations['response']['expected'], 1)
        self.assertEqual(proposed['claims'][-1]['pointer'], '/processing/status')
        self.assertNotIn('requirements', proposed)
        artifact['records'][0]['fields']['classification']['status'] = 'unclassified'
        proposed = mapping.candidates(self.draft, artifact)
        self.assertFalse(any('operation' in c for c in proposed['claims']))

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

    def test_redacted_initial_ids_are_checked_before_creating_outputs(self):
        import hashlib, json, subprocess, sys, tempfile
        artifact = copy.deepcopy(self.artifact)
        initial = ['opaque-' + hashlib.sha256(item['id'].encode()).hexdigest() for item in self.plan['questions']]
        artifact['content'] = 'redacted'
        artifact['question_checklist'] = {'question': ['[REDACTED QUESTION]'], 'required_relationship': initial}
        artifact['report_metadata']['evidence']['query']['command'] = '[REDACTED QUERY]'
        self.assertEqual(mapping.inventory(self.draft, artifact)['required_initial_ids'], initial)
        plan = copy.deepcopy(self.plan)
        for item, qid in zip(plan['questions'], initial):
            item['id'] = qid
        self.assertEqual(mapping.compile_plan(self.draft, artifact, plan)['required_claim_ids'], initial)
        plan['questions'].pop()
        with self.assertRaisesRegex(ValueError, 'initial required'):
            mapping.compile_plan(self.draft, artifact, plan)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, value in [('draft', self.draft), ('artifact', artifact), ('plan', plan)]:
                (root / name).write_text(json.dumps(value), encoding='utf-8')
            missing_draft = root / 'missing-draft'
            inventory = subprocess.run([sys.executable, mapping.__file__, '--draft', str(missing_draft),
                                        '--artifact', str(root / 'artifact')], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(inventory.returncode, 0, inventory.stderr)
            view = json.loads(inventory.stdout)
            self.assertEqual(view['required_initial_ids'], initial)
            self.assertEqual(view['supported_options'], [])
            self.assertEqual(view['event_candidates'][0]['finding_id'], 'root-a')
            self.assertFalse(missing_draft.exists())
            output, verification = root / 'claims.json', root / 'verification.json'
            result = subprocess.run([sys.executable, mapping.__file__, '--draft', str(root / 'draft'),
                                     '--artifact', str(root / 'artifact'), '--plan', str(root / 'plan'),
                                     '--output', str(output), '--analyzer', sys.executable,
                                     '--verification-output', str(verification)], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('initial required', result.stderr)
            self.assertFalse(output.exists())
            self.assertFalse(verification.exists())
        del artifact['question_checklist']
        self.assertEqual(mapping.inventory(self.draft, artifact)['required_initial_ids'], [])

    def test_artifact_allowance_reaches_drafting_and_verification(self):
        import contextlib, io, json, sys, tempfile
        from types import SimpleNamespace
        from unittest.mock import patch
        for allowance in [None, '100000000']:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                for name, value in [('artifact', self.artifact), ('plan', self.plan)]:
                    (root / name).write_text(json.dumps(value), encoding='utf-8')
                argv = ['map_questions', '--draft', str(root / 'draft'), '--artifact', str(root / 'artifact'),
                        '--plan', str(root / 'plan'), '--output', str(root / 'claims'),
                        '--analyzer', sys.executable, '--verification-output', str(root / 'verification')]
                if allowance is not None: argv.extend(['--artifact-max-bytes', allowance])
                def analyzer(command, **kwargs):
                    destination = Path(command[command.index('--output') + 1])
                    value = self.draft if '--draft-claims' in command else {'artifact_retrieval': {'claim_verification': {'status': 'verified'}}}
                    destination.write_text(json.dumps(value), encoding='utf-8')
                    return SimpleNamespace(returncode=0)
                with patch('sys.argv', argv), patch.object(mapping.subprocess, 'run', side_effect=analyzer) as run, contextlib.redirect_stdout(io.StringIO()):
                    mapping.main()
                self.assertEqual(run.call_count, 2)
                for call in run.call_args_list:
                    command = call.args[0]
                    if allowance is None:
                        self.assertNotIn('--artifact-max-bytes', command)
                    else:
                        self.assertEqual(command[command.index('--artifact-max-bytes') + 1], allowance)

    def test_artifact_reads_are_bounded_and_hash_the_parsed_bytes(self):
        import hashlib, io, json
        from types import SimpleNamespace
        from unittest.mock import Mock
        content = json.dumps(self.artifact).encode('utf-8')
        path = Mock(spec=Path)
        path.stat.return_value = SimpleNamespace(st_size=67108865)
        with self.assertRaisesRegex(ValueError, 'Artifact exceeds'):
            mapping.read_artifact(path, 67108864)
        path.open.assert_not_called()
        path.stat.return_value = SimpleNamespace(st_size=len(content))
        path.open.return_value = io.BytesIO(content)
        value, sha = mapping.read_artifact(path, len(content))
        self.assertEqual(value, self.artifact)
        self.assertEqual(sha, hashlib.sha256(content).hexdigest())
        path.reset_mock()
        with self.assertRaisesRegex(ValueError, 'Artifact exceeds'):
            mapping.read_artifact(path, len(content) - 1)
        path.open.assert_not_called()
        path.stat.return_value = SimpleNamespace(st_size=1)
        path.open.return_value = io.BytesIO(content)
        with self.assertRaisesRegex(ValueError, 'while reading'):
            mapping.read_artifact(path, 1)

    def test_output_collisions_fail_before_drafting_or_publication(self):
        import contextlib, io, json, sys, tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, value in [('artifact', self.artifact), ('plan', self.plan)]:
                (root / name).write_text(json.dumps(value), encoding='utf-8')
            draft = root / 'draft'
            for output, verification in [(root / 'output', root / 'output'),
                                         (root / 'output', root / 'unused/../output'),
                                         (draft, root / 'verification'),
                                         (root / 'output', draft)]:
                argv = ['map_questions', '--draft', str(draft), '--artifact', str(root / 'artifact'),
                        '--plan', str(root / 'plan'), '--output', str(output),
                        '--analyzer', sys.executable, '--verification-output', str(verification)]
                error = io.StringIO()
                with patch('sys.argv', argv), patch.object(mapping.subprocess, 'run') as run, contextlib.redirect_stderr(error):
                    with self.assertRaises(SystemExit) as caught:
                        mapping.main()
                self.assertEqual(caught.exception.code, 1)
                self.assertIn('paths must be distinct', error.getvalue())
                run.assert_not_called()
                self.assertFalse(draft.exists())
                self.assertFalse(output.exists())
                self.assertFalse(verification.exists())

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

    def test_unknown_intervals_preserve_intermediate_starts_and_unicode_reason_limits(self):
        plan = copy.deepcopy(self.plan)
        plan['questions'][0] = {'id': 'duration', 'kind': 'interval', 'finding_id': 'root-a',
                                'start_operation': 'CaptureStart', 'end_operation': 'End', 'pointer': '/findings/0/details/event_chain/steps/0/join_samples/1/endpoint_interval', 'reason': '界' * 4096}
        result = mapping.compile_plan(self.draft, self.artifact, plan)
        self.assertEqual(result['requirements'][0]['start_operation'], 'CaptureStart')
        self.assertEqual(result['requirements'][0]['pointer'], plan['questions'][0]['pointer'])
        self.assertEqual(result['unresolved'][0]['reason'], '界' * 4096)
        for change in ({'start_operation': ''}, {'reason': '界' * 4097}):
            bad = copy.deepcopy(plan)
            bad['questions'][0].update(change)
            with self.assertRaises(ValueError):
                mapping.compile_plan(self.draft, self.artifact, bad)

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

    def test_link_fact_and_interval_bindings_are_independent_of_selection(self):
        for first, second in [
            ({'kind': 'link', 'finding_id': 'root-a', 'step_id': 'end', 'subject': {'source': {'line': 1}, 'target': {'line': 2}}},
             {'kind': 'link', 'finding_id': 'root-a', 'step_id': 'end', 'subject': {'source': {'line': 1}, 'target': {'line': 3}}}),
            ({'kind': 'fact', 'pointer': '/first', 'equals': True},
             {'kind': 'fact', 'pointer': '/second', 'equals': False}),
            (self.draft['claims'][0], {**self.draft['claims'][0],
                                     'pointer': '/findings/0/details/event_chain/steps/0/join_samples/1/endpoint_interval'}),
            (self.draft['claims'][0], {**self.draft['claims'][0],
                                     'subject': {**self.draft['claims'][0]['subject'], 'end_operation': 'Other end'}})
        ]:
            draft = {'claims': [{**first, 'id': 'first'}, {**second, 'id': 'second'}]}
            artifact = copy.deepcopy(self.artifact)
            artifact['report_metadata'] = {}
            if first['kind'] == 'link':
                artifact['findings'][0]['details']['event_chain']['steps'] = [
                    {'id': 'end', 'join_samples': [c['subject'] for c in draft['claims']]}]
            bindings = [{k: v for k, v in mapping.requirement(c, artifact).items() if k != 'kind'}
                        for c in draft['claims']]
            plan = {'questions': [{'id': cid, 'claim_id': cid, 'binding': binding}
                                  for cid, binding in zip(('first', 'second'), bindings)],
                    'reviewed_checks': mapping.CHECKS}
            mapping.compile_plan(draft, artifact, plan)
            plan['questions'][0]['claim_id'], plan['questions'][1]['claim_id'] = 'second', 'first'
            with self.assertRaisesRegex(ValueError, 'binding'):
                mapping.compile_plan(draft, artifact, plan)

    def test_unknown_links_and_counts_keep_explicit_selectors(self):
        plan = copy.deepcopy(self.plan)
        plan['questions'][0] = {'id': 'duration', 'kind': 'link', 'finding_id': 'root-a',
                                'step_id': 'end', 'reason': 'Ambiguous relationship'}
        plan['questions'][1] = {'id': 'count', 'kind': 'distinct_records', 'finding_id': 'root-a',
                                'population': {'pointer': '/records', 'operation': None}, 'reason': 'Unclassified records'}
        result = mapping.compile_plan(self.draft, self.artifact, plan)
        self.assertEqual(result['claims'], [])
        self.assertEqual(result['requirements'][0]['step_id'], 'end')
        self.assertEqual(result['requirements'][1]['population'], plan['questions'][1]['population'])
        plan['questions'][0]['step_id'] = ''
        with self.assertRaisesRegex(ValueError, 'step_id'):
            mapping.compile_plan(self.draft, self.artifact, plan)
        del plan['questions'][0]['step_id']
        with self.assertRaises(KeyError):
            mapping.compile_plan(self.draft, self.artifact, plan)
        plan['questions'][0].update(step_id='end', source={'line': 1}, target={'line': 3})
        result = mapping.compile_plan(self.draft, self.artifact, plan)
        self.assertEqual(result['requirements'][0]['source'], {'line': 1})
        self.assertEqual(result['requirements'][0]['target'], {'line': 3})
        del plan['questions'][0]['target']
        with self.assertRaises(KeyError):
            mapping.compile_plan(self.draft, self.artifact, plan)

    def test_intermediate_interval_compiles_and_remains_visible(self):
        draft = copy.deepcopy(self.draft)
        draft['claims'][0]['subject'].update(source={'input_ordinal': 0, 'evidence_ref': {'line': 2, 'row_path': None}},
                                           start_operation='Submitted')
        plan = copy.deepcopy(self.plan)
        plan['questions'][0]['binding']['start_operation'] = 'Submitted'
        result = mapping.compile_plan(draft, self.artifact, plan)
        self.assertEqual(result['requirements'][0]['start_operation'], 'Submitted')
        self.assertEqual(result['claims'][0]['subject'], draft['claims'][0]['subject'])
        option = mapping.inventory(draft, self.artifact)['supported_options'][0]
        self.assertEqual(option['claim_id'], 'interval-a')
        self.assertEqual(option['source']['line'], 2)
        with self.assertRaisesRegex(ValueError, 'matching binding'):
            mapping.compile_plan(draft, self.artifact, self.plan)
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
                {'id': f'link-{index}', 'kind': 'link', 'finding_id': 'root-a', 'step_id': 'end',
                 'subject': {'source': occurrence, 'target': {**occurrence, 'evidence_ref': {**occurrence['evidence_ref'], 'line': index + 2}}}},
                {'id': f'interval-{index}', 'kind': 'interval', 'pointer': '/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval',
                 'expected_ms': index, 'subject': {'source': occurrence, 'target': occurrence, 'start_operation': 'Start', 'end_operation': 'End'}},
                {'id': f'count-{index}', 'kind': 'distinct_records', 'pointer': '/findings/0/details/event_chain/steps/0/evidence', 'expected': 1}])
        artifact['findings'][0]['details']['event_chain']['steps'] = [
            {'id': 'end', 'join_samples': [c['subject'] for c in claims if c['kind'] == 'link']}]
        inventory = mapping.inventory({'claims': claims}, artifact)
        text = json.dumps(inventory, ensure_ascii=False, separators=(',', ':')).encode('utf-8')
        preview = json.loads(text[:5000])
        self.assertEqual({r['claim_id'] for r in preview['supported_options']}, {c['id'] for c in claims})
        self.assertEqual(preview['required_initial_ids'], ['duration', 'count', 'cause'])
        self.assertEqual(preview['event_candidates'][0]['root']['line'], 1)
        self.assertEqual(preview['required_review_checks'], mapping.CHECKS)
        self.assertEqual(claims[0]['subject']['source']['snapshot_id'], 's' * 64)
