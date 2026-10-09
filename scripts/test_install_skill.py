"""Exercise standalone installation using an isolated source and destination."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

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
        self.project = self.root / 'destination project'
        self.project.mkdir()

    def install(self, cwd, *args):
        return subprocess.run(['/bin/bash' if Path('/bin/bash').exists() else 'bash',
                               str(self.source / 'scripts/install-skill.sh'), *args],
                              cwd=cwd, capture_output=True, text=True)

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

    def test_help_and_invalid_arguments(self):
        self.assertEqual(self.install(self.project, '--help').returncode, 0)
        self.assertNotEqual(self.install(self.project, '--invalid').returncode, 0)
        self.assertFalse((self.project / '.claude').exists())


class StandaloneSkillLinksTests(unittest.TestCase):
    def test_relative_markdown_links_stay_inside_standalone_bundle(self):
        skill = ROOT / '.claude/skills/analyze-logs'
        for page in skill.rglob('*.md'):
            for target in re.findall(r'\]\(([^)]+)\)', page.read_text(encoding='utf-8')):
                if '://' in target or target.startswith('#'):
                    continue
                resolved = (page.parent / target.split('#')[0]).resolve()
                with self.subTest(page=str(page), target=target):
                    self.assertTrue(resolved.is_relative_to(skill.resolve()))
                    self.assertTrue(resolved.exists())
