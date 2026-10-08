"""Regression checks for retrieval stops, incompatibility, and invalid citations."""
import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('check_examples', Path(__file__).with_name('check-examples.py'))
examples = importlib.util.module_from_spec(spec)
spec.loader.exec_module(examples)


def page(displayed=1, prior=0, cursor=None, remaining=0):
    total = prior + displayed + remaining
    return {'report_metadata': {'evidence': {'snapshot_id': 'snapshot', 'profile_sha256': 'profile', 'query_sha256': 'query'}}, 'rows': list(range(prior, prior + displayed)), 'retrieval': {'status': 'page' if cursor else 'complete', 'metadata_over_budget': False, 'report_sha256': 'report', 'prior_items': prior, 'displayed_items': displayed, 'next_cursor': cursor, 'remaining_items': remaining, 'total_items': total, 'collections': [{'path': '/rows', 'total': total, 'prior': prior, 'displayed': displayed, 'remaining': remaining}]}}


class FakeRunner(examples.Runner):
    def __init__(self, pages, max_pages=64):
        super().__init__(__file__, max_pages=max_pages)
        self.pages = iter(pages)

    def invoke(self, args, expected_exit=0):
        return next(self.pages)


class WorkflowChecks(unittest.TestCase):
    def test_unsupported_capability_is_failure(self):
        with self.assertRaisesRegex(AssertionError, 'unsupported capability'):
            examples.compatible({})

    def test_expected_nonzero_report_is_retained_and_argv_never_uses_shell(self):
        result = subprocess.CompletedProcess([], 1, b'{"coverage":{"status":"unparsed_input"}}', b'diagnostic')
        runner = examples.Runner(__file__)
        with patch.object(examples.subprocess, 'run', return_value=result) as invoke:
            report = runner.invoke(['trace', 'literal; touch injected', '--id', '$(malicious)'], expected_exit=1)
            self.assertEqual(report['coverage']['status'], 'unparsed_input')
            self.assertEqual(runner.calls[0]['exit'], 1)
            self.assertNotIn('shell', invoke.call_args.kwargs)
            self.assertEqual(invoke.call_args.args[0][2], 'literal; touch injected')
        with patch.object(examples.subprocess, 'run', return_value=result):
            with self.assertRaisesRegex(AssertionError, 'expected 0'):
                examples.Runner(__file__).invoke(['info'])

    def test_missing_progress_changed_identity_and_page_budget_stop(self):
        cursor = 'a' * 64 + ':1'
        with self.assertRaisesRegex(examples.BudgetExhausted, 'no_retrieval_progress'):
            FakeRunner([page(displayed=0, cursor=cursor, remaining=2)]).retrieve(['info'])
        altered = page(prior=1)
        altered['report_metadata']['evidence']['snapshot_id'] = 'changed'
        with self.assertRaisesRegex(AssertionError, 'identity changed'):
            FakeRunner([page(cursor=cursor, remaining=1), altered]).retrieve(['info'])
        with self.assertRaisesRegex(examples.BudgetExhausted, 'page_budget'):
            FakeRunner([page(cursor=cursor, remaining=1)], max_pages=1).retrieve(['info'])
        result = FakeRunner([page(cursor=cursor, remaining=1), page(prior=1)]).retrieve(['info'])
        self.assertEqual(result['rows'], [0, 1])
        self.assertNotIn('retrieval', result)

    def test_invalid_or_absent_citation_cannot_support_a_correct_guess(self):
        path = examples.ROOT / 'examples/investigations/failure.jsonl'
        sha = examples.hashlib.sha256(path.read_bytes()).hexdigest()
        input_id = examples.digest([str(path), sha])
        ref = {'input_id': input_id, 'line': 1, 'row_path': None, 'location_redacted': False}
        ref['reference_id'] = examples.digest([input_id, 1, None, None])
        report = {'report_metadata': {'evidence': {'inputs': [{'file': str(path), 'sha256': sha, 'input_id': input_id}]}}, 'evidence_records': [{'evidence_ref': ref}]}
        examples.check_reference(ref, report)
        for changed in [dict(ref, line=0), dict(ref, reference_id='f' * 64), dict(ref, location_redacted=True)]:
            with self.assertRaises(AssertionError):
                examples.check_reference(changed, report)
        report['evidence_records'] = []
        with self.assertRaisesRegex(AssertionError, 'absent from retrieved'):
            examples.check_reference(ref, report)

    def test_skipped_overlapping_and_inconsistent_pages_fail(self):
        cursor = 'a' * 64 + ':1'
        for pages in [[page(prior=1)], [page(cursor=cursor, remaining=1), page(displayed=2)], [page(cursor='a' * 64 + ':2', remaining=1)]]:
            with self.assertRaises(AssertionError):
                FakeRunner(pages).retrieve(['info'])
        bad = page()
        bad['rows'] = []
        with self.assertRaisesRegex(AssertionError, 'displayed collection size'):
            FakeRunner([bad]).retrieve(['info'])

    def test_forged_normalized_addresses_are_explicitly_unsupported(self):
        for row, expansion in [('/absent-normalized-row', None), (None, 1)]:
            ref = {'input_id': 'input', 'line': 1, 'row_path': row, 'expansion': expansion, 'location_redacted': False}
            ref['reference_id'] = examples.digest(['input', 1, row, expansion])
            report = {'report_metadata': {'evidence': {'inputs': [{'input_id': 'input'}]}}, 'evidence_records': [{'evidence_ref': ref}]}
            with self.assertRaisesRegex(AssertionError, 'address type unsupported'):
                examples.check_reference(ref, report)

    def test_cross_step_scope_and_independent_profile_must_agree(self):
        def report(file, snapshot, profile):
            return {'report_metadata': {'evidence': {'inputs': [{'file': file}], 'snapshot_id': snapshot, 'profile_sha256': profile}}}
        valid = report('run', 'snapshot', 'profile')
        examples.validate_step_scope({'validation': valid, 'timing': valid, 'baseline': report('baseline', 'other', 'profile')})
        for changed in [report('run', 'changed', 'profile'), report('baseline', 'other', 'different-profile')]:
            with self.assertRaises(AssertionError):
                examples.validate_step_scope({'validation': valid, 'timing': changed})

    def test_utc_z_timestamps_parse_on_python_310(self):
        self.assertEqual(examples.parse_time('2026-01-01T00:00:00Z'), examples.parse_time('2026-01-01T00:00:00+00:00'))

    def test_escaped_json_pointer(self):
        self.assertEqual(examples.pointer({'a/b': {'~key': [7]}}, '/a~1b/~0key/0'), 7)


if __name__ == '__main__':
    unittest.main()
