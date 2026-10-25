#!/usr/bin/env python3
"""Offline regression checks for license packaging and source extraction."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


bundler = module('bundle-licenses')
unpacker = module('unpack-kanata-source')


class PackagingGuards(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'dist/licenses').mkdir(parents=True)
        (self.root / 'bundle').mkdir()
        (self.root / 'Cargo.lock').write_text('reviewed dependencies')
        (self.root / 'dist/licenses/notice.txt').write_text('original copyright notice')
        (self.root / 'dist/licenses/kanata.json').write_text('{"tag":"v1.11.0"}')
        self.inventory = {
            'inputs': {'Cargo.lock': bundler.digest(self.root / 'Cargo.lock')},
            'notices': {'notice.txt': bundler.digest(self.root / 'dist/licenses/notice.txt')},
        }
        (self.root / 'dist/licenses/inventory.json').write_text(json.dumps(self.inventory))

    def refuses(self, message, version='v1.11.0'):
        argv = ['bundle-licenses.py', str(self.root / 'bundle'), '--os', 'linux', '--kanata-version', version]
        with patch.object(bundler, 'ROOT', self.root), patch.object(sys, 'argv', argv), \
                patch.object(bundler.urllib.request, 'urlopen') as network:
            with self.assertRaisesRegex(SystemExit, message):
                bundler.main()
            network.assert_not_called()
        self.assertEqual(list((self.root / 'bundle').iterdir()), [])

    def test_missing_generation_stops_before_download_or_bundle_write(self):
        (self.root / 'dist/licenses/inventory.json').unlink()
        self.refuses('Generate executable notices first')

    def test_dependency_change_stops_before_download_or_bundle_write(self):
        (self.root / 'Cargo.lock').write_text('updated dependency')
        self.refuses('inventory is stale')

    def test_windows_line_endings_do_not_invalidate_inputs(self):
        content = b'reviewed\ndependencies\n'
        self.inventory['inputs']['Cargo.lock'] = hashlib.sha256(content).hexdigest()
        (self.root / 'dist/licenses/inventory.json').write_text(json.dumps(self.inventory))
        (self.root / 'Cargo.lock').write_bytes(content.replace(b'\n', b'\r\n'))
        # Input verification succeeds and reaches the later version guard.
        self.refuses('binary version differs', 'v99.0.0')

    def test_altered_notice_stops_packaging(self):
        (self.root / 'dist/licenses/notice.txt').write_text('notice removed')
        self.refuses('notice is missing or changed')

    def test_missing_notice_stops_packaging(self):
        (self.root / 'dist/licenses/notice.txt').unlink()
        self.refuses('notice is missing or changed')

    def test_kanata_upgrade_needs_matching_source_pin(self):
        self.refuses('binary version differs', 'v99.0.0')

    def test_corrupt_cache_is_not_accepted(self):
        expected = hashlib.sha256(b'original source').hexdigest()
        (self.root / expected).write_bytes(b'changed source')
        with patch.object(bundler.urllib.request, 'urlopen') as network:
            with self.assertRaisesRegex(RuntimeError, 'Cached source checksum mismatch'):
                bundler.checked_download('https://example.org/source', expected, self.root)
            network.assert_not_called()

    def test_corrupt_download_is_not_cached(self):
        expected = hashlib.sha256(b'original source').hexdigest()
        with patch.object(bundler.urllib.request, 'urlopen', return_value=io.BytesIO(b'changed source')):
            with self.assertRaisesRegex(RuntimeError, 'Downloaded source checksum mismatch'):
                bundler.checked_download('https://example.org/source', expected, self.root)
        self.assertFalse((self.root / expected).exists())

    def archive(self, name, symlink=False):
        archive = self.root / 'source.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            entry = tarfile.TarInfo(name)
            if symlink:
                entry.type = tarfile.SYMTYPE
                entry.linkname = '../outside'
                tar.addfile(entry)
            else:
                entry.size = 6
                tar.addfile(entry, io.BytesIO(b'source'))
        return archive

    def test_traversal_and_links_are_rejected(self):
        for name, link in [('../escaped', False), ('/absolute', False), ('..\\escaped', False), ('link', True)]:
            with self.subTest(name=name):
                with self.assertRaisesRegex(ValueError, 'Unsafe source archive member'):
                    unpacker.extract(self.archive(name, link), self.root / 'output')
        self.assertFalse((self.root / 'output').exists())

    def test_regular_source_extracts(self):
        unpacker.extract(self.archive('crate/src/lib.rs'), self.root / 'output')
        self.assertEqual((self.root / 'output/crate/src/lib.rs').read_bytes(), b'source')


if __name__ == '__main__':
    unittest.main()
