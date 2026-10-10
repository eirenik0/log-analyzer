import os
from pathlib import Path, PureWindowsPath
import tempfile
import unittest
from types import SimpleNamespace

from discovery import DiscoveryTools, run, score, validate_answer, windows_path_label


class DiscoveryTests(unittest.TestCase):
    def test_windows_verbatim_selectors_preserve_project_containment(self):
        root = PureWindowsPath(r'C:\project')
        selected = PureWindowsPath(windows_path_label(r'\\?\C:\project\config\team.toml'))
        self.assertTrue(selected.is_relative_to(root))
        outside = PureWindowsPath(windows_path_label(r'\\?\C:\other\team.toml'))
        self.assertFalse(outside.is_relative_to(root))
        self.assertEqual(windows_path_label(r'\\?\UNC\server\share\team.toml'), r'\\server\share\team.toml')
        self.assertEqual(windows_path_label(r'\\?\GLOBALROOT\Device\disk'), r'\\?\GLOBALROOT\Device\disk')
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory).resolve()
            (project / 'config').mkdir()
            candidate = project / 'config/team.toml'
            candidate.write_text('extends = "base"', encoding='utf-8')
            tools = DiscoveryTools(Path('/unused'), project, 'saved-mapping')
            prefix = '\\\\?\\' if os.name == 'nt' else ''
            self.assertEqual(tools.path(prefix + str(candidate)), candidate)
            with self.assertRaisesRegex(AssertionError, 'outside declared project'):
                tools.path(prefix + str(project.parent / 'outside.toml'))

    def tools(self):
        return SimpleNamespace(reports=[{'brief_version': 1, 'profile': {'profile': 'investigation-example'},
                                         'binding': {'profile_sha256': 'profile', 'snapshot_id': 'snapshot'}, 'coverage': []}],
                               calls=[{'request': {'tool': 'evidence'}}],
                               revealed={'end': {'occurrence': {'snapshot_id': 'snapshot', 'evidence_ref': {'line': 8}}}})

    def test_scorer_requires_retrieved_current_terminal_evidence_and_correct_outcome(self):
        truth = {'ended': True, 'outcome': 'failure', 'terminal_line': 8, 'limitation': None}
        answer = {'profile_sha256': 'profile', 'ended': True, 'outcome': 'failure',
                  'terminal_refs': ['end'], 'limitations': ['upstream_unknown']}
        self.assertEqual(score(answer, self.tools(), truth)['status'], 'PASS')
        for key, value in [('profile_sha256', 'wrong'), ('ended', 'unknown'), ('outcome', 'success'),
                           ('terminal_refs', []), ('terminal_refs', ['unseen']), ('limitations', [])]:
            wrong = {**answer, key: value}
            self.assertEqual(score(wrong, self.tools(), truth)['status'], 'FAIL', key)
        for mutation in ('snapshot', 'line', 'profile', 'retrieval'):
            tools = self.tools()
            if mutation == 'snapshot': tools.revealed['end']['occurrence']['snapshot_id'] = 'old'
            if mutation == 'line': tools.revealed['end']['occurrence']['evidence_ref']['line'] = 2
            if mutation == 'profile': tools.reports[0]['profile']['profile'] = 'alternate-outcome'
            if mutation == 'retrieval': tools.calls = []
            self.assertEqual(score(answer, tools, truth)['status'], 'FAIL', mutation)

    def test_abstention_requires_observed_limits_and_cannot_claim_terminal_evidence(self):
        tools = self.tools()
        truth = {'ended': 'unknown', 'outcome': 'unknown', 'terminal_line': 8, 'limitation': 'processing_cutoff'}
        answer = {'profile_sha256': 'profile', 'ended': 'unknown', 'outcome': 'unknown',
                  'terminal_refs': [], 'limitations': ['upstream_unknown', 'processing_cutoff']}
        self.assertEqual(score(answer, tools, truth)['status'], 'PASS')
        for wrong in [{**answer, 'terminal_refs': ['end']}, {**answer, 'limitations': ['upstream_unknown']}, {**answer, 'ended': True}]:
            self.assertEqual(score(wrong, tools, truth)['status'], 'FAIL')

    def test_scripted_run_cannot_claim_skill_adherence(self):
        for mode in ('entrypoint', 'discover'):
            with self.assertRaisesRegex(AssertionError, 'cannot measure skill'):
                run('unused', skill_mode=mode)
        with self.assertRaisesRegex(AssertionError, 'require an adapter'):
            run('unused', model='model-id')

    def test_no_skill_arm_withholds_resources_but_discovery_arm_can_read_them(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / 'skill').mkdir()
            (root / 'skill/SKILL.md').write_text('Use bounded evidence.', encoding='utf-8')
            baseline = DiscoveryTools(Path('/unused'), root, 'project-profile')
            self.assertNotIn('skill', baseline.invoke({'tool': 'list'})['files'])
            with self.assertRaisesRegex(AssertionError, 'unavailable in this arm'):
                baseline.invoke({'tool': 'read', 'path': 'skill/SKILL.md'})
            discovered = DiscoveryTools(Path('/unused'), root, 'project-profile', skill_mode='discover')
            self.assertIn('skill', discovered.invoke({'tool': 'list'})['files'])
            self.assertEqual(discovered.invoke({'tool': 'read', 'path': 'skill/SKILL.md'})['text'], 'Use bounded evidence.')

    def test_broker_rejects_scope_escape_mutation_and_undelivered_cursors(self):
        with tempfile.TemporaryDirectory() as directory:
            tools = DiscoveryTools(Path('/unused'), Path(directory).resolve(), 'project-profile')
            for request in [{'tool': 'read', 'path': '../secret'}, {'tool': 'shell'},
                            {'tool': 'mappings', 'action': 'remember'}, {'tool': 'investigate', 'argv': ['--output', 'input.jsonl']}]:
                with self.assertRaises(AssertionError): tools.invoke(request)
            tools.saved = (Path(directory) / 'evidence.json', 'hash')
            with self.assertRaisesRegex(AssertionError, 'not delivered'):
                tools.arguments({'tool': 'evidence', 'cursor': 'invented'})

    def test_answer_shape_rejects_freeform_or_ambiguous_claims(self):
        answer = {'profile_sha256': 'profile', 'ended': True, 'outcome': 'success', 'terminal_refs': ['end'], 'limitations': []}
        validate_answer(answer, None)
        for wrong in [{**answer, 'cause': 'network'}, {**answer, 'ended': False}, {**answer, 'terminal_refs': 'end'}]:
            with self.assertRaises(AssertionError): validate_answer(wrong, None)


if __name__ == '__main__': unittest.main()
