#!/usr/bin/env python3
"""Regenerate bundle notices with Python 3.11+ and cargo-about 0.9.2."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import re
from runpy import run_path
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / 'scripts/license-config'
LICENSES = ROOT / 'dist/licenses'
checked_download = run_path(str(ROOT / 'scripts/bundle-licenses.py'))['checked_download']
extract = run_path(str(ROOT / 'scripts/unpack-kanata-source.py'))['extract']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def locked_packages(path):
    return {(p['name'], p['version']): p for p in tomllib.loads(path.read_text())['package']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo-about', default='cargo-about')
    parser.add_argument('--kanata-source', type=Path, help='Use an existing pinned source tree instead of downloading it')
    args = parser.parse_args()
    version = subprocess.check_output([args.cargo_about, '--version'], text=True).strip()
    if version != 'cargo-about 0.9.2':
        raise SystemExit('Use the pinned cargo-about 0.9.2 version.')
    pins = json.loads((CONFIG / 'kanata.json').read_text())
    cache = ROOT / 'dist/.license-source-cache'; cache.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='zack-notices-') as tmp:
        if args.kanata_source:
            source = args.kanata_source.resolve()
        else:
            archive = checked_download(pins['source_url'], pins['source_sha256'], cache)
            extract(archive, Path(tmp))
            source = Path(tmp) / ('kanata-' + pins['commit'])
        generate(args, version, pins, source, cache)


def generate(args, version, pins, source, cache):
    # A failed generation must not leave an old inventory usable for packaging.
    if LICENSES.exists():
        shutil.rmtree(LICENSES)
    LICENSES.mkdir(parents=True)
    for name, key in [('Cargo.toml', 'cargo_toml_sha256'), ('Cargo.lock', 'cargo_lock_sha256')]:
        if sha(source / name) != pins[key]:
            raise SystemExit('Kanata source does not match the reviewed pin: ' + name)
    upstream = json.loads((CONFIG / 'upstream-notices.json').read_text())
    resolutions = {(p['name'], p['version']): p for p in upstream}
    if any('error' in row for row in upstream):
        raise SystemExit('Resolve upstream notice errors before generating.')
    # A deliberate union covers the four Zack build targets. Kanata reports
    # use the exact features/targets of its two selected upstream binaries.
    jobs = [
        ('zack', ROOT / 'launcher', []),
        ('kanata-linux', source, ['--target', 'x86_64-unknown-linux-gnu']),
        ('kanata-windows', source, ['--target', 'x86_64-pc-windows-msvc', '--features',
         'gui,win_manifest,win_sendinput_send_scancodes,win_llhook_read_scancodes']),
    ]
    inventory = {'generator': version, 'zack_targets': tomllib.loads((CONFIG / 'about.toml').read_text())['targets'],
                 # Input checks allow Git's Windows CRLF checkout conversion.
                 'inputs': {p: hashlib.sha256((ROOT / p).read_bytes().replace(b'\r\n', b'\n')).hexdigest() for p in [
                     'launcher/Cargo.toml', 'launcher/Cargo.lock', 'launcher/.cargo/config.toml',
                     'scripts/license-config/about.toml', 'scripts/license-config/kanata.json',
                     'scripts/license-config/upstream-notices.json', 'scripts/license-config/SOURCES.md',
                     'LICENSE', 'THIRD-PARTY-NOTICES.md', 'scripts/bundle-licenses.py',
                     'scripts/unpack-kanata-source.py',
                     'scripts/generate-license-notices.py', 'scripts/make-bundle.sh',
                     'scripts/build-macos.sh', 'scripts/build-in-container.sh',
                     'scripts/build-in-container.ps1', 'scripts/docker/Dockerfile']},
                 'components': [], 'sources': [], 'notices': {}}
    with tempfile.TemporaryDirectory(prefix='zack-licenses-') as tmp:
        temporary = Path(tmp)
        config = temporary / 'kanata-about.toml'
        config.write_text((CONFIG / 'about.toml').read_text().replace('"MPL-2.0",', '"MPL-2.0", "LGPL-3.0-only",'))
        for label, project, flags in jobs:
            output = temporary / (label + '.json')
            subprocess.run([args.cargo_about, 'generate', '--locked', '--fail', '--format', 'json',
                            '--config', str(CONFIG / 'about.toml' if label == 'zack' else config),
                            '--manifest-path', str(project / 'Cargo.toml'), '--output-file', str(output), *flags], check=True)
            report = json.loads(output.read_text())
            packages = locked_packages(project / 'Cargo.lock')
            texts = {}
            def notice(text, used_by, origin):
                digest = hashlib.sha256(text.encode()).hexdigest()
                item = texts.setdefault(digest, {'text': text, 'used_by': set(), 'origins': set()})
                item['used_by'].update(used_by); item['origins'].add(origin)
            for entry in report['licenses']:
                # Actual files are retained verbatim. Standard terms are clearly
                # marked and never acquire an invented copyright holder/year.
                text = entry['text']
                if not entry.get('source_path'):
                    text = text.replace('Copyright (c) <year> <copyright holders>\n\n', '')
                notice(text, [u['crate']['name'] + ' ' + u['crate']['version'] for u in entry['used_by']],
                       'Packaged license text' if entry.get('source_path') else 'Standard ' + entry['id'] + ' terms; see original notices and package authors below')
            for entry in report['crates']:
                package = entry['package']; key = (package['name'], package['version']); named = ' '.join(key)
                locked = packages[key]
                component = {'group': label, 'name': key[0], 'version': key[1], 'license': entry['license'],
                             'authors': package.get('authors', []), 'repository': package.get('repository'),
                             'checksum': locked.get('checksum')}
                inventory['components'].append(component)
                base = Path(package['manifest_path']).parent
                if label == 'zack' and key[0] == 'epaint_default_fonts':
                    fonts = LICENSES / 'fonts'; fonts.mkdir(exist_ok=True)
                    for name in ['Hack-Regular.txt', 'OFL.txt', 'UFL.txt', 'emoji-icon-font-mit-license.txt']:
                        shutil.copyfile(base / 'fonts' / name, fonts / name)
                candidates = [p for p in base.iterdir() if p.is_file() and re.match(r'(?i)^(licen[cs]e|copying|copyright|notice|unlicense)', p.name)]
                for p in candidates:
                    notice(p.read_text(), [named], named + '/' + p.name)
                resolution = resolutions.get(key)
                if resolution:
                    for f in resolution.get('files', []):
                        url = f['url']
                        if url.startswith('https://github.com/'):
                            url = url.replace('https://github.com/', 'https://raw.githubusercontent.com/', 1).replace('/blob/', '/', 1)
                        p = checked_download(url, f['sha256'], cache)
                        notice(p.read_text(), [named], f['url'])
                    if resolution.get('resolution'):
                        component['notice_note'] = resolution['resolution']
                if not candidates and not resolution and key[0] != 'zack':
                    # cargo-about may have harvested a nested original file.
                    covered = any(e.get('source_path') and any(u['crate']['name'] == key[0] and u['crate']['version'] == key[1] for u in e['used_by']) for e in report['licenses'])
                    if not covered: raise SystemExit('Review missing original notices for ' + named)
                if label == 'zack' and ('MPL-2.0' in entry['license'] or (resolution and resolution.get('resolution'))):
                    inventory['sources'].append({'group': 'zack', 'name': key[0], 'version': key[1], 'sha256': locked['checksum']})
            if label == 'kanata-linux':
                # All locked registry sources are included, including other
                # targets, so Cargo can resolve the entire graph offline.
                for key, package in sorted(packages.items()):
                    if package.get('source'):
                        if not package['source'].startswith('registry+https://github.com/rust-lang/crates.io-index'):
                            raise SystemExit('Review non-crates.io source: ' + key[0])
                        inventory['sources'].append({'group': 'kanata', 'name': key[0], 'version': key[1], 'sha256': package['checksum']})
            parts = ['<!doctype html><html lang="en"><meta charset="utf-8"><title>' + label + ' licenses</title>',
                     '<style>body{max-width:1000px;margin:40px auto;font:16px system-ui;padding:0 20px}pre{white-space:pre-wrap}td,th{padding:8px;text-align:left;vertical-align:top}table{border-collapse:collapse}tr{border-bottom:1px solid #ddd}</style>',
                     '<h1>' + label + ': third-party licenses</h1><p>Generated from the locked source packages. Additional font and upstream notices are included beside this document.</p><table><tr><th>Package</th><th>Declared license</th><th>Package authors</th></tr>']
            for c in inventory['components']:
                if c['group'] == label:
                    parts.append('<tr><td>' + html.escape(c['name']+' '+c['version']) + '</td><td>'+html.escape(c['license'])+'</td><td>'+html.escape(', '.join(c['authors']))+'</td></tr>')
            parts.append('</table>')
            for item in texts.values():
                parts.append('<h2>'+html.escape(', '.join(sorted(item['used_by'])))+'</h2><p>'+html.escape('; '.join(sorted(item['origins'])))+'</p><pre>'+html.escape(item['text'])+'</pre>')
            parts.append('</html>')
            destination = LICENSES / (label + '.html'); destination.write_text('\n'.join(parts)+'\n')
    upstream = LICENSES / 'upstream'; upstream.mkdir(exist_ok=True)
    for destination, original in {
        'Kanata-LGPL-3.0.txt': 'LICENSE',
        'Keyberon-MIT.txt': 'keyberon/LICENSE',
        'Kanata-GUI-MIT.txt': 'src/gui/win_nwg_ext/license-MIT',
        'Native-Windows-GUI-MIT.txt': 'src/gui/win_nwg_ext/license-nwg-MIT',
        'Kanata-icons.txt': 'cfg_samples/tray-icon/license_icons.txt',
    }.items():
        shutil.copyfile(source / original, upstream / destination)
    gpl = pins['gpl_notice']
    shutil.copyfile(checked_download(gpl['url'], gpl['sha256'], cache), upstream / 'GPL-3.0.txt')
    shutil.copyfile(CONFIG / 'kanata.json', LICENSES / 'kanata.json')
    shutil.copyfile(CONFIG / 'SOURCES.md', LICENSES / 'SOURCES.md')
    (LICENSES / 'README.md').write_text(
        '# Generated executable notices\n\n'
        'Open `zack.html` for Zack dependency licenses and original notices.\n'
        'Windows/Linux bundles also include the Kanata reports and sources.\n'
        '`fonts/` and `upstream/` preserve additional original notices.\n'
        '`inventory.json` records package versions, authors, checksums and inputs.\n'
        'See `../sources/README.md` in a bundle for source rebuild instructions.\n'
        'See `docs/releasing.md` in the source repository for report coverage,\n'
        'known notice gaps and regeneration. These files are generated; do not edit them.\n')
    for p in sorted(LICENSES.rglob('*')):
        if p.is_file() and p.name != 'inventory.json':
            inventory['notices'][str(p.relative_to(LICENSES))] = sha(p)
    (LICENSES / 'inventory.json').write_text(json.dumps(inventory, indent=2, sort_keys=True)+'\n')
    print('Generated notices for',len(inventory['components']),'package/target entries.')


if __name__ == '__main__':
    main()
