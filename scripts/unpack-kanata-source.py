#!/usr/bin/env python3
"""Unpack the included Kanata and dependency sources into an offline Cargo tree."""
import hashlib
import json
from pathlib import Path
import sys
import tarfile


def extract(archive, destination):
    with tarfile.open(archive) as tar:
        for member in tar.getmembers():
            path = Path(member.name)
            if path.is_absolute() or '..' in path.parts or '\\' in member.name or not (member.isfile() or member.isdir()):
                raise ValueError('Unsafe source archive member: ' + member.name)
        tar.extractall(destination)


def main():
    source = Path(__file__).resolve().parent
    if len(sys.argv) != 2:
        raise SystemExit('Usage: python3 unpack-kanata-source.py NEW_OUTPUT_DIRECTORY')
    output = Path(sys.argv[1]).resolve()
    if output.exists():
        raise SystemExit('Choose a new output directory; existing data will not be overwritten.')
    records = json.loads((source / 'manifest.json').read_text())
    for entry in records:
        relative = Path(entry['file'])
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError('Invalid source-manifest path')
        if hashlib.sha256((source / relative).read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError('Source checksum mismatch: ' + entry['file'])
    output.mkdir(parents=True)
    extract(source / 'kanata-source.tar.gz', output)
    project = next(output.glob('kanata-*'))
    vendor = project / 'vendor'; vendor.mkdir()
    for entry in records:
        if not entry['file'].endswith('.crate'):
            continue
        extract(source / entry['file'], vendor)
        crate = vendor / (entry['name'] + '-' + entry['version'])
        files = {p.relative_to(crate).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in sorted(crate.rglob('*')) if p.is_file() and p.name != '.cargo-checksum.json'}
        (crate / '.cargo-checksum.json').write_text(json.dumps({'files': files, 'package': entry['sha256']})+'\n')
    (project / '.cargo').mkdir(exist_ok=True)
    config = project / '.cargo/config.toml'
    with config.open('a') as handle:
        handle.write('\n[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n')
    print('Sources ready:', project)
    print('See sources/README.md for the original Linux and Windows build commands.')


if __name__ == '__main__':
    main()
