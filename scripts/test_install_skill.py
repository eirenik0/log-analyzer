"""Exercise standalone installation using an isolated source and destination."""
import os
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipIf(os.name == 'nt' or not shutil.which('bash'), 'The installer is a Bash script')
class SkillInstallationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='skill installation ')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / 'source repository'
        (self.source / 'scripts').mkdir(parents=True)
        shutil.copy2(ROOT / 'scripts/install-skill.sh', self.source / 'scripts/install-skill.sh')
        self.skill = self.source / '.claude/skills/analyze-logs'
        shutil.copytree(ROOT / '.claude/skills/analyze-logs', self.skill)
        self.portable = self.source / '.agents/skills/analyze-logs'
        shutil.copytree(ROOT / '.agents/skills/analyze-logs', self.portable)
        self.home = self.root / 'user home'
        self.home.mkdir()
        self.project = self.root / 'destination project'
        self.project.mkdir()

    def install(self, cwd, *args, path=None):
        return subprocess.run(['/bin/bash' if Path('/bin/bash').exists() else 'bash',
                               str(self.source / 'scripts/install-skill.sh'), *args],
                              cwd=cwd, capture_output=True, text=True,
                              env={**os.environ, 'HOME': str(self.home), **({'PATH': str(path)} if path else {})})

    def test_source_checkout_is_successful_noop(self):
        before = {p.relative_to(self.skill): p.read_bytes() for p in self.skill.rglob('*') if p.is_file()}
        for _ in range(2):
            result = self.install(self.source)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        after = {p.relative_to(self.skill): p.read_bytes() for p in self.skill.rglob('*') if p.is_file()}
        self.assertEqual(before, after)

    def test_current_project_receives_complete_skill_and_repeat_updates_it(self):
        (self.skill / '.fixture').write_text('hidden support file', encoding='utf-8')
        target = self.project / '.claude/skills/analyze-logs'
        for iteration in range(2):
            expected = f'updated content {iteration}'
            (self.skill / 'reference.md').write_text(expected, encoding='utf-8')
            result = self.install(self.project)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((target / 'reference.md').read_text(encoding='utf-8'), expected)
            self.assertEqual((target / '.fixture').read_text(encoding='utf-8'), 'hidden support file')
            self.assertEqual((target / 'templates/eyes.toml').read_bytes(), (self.skill / 'templates/eyes.toml').read_bytes())
            self.assertFalse((target / 'analyze-logs').exists())

    def test_nested_working_directory_is_rejected_without_partial_copies(self):
        before = sorted(str(p.relative_to(self.skill)) for p in self.skill.rglob('*'))
        for cwd in (self.skill, self.skill / 'examples'):
            with self.subTest(cwd=str(cwd)):
                result = self.install(cwd)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('overlaps the skill source', result.stderr)
                self.assertEqual(sorted(str(p.relative_to(self.skill)) for p in self.skill.rglob('*')), before)

    def test_symlinked_destination_inside_source_is_rejected(self):
        (self.project / '.claude').symlink_to(self.skill, target_is_directory=True)
        result = self.install(self.project)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('overlaps the skill source', result.stderr)
        self.assertFalse((self.skill / 'skills').exists())

    def test_destination_ancestor_of_source_is_rejected(self):
        target = self.project / '.claude/skills/analyze-logs'
        target.parent.mkdir(parents=True)
        target.symlink_to(self.skill.parent, target_is_directory=True)
        result = self.install(self.project)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('overlaps the skill source', result.stderr)
        self.assertFalse((self.skill.parent / 'SKILL.md').exists())

    def test_each_host_and_scope_installs_a_self_contained_bundle(self):
        for host in ('codex', 'pi', 'claude'):
            for scope in ('project', 'user'):
                with self.subTest(host=host, scope=scope):
                    result = self.install(self.project, '--host', host, '--scope', scope)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    base = self.project if scope == 'project' else self.home
                    folder = '.claude' if host == 'claude' else '.agents'
                    target = base / folder / 'skills/analyze-logs'
                    source = self.skill if host == 'claude' else self.portable
                    for page in source.rglob('*'):
                        if page.is_file():
                            self.assertEqual((target / page.relative_to(source)).read_bytes(), page.read_bytes())
                    assert_links(self, target)
                    self.assertFalse(any(p.is_symlink() for p in target.rglob('*')))

    def test_portable_repeat_install_and_source_noop(self):
        for host in ('codex', 'pi'):
            for _ in range(2):
                result = self.install(self.source, '--host', host)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for iteration in range(2):
                (self.portable / 'reference.md').write_text(str(iteration))
                result = self.install(self.project, '--host', host)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((self.project / '.agents/skills/analyze-logs/reference.md').read_text(), str(iteration))

    def test_global_aliases_and_conflicting_scope(self):
        for flag in ('--global', '-g'):
            for host in ('claude', 'codex', 'pi'):
                result = self.install(self.project, flag, '--host', host)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                folder = '.claude' if host == 'claude' else '.agents'
                self.assertTrue((self.home / folder / 'skills/analyze-logs/SKILL.md').is_file())
        for args in (('--scope', 'project', '--global'), ('-g', '--scope', 'project')):
            result = self.install(self.project, *args)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('conflicting scope', result.stderr)

    def test_portable_overlaps_are_rejected_before_copy(self):
        for host in ('claude', 'codex', 'pi'):
            for cwd in (self.portable, self.portable / 'examples', self.skill):
                result = self.install(cwd, '--host', host)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn('overlaps the skill source', result.stderr)

    def test_existing_directory_symlinks_are_resolved(self):
        actual = self.root / 'actual target'
        actual.mkdir()
        (self.project / '.agents').symlink_to(actual, target_is_directory=True)
        result = self.install(self.project, '--host', 'codex')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        assert_links(self, actual / 'skills/analyze-logs')

    def test_portable_symlink_overlap_and_dangling_destination(self):
        (self.project / '.agents').symlink_to(self.portable, target_is_directory=True)
        result = self.install(self.project, '--host', 'pi')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('overlaps the skill source', result.stderr)
        (self.project / '.agents').unlink()
        (self.project / '.agents').symlink_to(self.root / 'missing')
        result = self.install(self.project, '--host', 'pi')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Not an accessible directory', result.stderr)

    def test_destination_child_symlink_cannot_overwrite_external_file(self):
        target = self.project / '.agents/skills/analyze-logs'
        target.mkdir(parents=True)
        outside = self.root / 'outside.md'
        outside.write_text('keep me')
        (target / 'reference.md').symlink_to(outside)
        result = self.install(self.project, '--host', 'codex')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('destination contains a symlink', result.stderr)
        self.assertEqual(outside.read_text(), 'keep me')

    def test_missing_executable_warns_and_keeps_installation(self):
        commands = self.root / 'isolated PATH'
        commands.mkdir()
        for command in ('dirname', 'basename', 'mkdir', 'cp', 'find'):
            (commands / command).symlink_to(shutil.which(command))
        result = self.install(self.project, '--host', 'pi', path=commands)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('binary not found in PATH', result.stdout)
        self.assertIn('cargo install log-analyzer --locked', result.stdout)
        self.assertTrue((self.project / '.agents/skills/analyze-logs/SKILL.md').exists())
        executable = commands / 'log-analyzer'
        executable.write_text('#!/bin/sh\nexit 0\n')
        executable.chmod(0o755)
        result = self.install(self.project, '--host', 'codex', path=commands)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(str(executable), result.stdout)
        self.assertIn('does not prove compatibility', result.stdout)

    def test_help_and_invalid_arguments(self):
        self.assertEqual(self.install(self.project, '--help').returncode, 0)
        for args in (('--invalid',), ('--host',), ('--scope',), ('--host', 'unknown'), ('--scope', 'global')):
            self.assertNotEqual(self.install(self.project, *args).returncode, 0)
        self.assertFalse((self.project / '.claude').exists())


def assert_links(test, skill):
    for page in skill.rglob('*.md'):
        for target in re.findall(r'\]\(([^)]+)\)', page.read_text(encoding='utf-8')):
            if '://' in target or target.startswith('#'):
                continue
            resolved = (page.parent / target.split('#')[0]).resolve()
            with test.subTest(page=str(page), target=target):
                test.assertTrue(resolved.is_relative_to(skill.resolve()))
                test.assertTrue(resolved.exists())


class StandaloneSkillLinksTests(unittest.TestCase):
    def test_relative_links_stay_inside_each_bundle(self):
        for folder in ('.agents', '.claude'):
            assert_links(self, ROOT / folder / 'skills/analyze-logs')

    def test_bundle_readme_links_target_existing_headings(self):
        headings = re.findall(r'^#{1,6} (.+)$', (ROOT / 'README.md').read_text(encoding='utf-8'), re.MULTILINE)
        anchors = {re.sub(r'[^\w -]', '', title.lower()).replace(' ', '-') for title in headings}
        for folder in ('.agents', '.claude'):
            for page in (ROOT / folder / 'skills/analyze-logs').rglob('*.md'):
                for anchor in re.findall(r'\]\(https://github.com/eirenik0/log-analyzer#([^)]+)\)', page.read_text(encoding='utf-8')):
                    with self.subTest(page=str(page), anchor=anchor):
                        self.assertIn(anchor, anchors)

    def test_claude_generated_bundle_has_no_drift(self):
        result = subprocess.run([os.sys.executable, str(ROOT / 'scripts/sync-skills.py'), '--check'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_generated_crlf_checkout_passes_but_content_drift_still_fails(self):
        spec = importlib.util.spec_from_file_location('sync_skills', ROOT / 'scripts/sync-skills.py')
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        with tempfile.TemporaryDirectory(prefix='skill line endings ') as directory:
            root = Path(directory)
            source, target = root / 'canonical', root / 'claude'
            shutil.copytree(generator.SOURCE, source)
            with patch.object(generator, 'SOURCE', source), patch.object(generator, 'TARGET', target):
                generator.sync()
                for name in ('SKILL.md', 'workflow.md'):
                    page = target / name
                    page.write_bytes(page.read_bytes().replace(b'\n', b'\r\n'))
                generator.sync(check=True)
                page = target / 'workflow.md'
                page.write_bytes(page.read_bytes() + b'Unexpected workflow drift\r\n')
                with self.assertRaisesRegex(SystemExit, 'workflow.md'):
                    generator.sync(check=True)
                generator.sync()
                generator.sync(check=True)

    def test_portable_metadata_and_pi_resource_paths(self):
        skill = ROOT / '.agents/skills/analyze-logs'
        front = (skill / 'SKILL.md').read_text().split('---', 2)[1]
        metadata = dict(line.split(': ', 1) for line in front.strip().splitlines())
        self.assertEqual(set(metadata), {'name', 'description'})
        self.assertEqual(metadata['name'], skill.name)
        self.assertLessEqual(len(metadata['description']), 1024)
        manifest = json.loads((ROOT / 'package.json').read_text())
        plugin = json.loads((ROOT / '.claude-plugin/plugin.json').read_text())
        self.assertEqual(plugin['skills'], ['./.claude/skills'])
        for resource in plugin['skills']:
            self.assertTrue((ROOT / resource / 'analyze-logs/SKILL.md').is_file())
        self.assertEqual(set(manifest['pi']), {'skills'})
        self.assertEqual(len(manifest['pi']['skills']), 1)
        for resource in manifest['pi']['skills']:
            resolved = (ROOT / resource).resolve()
            self.assertTrue(resolved.is_relative_to(ROOT))
            self.assertEqual(resolved, skill)
            self.assertTrue((resolved / 'SKILL.md').is_file())
        # The optional metadata uses quoted YAML scalar strings; JSON parses that
        # subset without adding a Python YAML runtime dependency to CI.
        display = (skill / 'agents/openai.yaml').read_text().splitlines()
        self.assertEqual(display[0], 'interface:')
        values = {key.strip(): json.loads(value) for key, value in (line.split(': ', 1) for line in display[1:])}
        self.assertTrue(values['display_name'])
        self.assertTrue(25 <= len(values['short_description']) <= 64)
        self.assertIn('$analyze-logs', values['default_prompt'])
