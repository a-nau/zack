#!/usr/bin/env python3
"""Attach checked notices and matching source archives before a bundle is zipped."""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import shutil
import urllib.request

ROOT = Path(__file__).resolve().parent.parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checked_download(url, expected, cache, crate=None):
    path = cache / expected
    if path.exists():
        if digest(path) != expected:
            raise RuntimeError('Cached source checksum mismatch: ' + path.name)
        return path
    if crate:
        cargo = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')))
        for candidate in (cargo / 'registry/cache').glob('*/' + crate):
            if digest(candidate) == expected:
                shutil.copyfile(candidate, path)
                return path
    with urllib.request.urlopen(url, timeout=90) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != expected:
        raise RuntimeError('Downloaded source checksum mismatch: ' + url)
    path.write_bytes(data)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    parser.add_argument('--os', required=True, choices=['macos', 'linux', 'windows'])
    parser.add_argument('--kanata-version', default='v1.11.0')
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    if not bundle.is_dir():
        raise SystemExit('Bundle directory must already exist.')
    generated = ROOT / 'dist/licenses'
    if not (generated / 'inventory.json').is_file():
        raise SystemExit('Generate executable notices first: python3 scripts/generate-license-notices.py (see docs/releasing.md).')
    inventory = json.loads((generated / 'inventory.json').read_text())
    # Check everything before adding files to the bundle. A Cargo change or a
    # missing/modified notice requires regeneration, never a silent omission.
    for name, expected in inventory['inputs'].items():
        path = ROOT / name
        if not path.is_file() or hashlib.sha256(path.read_bytes().replace(b'\r\n', b'\n')).hexdigest() != expected:
            raise SystemExit('License inventory is stale: ' + name + '; regenerate the notices.')
    for name, expected in inventory['notices'].items():
        if not (generated / name).is_file() or digest(generated / name) != expected:
            raise SystemExit('License notice is missing or changed: ' + name)
    pins = json.loads((generated / 'kanata.json').read_text())
    if args.os != 'macos' and args.kanata_version != pins['tag']:
        raise SystemExit('Kanata binary version differs from its reviewed source/license pin.')
    cache = ROOT / 'dist/.license-source-cache'; cache.mkdir(parents=True, exist_ok=True)
    selected = [s for s in inventory['sources'] if s['group'] == 'zack' or args.os != 'macos']
    # A crate shared by Zack and Kanata needs only one source archive.
    selected = list({(s['name'], s['version']): s for s in selected}.values())
    def fetch(item):
        filename = item['name'] + '-' + item['version'] + '.crate'
        url = 'https://static.crates.io/crates/' + item['name'] + '/' + filename
        return item, filename, checked_download(url, item['sha256'], cache, filename)
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        fetched = list(pool.map(fetch, selected))
    kanata_archive = None
    if args.os != 'macos':
        kanata_archive = checked_download(pins['source_url'], pins['source_sha256'], cache)
    for name in ['LICENSE', 'THIRD-PARTY-NOTICES.md']:
        shutil.copyfile(ROOT / name, bundle / name)
    notices = bundle / 'licenses'; notices.mkdir(exist_ok=True)
    for name in ['zack.html', 'inventory.json', 'README.md', 'SOURCES.md']:
        shutil.copyfile(generated / name, notices / name)
    for directory in ['fonts'] + (['upstream'] if args.os != 'macos' else []):
        shutil.copytree(generated / directory, notices / directory, dirs_exist_ok=True)
    sources = bundle / 'sources'; (sources / 'crates').mkdir(parents=True, exist_ok=True)
    source_index = []
    for item, name, cached in fetched:
        shutil.copyfile(cached, sources / 'crates' / name)
        source_index.append({'file': 'crates/' + name, 'sha256': item['sha256'], 'name': item['name'], 'version': item['version']})
    if kanata_archive:
        for name in ['kanata-linux.html', 'kanata-windows.html', 'kanata.json']:
            shutil.copyfile(generated / name, notices / name)
        shutil.copyfile(kanata_archive, sources / 'kanata-source.tar.gz')
        source_index.append({'file': 'kanata-source.tar.gz', 'sha256': pins['source_sha256'], 'commit': pins['commit']})
        shutil.copyfile(ROOT / 'scripts/unpack-kanata-source.py', sources / 'unpack-kanata-source.py')
    (sources / 'manifest.json').write_text(json.dumps(source_index, indent=2, sort_keys=True)+'\n')
    shutil.copyfile(generated / 'SOURCES.md', sources / 'README.md')
    files = {p.relative_to(bundle).as_posix(): digest(p) for p in sorted(bundle.rglob('*')) if p.is_file() and p.name != 'bundle-manifest.json'}
    (bundle / 'bundle-manifest.json').write_text(json.dumps({'platform': args.os, 'files': files}, indent=2, sort_keys=True)+'\n')
    print('Attached license notices and',len(source_index),'verified source archives.')


if __name__ == '__main__':
    main()
