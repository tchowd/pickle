#!/usr/bin/env python3
"""Usage: python3 scripts/install-browser-bridge.py EXTENSION_ID [chrome|edge|brave|arc]"""
import json, pathlib, re, shlex, shutil, sys
root = pathlib.Path(__file__).resolve().parent.parent
if len(sys.argv) not in (2,3) or not re.fullmatch('[a-p]{32}',sys.argv[1]):
    raise SystemExit(__doc__)
browsers = {'chrome':('Google/Chrome','com.google.Chrome'), 'edge':('Microsoft Edge','com.microsoft.edgemac'), 'brave':('BraveSoftware/Brave-Browser','com.brave.Browser'), 'arc':('Arc/User Data','company.thebrowser.Browser')}
folder,bundle = browsers[sys.argv[2] if len(sys.argv)>2 else 'chrome']
app = root/'dist/Pickle.app'
if not app.exists(): raise SystemExit('Build Pickle first: ./scripts/build-app.sh')
host = pathlib.Path.home()/'Library/Application Support/Pickle/NativeHost'
host.mkdir(parents=True, exist_ok=True, mode=0o700)
shutil.copyfile(root/'scripts/browser-host.py',host/'browser-host.py')
launcher = host/('host-'+bundle)
launcher.write_text('#!/bin/sh\nexport PICKLE_BROWSER_BUNDLE='+shlex.quote(bundle)+'\nexport PICKLE_APP='+shlex.quote(str(app))+'\nexec /usr/bin/python3 '+shlex.quote(str(host/'browser-host.py'))+' "$@"\n')
launcher.chmod(0o700)
manifest = pathlib.Path.home()/'Library/Application Support'/folder/'NativeMessagingHosts/com.pickle.reader.json'
manifest.parent.mkdir(parents=True,exist_ok=True)
manifest.write_text(json.dumps({'name':'com.pickle.reader','description':'Pickle page context','path':str(launcher),'type':'stdio','allowed_origins':['chrome-extension://'+sys.argv[1]+'/']},indent=2))
print('Installed bridge:',manifest)
