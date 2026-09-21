#!/usr/bin/env python3
"""Opt-in Jev evaluation through Cloudflare using Wrangler credentials and the app's question bank.
Never run by build/test scripts. Requires human-reviewed labels; intentionally refuses draft fixtures.
"""
import argparse, json, subprocess, time, urllib.request, urllib.error
from pathlib import Path
root = Path(__file__).resolve().parent.parent
p = argparse.ArgumentParser()
p.add_argument('--live', action='store_true')
p.add_argument('--account', required=True)
p.add_argument('--limit', type=int, default=3)
p.add_argument('--fixtures', type=Path, default=root / 'Fixtures/jev-evaluation.json')
a = p.parse_args()
if not a.live: p.error('Use --live to authorize potentially billable Cloudflare calls.')
if not 1 <= a.limit <= 29: p.error('Limit must be between 1 and 29.')
if len(a.account) != 32 or any(c not in '0123456789abcdef' for c in a.account.lower()): p.error('Invalid account ID')
cases = json.loads(a.fixtures.read_text())['cases']
reviewed = [c for c in cases if c['review']['status'] == 'approved' and c['review']['reviewer'] and c['review']['reviewed_at']][:a.limit]
if not reviewed: p.error('No human-reviewed fixtures. Review labels and record reviewer/date before evaluating.')
banks = json.loads(subprocess.run(['swift', 'run', 'PickleChecks', '--export-evaluators'], cwd=root, check=True, capture_output=True, text=True).stdout)
auth = subprocess.run(['npx','--yes','wrangler@4.134.0','auth','token','--json'], check=True, capture_output=True, text=True)
token = json.loads(auth.stdout)['token']
report = {'mode':'live Cloudflare Jev','policy':'provisional 0.2/0.8 Noul; 0.8 Choice confidence','cases':[], 'label_provenance':'human-approved fixtures only'}
for c in reviewed:
    state = {'source':c['source']}
    if 'candidate' in c: state['candidate'] = c['candidate']
    body = {'model':'typesafe/jev','input':{'state':state,'questions':banks[c['family']]}}
    req = urllib.request.Request('https://api.cloudflare.com/client/v4/accounts/'+a.account+'/ai/run',data=json.dumps(body).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
    start=time.monotonic()
    try:
        with urllib.request.urlopen(req, timeout=40) as response: data=json.load(response)
        result=data.get('result',data); answers=result['answers']; predictions={}
        if c['family']=='chart':
            yes=lambda k: answers[k]['noul'] >= .8
            route='none'
            if yes('visual_justified'):
                if yes('comparable_quantities') and yes('compatible_units'): route='bar_chart'
                elif yes('ordered_process') or yes('relationships'): route='flow_diagram'
                else: route='uncertain'
            elif answers['visual_justified']['noul'] > .2: route='uncertain'
            predictions['route']=route
        else:
            for key in c['expected']:
                ans=answers[key]
                if ans['type']=='choice': predictions[key]=ans['choice'] if ans['confidence']>=.8 else 'uncertain'
                else: predictions[key]=True if ans['noul']>=.8 else False if ans['noul']<=.2 else 'uncertain'
        comparisons=[(v,predictions[k]) for k,v in c['expected'].items()]
        report['cases'].append({'id':c['id'],'family':c['family'],'model':result['model'],'expected':c['expected'],'predictions':predictions,'correct':sum(x==y for x,y in comparisons),'total':len(comparisons),'false_warnings':sum(x is False and y is True for x,y in comparisons),'missed_problems':sum(x is True and y is False for x,y in comparisons),'uncertain':sum(y=='uncertain' for x,y in comparisons),'elapsed_seconds':round(time.monotonic()-start,3),'usage':result.get('usage')})
    except urllib.error.HTTPError as error:
        report['cases'].append({'id':c['id'],'http_status':error.code,'result':'service unavailable'})
        break  # No automatic retries/billing loops.
report['cost']='Consult Cloudflare billing; this route does not report dollar cost.'
report['repair_success']='Not measured by this evaluator-only run; use controlled generation pairs for repair studies.'
path=root/'docs/evidence/jev-live-evaluation.json';path.write_text(json.dumps(report,indent=2)+'\n');print(str(path))
