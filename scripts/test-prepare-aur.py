#!/usr/bin/env python3
"""Regression checks for the recipe generation and checksum boundary."""
import hashlib
import importlib.util
import pathlib
import tempfile
import sys

sys.dont_write_bytecode = True
import unittest

spec = importlib.util.spec_from_file_location('prepare_aur', pathlib.Path(__file__).with_name('prepare-aur.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class RecipeTests(unittest.TestCase):
    def test_published_checksum_and_exact_versioned_archive(self):
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            archive = root / 'actionlay-1.5.0-linux-x64.tar.gz'
            archive.write_bytes(b'fixture archive')
            checksums = root / 'SHA256SUMS'
            sha = hashlib.sha256(archive.read_bytes()).hexdigest()
            checksums.write_text(f'{sha}  {archive.name}\n')
            module.prepare('1.5.0', archive, root / 'recipe', checksums)
            recipe = (root / 'recipe/PKGBUILD').read_text()
            self.assertIn(f"sha256sums=('{sha}'", recipe)
            self.assertIn('pkgver=1.5.0', recipe)
            self.assertNotIn('@VERSION@', recipe)
            self.assertIn('X-ActionLay-Managed=true', recipe)
            checksums.write_text(f'{"0" * 64}  {archive.name}\n')
            with self.assertRaises(ValueError):
                module.prepare('1.5.0', archive, root / 'bad', checksums)
            self.assertFalse((root / 'bad').exists())

    def test_development_versions_and_wrong_asset_names_are_rejected(self):
        for version in ['nightly', '1.5.0-dev', '1.5.0+git1', '1.5.0; touch nope', '01.5.0']:
            with self.assertRaises(ValueError):
                module.prepare(version, pathlib.Path('missing'), pathlib.Path('unused'))
        with self.assertRaises(ValueError):
            module.prepare('1.5.0', pathlib.Path('actionlay-1.4.2-linux-x64.tar.gz'), pathlib.Path('unused'))


if __name__ == '__main__':
    unittest.main()
