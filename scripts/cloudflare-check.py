#!/usr/bin/env python3
"""Explicit, bounded CLI smoke check. Never prints/saves credentials or raw error bodies."""
import argparse, json, os, subprocess, urllib.request, urllib.error, time
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('--account', required=True)
p.add_argument('--live', action='store_true', help='Make two small, potentially billable inference calls on bundled sample text.')
p.add_argument('--import-keychain', action='store_true', help='Import the current Wrangler token into the built app Keychain; OAuth tokens expire.')
a = p.parse_args()
if len(a.account) != 32 or any(c not in '0123456789abcdef' for c in a.account.lower()): p.error('Invalid account ID')
result = subprocess.run(['npx', '--yes', 'wrangler@4.134.0', 'auth', 'token', '--json'], capture_output=True, text=True)
if result.returncode: raise SystemExit('Wrangler authentication failed. Run npx wrangler login.')
try:
    auth = json.loads(result.stdout)
except Exception: raise SystemExit('Wrangler returned an unsupported credential format (not printed).')
token = auth.get('token') if isinstance(auth, dict) else None
if not token:
    raise SystemExit('No token field in Wrangler JSON. Available field names: ' + ', '.join(auth.keys()))
if a.import_keychain:
    app = Path(__file__).resolve().parent.parent / 'dist/Pickle.app/Contents/MacOS/Pickle'
    imported = subprocess.run([str(app), '--import-cli-credential', a.account], input=token, text=True, capture_output=True)
    print('Keychain import succeeded.' if imported.returncode == 0 else 'Keychain import failed.')
if not a.live: raise SystemExit(0)
report = {'date': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), 'wrangler': '4.134.0', 'calls': [], 'note': 'Fixed public fixtures only; no user selection. No provider credentials logged.'}
calls = [
    ('jev', '/ai/run', {'model': 'typesafe/jev', 'input': {'state': {'source': 'The treatment may reduce symptoms in some patients, but evidence is limited.', 'candidate': 'The treatment reduces symptoms.'}, 'questions': {'lost_uncertainty': {'type': 'noul', 'instructions': 'Does candidate remove uncertainty or evidence limitations present in source?'}}}}),
    ('llama', '/ai/run/@cf/meta/llama-3.3-70b-instruct-fp8-fast', {'messages': [{'role': 'system', 'content': 'Simplify faithfully. Preserve uncertainty, scope and evidence limitations. Return only one sentence.'}, {'role': 'user', 'content': 'The treatment may reduce symptoms in some patients, but the evidence remains limited.'}], 'max_tokens': 100, 'stream': False})
]
for name, path, body in calls:
    req = urllib.request.Request('https://api.cloudflare.com/client/v4/accounts/' + a.account + path, data=json.dumps(body).encode(), headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
    start = time.monotonic()
    entry = {'provider': name}
    try:
        with urllib.request.urlopen(req, timeout=40) as response:
            data = json.load(response); entry['http_status'] = response.status
        output = data.get('result', data)
        entry['success'] = data.get('success', True)
        if isinstance(output, dict):
            entry['response_fields'] = list(output.keys())
            for key in ['model', 'usage', 'answers']:
                if key in output: entry[key] = output[key]
            if 'response' in output: entry['sample_response'] = output['response']
    except urllib.error.HTTPError as error:
        entry['http_status'] = error.code
        try:
            body = json.load(error)
            entry['error_codes'] = [e.get('code') for e in body.get('errors', [])]
            # Only report known infrastructure errors; never dump arbitrary provider bodies.
            safe = ['not found', 'not available', 'Unauthorized', 'not authorized', 'Authentication', 'Unknown model']
            entry['error_summary'] = next((s for s in safe if s.lower() in json.dumps(body).lower()), 'Provider rejected the request')
        except Exception: pass
    except Exception as error: entry['error_type'] = type(error).__name__
    entry['elapsed_seconds'] = round(time.monotonic() - start, 3)
    report['calls'].append(entry)
path = Path(__file__).resolve().parent.parent / 'docs/evidence/cloudflare-smoke.json'
path.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
