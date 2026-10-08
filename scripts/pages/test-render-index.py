#!/usr/bin/env python3
"""Public Arch instructions require a complete signed repository."""
import pathlib
import subprocess
import tempfile
import unittest

class PublicationTests(unittest.TestCase):
    def test_arch_instructions_follow_repository_availability(self):
        with tempfile.TemporaryDirectory() as directory:
            site = pathlib.Path(directory)
            apt = site / 'stable/apt'
            apt.mkdir(parents=True)
            (apt / 'InRelease').touch()
            repo = site / 'packages/stable/arch/x86_64'
            repo.mkdir(parents=True)
            def render():
                subprocess.run(['python3', 'scripts/pages/render-index.py', str(site)], check=True)
                return (site / 'packages/index.html').read_text()
            self.assertNotIn('pacman -Syu', render())
            for name in ['actionlay.db', 'key.asc', 'actionlay-bin-1.4.3-1-x86_64.pkg.tar.zst', 'actionlay-bin-1.4.3-1-x86_64.pkg.tar.zst.sig']:
                (repo / name).touch()
            self.assertNotIn('pacman -Syu', render())
            (repo / 'actionlay.db.sig').touch()
            self.assertIn('pacman -Syu actionlay-bin', render())
            self.assertIn('PackageRequired DatabaseRequired', render())

if __name__ == '__main__':
    unittest.main()
