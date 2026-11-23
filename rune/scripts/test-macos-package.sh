#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bundle="${1:-artifacts/Pickle Rune.app}"
executable="$bundle/Contents/MacOS/PickleRune"
host="$bundle/Contents/MacOS/pickle-browser-host"
for file in "$executable" "$host" "$bundle/Contents/Info.plist" \
  "$bundle/Contents/Resources/THIRD_PARTY_NOTICES.txt" \
  "$bundle/Contents/Resources/dependency-inventory.json" \
  "$bundle/Contents/Resources/browser-extension/manifest.json"; do
  if [[ ! -s "$file" ]]; then printf 'Missing packaged file: %s\n' "$file" >&2; exit 1; fi
done
codesign --verify --strict --verbose=2 "$bundle"
codesign --verify --strict --verbose=2 "$host"
mkdir -p artifacts
"$executable" --version > artifacts/macos-version.txt
"$executable" --capabilities > artifacts/macos-capabilities.json
python3 - "$bundle" <<'PY'
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess
import sys

bundle = Path(sys.argv[1]).resolve()
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], text=True))
version = next(p['version'] for p in metadata['packages'] if p['name'] == 'pickle-app')
observed = Path('artifacts/macos-version.txt').read_text().strip()
if observed != f'Pickle Rune {version}':
    raise SystemExit(f'Unexpected installed version: {observed!r}')
with (bundle / 'Contents/Info.plist').open('rb') as stream:
    info = plistlib.load(stream)
if info.get('CFBundleShortVersionString') != version:
    raise SystemExit('The bundle version and executable version differ.')
capabilities = json.loads(Path('artifacts/macos-capabilities.json').read_text())
if not capabilities.get('selection') or not capabilities.get('menu_bar'):
    raise SystemExit('The packaged application is missing core Mac capabilities.')
binaries = []
for name in ('PickleRune', 'pickle-browser-host'):
    path = bundle / 'Contents/MacOS' / name
    listing = subprocess.check_output(['otool', '-L', str(path)], text=True)
    dependencies = [line.strip().split(' (', 1)[0] for line in listing.splitlines()[1:] if line.strip()]
    unexpected = [dep for dep in dependencies
                  if not dep.startswith(('/System/Library/', '/usr/lib/'))]
    if unexpected:
        raise SystemExit(f'{name} requires unbundled non-system libraries: {unexpected}')
    binaries.append({'name': name, 'bytes': path.stat().st_size,
                     'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                     'dependencies': dependencies})
report = {'test': 'macos-package-inspection', 'passed': True,
          'version': version, 'binaries': binaries,
          'scope': 'ad-hoc signature integrity, payload, CLI loading and system library dependencies; no native UI or release signing acceptance'}
Path('artifacts/macos-package-report.json').write_text(json.dumps(report, indent=2) + '\n')
PY
printf 'PASS: Mac package payload, signature integrity, CLI version/capabilities and system-only loader dependencies.\n'
