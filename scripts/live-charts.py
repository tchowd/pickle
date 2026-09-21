#!/usr/bin/env python3
"""Explicit opt-in: run TWO potentially billable chart tests using the native Swift pipeline."""
import argparse,json,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--live',action='store_true');p.add_argument('--account',required=True);p.add_argument('--flow-only',action='store_true');a=p.parse_args()
if not a.live:p.error('Specify --live to authorize two small Cloudflare chart calls.')
root=Path(__file__).resolve().parent.parent
subprocess.run(['swift','build','--product','PickleChecks'],cwd=root,check=True)
auth=subprocess.run(['npx','--yes','wrangler@4.134.0','auth','token','--json'],capture_output=True,text=True,check=True)
token=json.loads(auth.stdout)['token']
result=subprocess.run([str(root/'.build/debug/PickleChecks'),'--live-cloudflare-charts',a.account]+(['--flow-only'] if a.flow_only else []),input=token,capture_output=True,text=True)
evidence=root/'docs/evidence/native-live-charts.txt'
evidence.write_text((evidence.read_text() if evidence.exists() else '') + result.stdout)
print(result.stdout)
raise SystemExit(result.returncode)
