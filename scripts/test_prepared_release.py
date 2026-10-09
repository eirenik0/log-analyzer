import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('prepared_release', Path(__file__).with_name('check-prepared-release.py'))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class PreparedReleaseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / '.claude-plugin').mkdir()
        (self.root / '.changeset').mkdir()
        self.write('Cargo.toml', '[package]\nname = "log-analyzer"\nversion = "0.3.0"\n\n[dependencies]\nother = "1"\n')
        self.write('Cargo.lock', 'version = 4\n[[package]]\nname = "other"\nversion = "1.0.0"\n[[package]]\nname = "log-analyzer"\nversion = "0.3.0"\n')
        self.write('.claude-plugin/plugin.json', json.dumps({'name': 'log-analyzer', 'version': '0.3.0'}))
        self.write('.claude-plugin/marketplace.json', json.dumps({'plugins': [{'name': 'log-analyzer', 'version': '0.3.0'}]}))
        self.write('CHANGELOG.md', '# Changelog\n\n## Unreleased\n\n## 0.3.0 (2026-10-09)\n\n- Evidence engine.\n\n## 0.2.0 (2026-02-25)\n\n- Old.\n')

    def write(self, name, text):
        (self.root / name).write_text(text, encoding='utf-8')

    def test_synchronized_release_passes(self):
        self.assertEqual(release.validate(self.root, '0.3.0'), '0.3.0')

    def test_each_mismatched_version_fails(self):
        for name in ('Cargo.toml', 'Cargo.lock', '.claude-plugin/plugin.json', '.claude-plugin/marketplace.json'):
            path = self.root / name
            original = path.read_text(encoding='utf-8')
            with self.subTest(file=name):
                self.write(name, original.replace('0.3.0', '0.2.0'))
                with self.assertRaises(ValueError): release.validate(self.root, '0.3.0')
                self.write(name, original)

    def test_unsafe_or_nonstable_version_fails(self):
        for version in ('0.3.0; touch /tmp/nope', '0.3.0-rc.1', 'v0.3.0', '00.3.0', '0.2.0'):
            with self.subTest(version=version):
                with self.assertRaises(ValueError): release.validate(self.root, version)

    def test_wrong_missing_empty_or_duplicate_release_notes_fail(self):
        for text in ('## 0.2.0 (2026-10-09)\n- Old.', '## 0.3.0 (2026-99-09)\n- Bad date.', '## 0.3.0 (2026-10-09)\n', '## 0.3.0 (2026-10-09)\n- New.\n## 0.3.0 (2026-10-09)\n- Duplicate.'):
            with self.subTest(text=text):
                self.write('CHANGELOG.md', text)
                with self.assertRaises(ValueError): release.validate(self.root, '0.3.0')

    def test_pending_changesets_fail(self):
        self.write('.changeset/pending.md', '---\ndefault: minor\n---\nFuture feature.')
        with self.assertRaises(ValueError): release.validate(self.root, '0.3.0')
