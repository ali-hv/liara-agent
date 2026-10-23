#!/usr/bin/env python3
"""Reproducible, resumable public documentation crawl. pip install requests beautifulsoup4 pyyaml."""
import concurrent.futures, datetime, hashlib, json, pathlib, re, time, sys
from urllib.parse import urljoin, urlparse, urldefrag
import requests, yaml
from bs4 import BeautifulSoup

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs' / 'liara-snapshot'
OUT.mkdir(parents=True, exist_ok=True)
HOSTS = {'developers.liara.ir', 'docs.liara.ir'}
SEEDS = ['https://developers.liara.ir/', 'https://developers.liara.ir/llms.txt', 'https://developers.liara.ir/sitemap.xml', 'https://docs.liara.ir/', 'https://docs.liara.ir/sitemap.xml']

def normalized(url):
    url = urldefrag(url)[0].split('?')[0]
    p = urlparse(url)
    if p.scheme not in ('https', 'http') or p.hostname not in HOSTS: return None
    if any(x in p.path for x in ['/_next/', '/cdn-cgi/', '/assets/', '/images/']): return None
    if re.search(r'\.(png|svg|jpg|jpeg|webp|gif|mp4|woff2?|ttf|css|js|ico|pdf|zip)$', p.path): return None
    if p.hostname == 'developers.liara.ir' and not p.path.endswith(('.txt', '.xml', '.yaml', '.json', '.md')) and p.path != '/':
        url = url.rstrip('/') + '.md'
    return url

def fetch(url):
    key = hashlib.sha256(url.encode()).hexdigest()[:20]
    target = OUT / (key + '.txt')
    cached = OUT / (key + '.json')
    if cached.exists() and target.exists():
        record = json.loads(cached.read_text())
        if record['status'] == 200:
            content=target.read_text()
            try:
                recovered=content.encode('latin-1').decode('utf-8')
                if recovered!=content:
                    content=recovered; target.write_text(content); record['sha256']=hashlib.sha256(content.encode()).hexdigest(); record['encoding']='utf-8 (recovered from HTTP default encoding)'; cached.write_text(json.dumps(record,ensure_ascii=False,indent=2))
            except (UnicodeEncodeError,UnicodeDecodeError): pass
            return record, content
    if '--offline' in sys.argv and cached.exists(): return json.loads(cached.read_text()), ''
    record = {'url': url, 'retrieved_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'file': target.name}
    for attempt in range(3):
        try:
            r = requests.get(url, timeout=35, headers={'User-Agent': 'LiaraDesktopDocumentationAudit/1.0'})
            r.encoding = 'utf-8'
            record.update(status=r.status_code, content_type=r.headers.get('content-type',''), final_url=r.url)
            if r.status_code == 200:
                text = r.text
                target.write_text(text)
                record['sha256'] = hashlib.sha256(text.encode()).hexdigest()
                cached.write_text(json.dumps(record, ensure_ascii=False, indent=2))
                return record, text
            record['error'] = r.reason
            if r.status_code not in (429, 500, 502, 503, 504): break
        except Exception as e: record.update(status=0, error=str(e))
        time.sleep(attempt + 1)
    cached.write_text(json.dumps(record, ensure_ascii=False, indent=2))
    return record, ''

def links(url, body):
    found = re.findall(r'<loc>(.*?)</loc>', body)
    if '<html' in body[:2000].lower() or '<!doctype' in body[:100].lower():
        soup = BeautifulSoup(body, 'html.parser')
        found += [a.get('href','') for a in soup.find_all('a')]
    else:
        found += re.findall(r'\]\(([^)\s]+)', body)
        found += re.findall(r'(/spec/[a-zA-Z0-9_-]+\.ya?ml)', body)
    return {v for x in found if (v := normalized(urljoin(url,x)))}

seen, records, bodies = set(), [], {}
pending = set(SEEDS)
while pending:
    batch = sorted(pending - seen)
    if not batch: break
    seen.update(batch); pending = set()
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        for rec, body in pool.map(fetch, batch):
            records.append(rec)
            if body:
                bodies[rec['url']] = body
                pending.update(links(rec['url'], body) - seen)
    print(f'Fetched {len(records)} pages; pending {len(pending)}; failures {sum(r["status"]!=200 for r in records)}', flush=True)

# Extract authoritative operation definitions from downloaded full OpenAPI specifications.
operations = {}
for url, body in bodies.items():
    if url.endswith('.md'):
        match = re.search(r'````yaml[^\n]*\n(.*?)\n````', body, re.S)
        if not match: continue
        body = match.group(1)
    elif not url.endswith(('.yaml','.yml','.json')): continue
    try: spec = yaml.safe_load(body)
    except Exception: continue
    if not isinstance(spec,dict) or 'paths' not in spec: continue
    def resolve(obj):
        if isinstance(obj,dict) and obj.get('$ref','').startswith('#/'):
            value=spec
            for part in obj['$ref'][2:].split('/'): value=value.get(part,{})
            return value
        return obj or {}
    service=spec.get('info',{}).get('title',url)
    for path, item in spec['paths'].items():
        for method, op in item.items():
            if method.lower() not in ['get','post','put','patch','delete','head','options']: continue
            params=[f"{p.get('name')} ({p.get('in')}{', required' if p.get('required') else ''})" for p in op.get('parameters',[]) if isinstance(p,dict)]
            body_schema=[]
            for mime, content in op.get('requestBody',{}).get('content',{}).items():
                schema=resolve(content.get('schema',{})); required=schema.get('required',[])
                body_schema.append(mime + ': ' + ', '.join(k+('*' if k in required else '') for k in schema.get('properties',{})))
            responses=[]
            for code,r in op.get('responses',{}).items():
                props=[]
                for c in r.get('content',{}).values(): props+=list(resolve(c.get('schema',{})).get('properties',{}))
                responses.append(str(code)+(': '+', '.join(props) if props else ' (schema unspecified)'))
            operations[(service,method,path)]={'service':service,'method':method.upper(),'path':path,'summary':op.get('summary',op.get('description','')),'inputs':'; '.join(params+body_schema) or 'No inputs specified','outputs':'; '.join(responses),'authentication':op.get('security',spec.get('security',[])),'source':url,'servers':spec.get('servers',[])}
manifest={'generated_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'seeds':SEEDS,'counts':{'discovered':len(records),'successful':sum(r['status']==200 for r in records),'failed':sum(r['status']!=200 for r in records),'operations':len(operations)},'pages':sorted(records,key=lambda r:r['url'])}
(ROOT/'docs/liara-crawl-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2))
(ROOT/'docs/liara-operations.json').write_text(json.dumps(list(operations.values()),ensure_ascii=False,indent=2))

def esc(x):return str(x).replace('|','\\|').replace('\n',' ').strip()
lines=['# Liara API capability inventory','',f"Generated: {manifest['generated_at']}",'',json.dumps(manifest['counts']), '', '## Evidence and scope','', 'This catalog is generated from the complete reachable documentation crawl and linked OpenAPI specifications. An API operation is documented, not authenticated/live-tested. An asterisk on an input means the specification marks it required. Unspecified schemas are not guarantees of empty responses. All raw pages, hashes, retrieval times and failures are retained in the crawl manifest and local snapshot.','', 'The desktop release uses account/billing, PaaS, DBaaS, object storage and DNS for deployment. Reports are limited to deployment diagnostics. VPS, mail administration, AI services, file-browser administration and ongoing SRE are cataloged but outside this release. No endpoint has been invented.','', '## Authentication and limitations','', 'Send the user token using the documented Bearer Authorization scheme. Service base URLs differ and are recorded in liara-operations.json. Respect permissions, errors and rate limits; no numeric rate limit is assumed. Pricing and resource availability must come from current account data. Never log account/storage credential fields. Some OpenAPI schemas omit fields used by the official CLI; these must be separately corroborated before use.','', '## Official CLI corroboration','', '[Plan list source](https://github.com/liara-cloud/liara-cli/blob/master/src/commands/plan/list.ts) corroborates GET /v1/me for project and bundle plans. [Deployment source](https://github.com/liara-cloud/liara-cli/blob/master/src/commands/deploy.ts) corroborates source upload followed by release creation and release polling. These references are distinct from the generated OpenAPI contract. Neither establishes live access for a particular account.','']
for service in sorted({o['service'] for o in operations.values()}):
    lines += ['## '+service,'','| Capability | Method and path | Inputs | Outputs | Auth | Release scope | Source |','|---|---|---|---|---|---|---|']
    for o in operations.values():
        if o['service']!=service:continue
        low=service.lower();scope='Deployment integration' if any(x in low for x in ['paas','dbaas','account','billing','storage','dns']) else 'Catalog only'
        if any(x in o['path'] for x in ['reports','metrics']):scope='Deployment diagnostics only'
        lines += ['| '+' | '.join(map(esc,[o['summary'],f"`{o['method']} {o['path']}`",o['inputs'],o['outputs'],json.dumps(o['authentication']),scope,f"[spec]({o['source']})"]))+' |']
    lines += ['']
lines+=['## Crawl failures','']+[f"- {r['url']}: {r['status']} {r.get('error','')}" for r in records if r['status']!=200]
(ROOT/'docs/liara-api-capabilities.md').write_text('\n'.join(lines)+'\n')
print(json.dumps(manifest['counts']),flush=True)
