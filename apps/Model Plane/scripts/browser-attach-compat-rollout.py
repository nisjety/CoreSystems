"""Protected local browser-attach compatibility rollout.

dry-check-current: validate both current configs with a disposable in-memory key.
prepare: snapshot current runtime configurations and privately generate a new key.
check: render upgrade and rollback with a separate temporary key; no containers changed.
apply / rollback: explicitly recreate the two named services after re-validating configuration.
Usage: python browser-attach-compat-rollout.py prepare|check|apply|reapply-check|reapply|rollback-check|rollback
"""
import ctypes, ctypes.wintypes as wt, json, os, pathlib, secrets, subprocess, sys, tempfile, hashlib, re

ROOT = pathlib.Path(__file__).resolve().parents[1]
SNAPSHOT = pathlib.Path(os.environ.get('VEREVON_ATTACH_SNAPSHOT_PATH', str(pathlib.Path(tempfile.gettempdir())/'verevon-attach-compat-rollout.dpapi')))
CANDIDATES = {'execution-core':'verevon-execution-attach-compat:local', 'model-gateway':'verevon-gateway-attach-compat:local'}
CONTAINERS = {x:'model-plane-'+x+'-1' for x in CANDIDATES}
COMPAT = 'BROWSER_ATTACH_COMPAT_HMAC_KEY'

class Blob(ctypes.Structure):
    _fields_ = [('cbData', wt.DWORD), ('pbData', ctypes.POINTER(ctypes.c_byte))]
def crypt(raw, encrypt):
    buf=ctypes.create_string_buffer(raw); src=Blob(len(raw),ctypes.cast(buf,ctypes.POINTER(ctypes.c_byte))); dst=Blob()
    func=ctypes.windll.crypt32.CryptProtectData if encrypt else ctypes.windll.crypt32.CryptUnprotectData
    if not func(ctypes.byref(src),None,None,None,None,0,ctypes.byref(dst)): raise RuntimeError('DPAPI operation failed')
    try: return ctypes.string_at(dst.pbData,dst.cbData)
    finally: ctypes.windll.kernel32.LocalFree(dst.pbData)
def docker_json(*args):
    return json.loads(subprocess.check_output(['docker',*args],stderr=subprocess.DEVNULL))[0]
def env_of(snapshot):
    return dict(item.split('=',1) for item in snapshot['Config']['Env'])
def current(): return {name:docker_json('inspect',container) for name,container in CONTAINERS.items()}
def candidates(): return {name:docker_json('image','inspect',image) for name,image in CANDIDATES.items()}
def snapshot_read(): return json.loads(crypt(SNAPSHOT.read_bytes(),False))
def security(option):
    if not option.startswith('seccomp='): return option
    value=option[len('seccomp='):]
    if value.startswith('{'): return json.dumps(json.loads(value),sort_keys=True)
    profile=pathlib.Path(value)
    if not profile.is_absolute(): profile=ROOT/'deploy'/profile
    return json.dumps(json.loads(profile.read_text(encoding='utf-8')),sort_keys=True)
def image_user(image): return image.get('Config',{}).get('User') or ''
def normalise_size(value):
    if value is None: return 0
    if isinstance(value,(int,float)): return int(value)
    text=str(value).strip().lower(); units={'b':1,'k':1000,'kb':1000,'m':1000**2,'mb':1000**2,'g':1000**3,'gb':1000**3,'ki':1024,'kib':1024,'mi':1024**2,'mib':1024**2,'gi':1024**3,'gib':1024**3}
    for u in sorted(units,key=len,reverse=True):
        if text.endswith(u): return int(float(text[:-len(u)])*units[u])
    return int(text)
def duration_ns(value):
    if value is None: return 0
    if isinstance(value, int): return value
    factors={'h':3600e9, 'm':60e9, 's':1e9, 'ms':1e6, 'us':1e3, 'µs':1e3, 'ns':1}
    parts=re.findall(r'(\d+(?:\.\d+)?)(ns|us|µs|ms|h|m|s)', str(value))
    if not parts or ''.join(a+b for a,b in parts)!=str(value): raise ValueError('unsupported duration format')
    return int(sum(float(a)*factors[b] for a,b in parts))
def verify_baseline(saved, actual, phase):
    for name in CANDIDATES:
        old=saved['snapshots'][name]; now=actual[name]; env=env_of(now); original=env_of(old)
        if phase in ('check','apply'):
            assert now['Id']==old['Id'] and now['Image']==old['Image'] and env==original, name+' changed since snapshot'
        else:
            expected_candidates={saved['candidates'][name]['Id'],old['Image']}
            assert now['Image'] in expected_candidates, name+' image changed since preparation'
            if now['Image']==old['Image']: assert env==original, name+' original env changed'
            else:
                desired=dict(original);desired[COMPAT]=saved['key'];assert env==desired, name+' upgraded env changed'
        assert now['State']['Running'], name+' is not running'
def verify_reapply_baseline(saved, actual):
    # Re-creation by plain Compose changes container IDs and may remove the key.
    # It must not silently authorize unrelated configuration drift.
    host_fields = ('Privileged','ReadonlyRootfs','Init','CapAdd','CapDrop','SecurityOpt',
                   'PidsLimit','Memory','MemorySwap','NanoCpus','RestartPolicy','PortBindings',
                   'NetworkMode','Devices','DeviceRequests','Dns','DnsOptions','DnsSearch',
                   'ExtraHosts','GroupAdd','IpcMode','PidMode','UTSMode','UsernsMode',
                   'Sysctls','Ulimits','CpuShares','CpuQuota','CpuPeriod','CpusetCpus',
                   'CpusetMems','OomKillDisable','OomScoreAdj','ShmSize','Runtime',
                   'CgroupnsMode','CgroupParent','LogConfig')
    cfg_fields=('User','WorkingDir','Entrypoint','Cmd','Healthcheck','StopSignal')
    for name in CANDIDATES:
        old=saved['snapshots'][name]; now=actual[name]
        assert now['State']['Running'], name+' is not running'
        assert now['Image'] in {old['Image'],saved['candidates'][name]['Id']}, name+' image changed since preparation'
        original=env_of(old); actual_env=env_of(now); with_key=dict(original); with_key[COMPAT]=saved['key']
        assert actual_env in (original,with_key),name+' environment changed since preparation'
        for field in host_fields:
            assert now['HostConfig'].get(field)==old['HostConfig'].get(field),name+' host configuration changed: '+field
        for field in cfg_fields:
            assert now['Config'].get(field)==old['Config'].get(field),name+' configuration changed: '+field
        assert set(now['NetworkSettings']['Networks'])==set(old['NetworkSettings']['Networks']),name+' networks changed'
        def mounts(container):
            return sorted((m['Type'],m.get('Name',m['Source']),m['Destination'],m['RW'],m.get('Propagation','')) for m in container['Mounts'])
        assert mounts(now)==mounts(old),name+' mounts changed'
        old_image=docker_json('image','inspect',old['Image'])
        inherited=old_image.get('Config',{}).get('Labels') or {}
        custom={k:v for k,v in (old['Config'].get('Labels') or {}).items() if not k.startswith('com.docker.compose.') and k not in inherited}
        assert all((now['Config'].get('Labels') or {}).get(k)==v for k,v in custom.items()),name+' custom labels changed'

def compose_plan(saved, image_refs, temp_key):
    env=os.environ.copy(); env['MSYS2_ENV_CONV_EXCL']='VEREVON_COMPAT_ENV_'
    lines=['services:']; wanted={}; n=0
    for name in CANDIDATES:
        snap=saved['snapshots'][name]; values=env_of(snap)
        if temp_key is not None: values[COMPAT]=temp_key
        wanted[name]=values
        lines += ['  '+name+':','    image: '+image_refs[name], '    environment: !override']
        for k,v in values.items():
            var='VEREVON_COMPAT_ENV_'+str(n); n+=1; env[var]=v; lines.append('      '+json.dumps(k)+': ${'+var+'}')
    envfiles=saved['snapshots']['execution-core']['Config']['Labels']['com.docker.compose.project.environment_file'].split(',')
    other=saved['snapshots']['model-gateway']['Config']['Labels']['com.docker.compose.project.environment_file'].split(',')
    assert set(envfiles)==set(other) and all(pathlib.Path(f).is_file() for f in envfiles), 'compose environment file mismatch'
    envfiles=list(dict.fromkeys(envfiles))
    tmp=tempfile.NamedTemporaryFile('w',encoding='utf8',suffix='.yml',prefix='.attach-compat-',dir=ROOT/'deploy',delete=False)
    path=pathlib.Path(tmp.name)
    try:
        with tmp: tmp.write('\n'.join(lines)+'\n')
        cmd=[r'C:\Program Files\Git\bin\bash.exe','./scripts/compose.sh']
        for f in envfiles: cmd+=['--env-file',f]
        cmd+=['-f','deploy/'+path.name]
        res=subprocess.run(cmd+['config','--format','json'],cwd=ROOT,env=env,text=True,capture_output=True)
        if res.returncode: raise RuntimeError('Could not render compose config (exit '+str(res.returncode)+')')
        cfg=json.loads(res.stdout)
        for name in CANDIDATES:
            svc=cfg['services'][name]; snap=saved['snapshots'][name];host=snap['HostConfig']; candidate=saved['candidates'][name]
            assert svc['image']==image_refs[name] and {k:str(v) for k,v in svc['environment'].items()}==wanted[name], name+' image/env mismatch'
            networks={cfg['networks'][n].get('name',n) for n in svc['networks']};assert networks==set(snap['NetworkSettings']['Networks']),name+' networks mismatch'
            ports={(str(p['target'])+'/'+p.get('protocol','tcp'),str(p.get('published','')),p.get('host_ip','')) for p in svc.get('ports',[])}
            expected={(t,str(p['HostPort']),p['HostIp']) for t,entries in (host.get('PortBindings') or {}).items() for p in entries};assert ports==expected,name+' ports mismatch'
            mounts={(m['type'],cfg.get('volumes',{}).get(m.get('source'),{}).get('name',m.get('source')),m['target'],not m.get('read_only',False)) for m in svc.get('volumes',[])}
            expected={(m['Type'],m.get('Name',m['Source']),m['Destination'],m['RW']) for m in snap['Mounts']};assert mounts==expected,name+' mounts mismatch'
            for comp,field in [('privileged','Privileged'),('read_only','ReadonlyRootfs'),('init','Init')]: assert bool(svc.get(comp,False))==bool(host.get(field,False)),name+' '+comp
            for comp,field in [('cap_add','CapAdd'),('cap_drop','CapDrop')]: assert sorted(svc.get(comp) or [])==sorted(host.get(field) or []),name+' '+comp
            assert sorted(map(security,svc.get('security_opt') or []))==sorted(map(security,host.get('SecurityOpt') or [])),name+' security_opt'
            assert svc.get('pids_limit')==host.get('PidsLimit'),name+' pids_limit'
            assert normalise_size(svc.get('mem_limit'))==host.get('Memory',0),name+' mem_limit'
            assert normalise_size(svc.get('memswap_limit'))==host.get('MemorySwap',0),name+' memswap_limit'
            assert int(float(svc.get('cpus',0))*1_000_000_000)==host.get('NanoCpus',0),name+' cpu limit'
            assert (svc.get('restart') or 'no')==host['RestartPolicy']['Name'],name+' restart policy'
            img=candidate if temp_key is not None else docker_json('image','inspect',snap['Image'])
            assert all(item.split('=',1)[0] in wanted[name] for item in (img.get('Config',{}).get('Env') or [])),name+' candidate adds unexpected image environment keys'
            assert (svc.get('user') or image_user(img))==(snap['Config'].get('User') or ''),name+' user mismatch'
            assert (svc.get('working_dir') or img.get('Config',{}).get('WorkingDir') or '')==(snap['Config'].get('WorkingDir') or ''),name+' working directory'
            assert (svc.get('entrypoint') or img.get('Config',{}).get('Entrypoint'))==snap['Config'].get('Entrypoint'),name+' entrypoint'
            assert (svc.get('command') if svc.get('command') is not None else img.get('Config',{}).get('Cmd'))==snap['Config'].get('Cmd'),name+' command mismatch'
            assert (svc.get('stop_signal') or img.get('Config',{}).get('StopSignal'))==snap['Config'].get('StopSignal'),name+' stop signal mismatch'
            health=svc.get('healthcheck'); oldhealth=snap['Config'].get('Healthcheck')
            if health is None: assert img.get('Config',{}).get('Healthcheck')==oldhealth,name+' image healthcheck mismatch'
            else:
                assert oldhealth is not None and health.get('test')==oldhealth.get('Test'),name+' healthcheck command mismatch'
                for cf,df in [('interval','Interval'),('timeout','Timeout'),('start_period','StartPeriod'),('start_interval','StartInterval')]:
                    assert (duration_ns(health[cf]) if cf in health else (img.get('Config',{}).get('Healthcheck') or {}).get(df,0))==oldhealth.get(df,0),name+' healthcheck '+cf+' mismatch'
                assert int(health.get('retries',(img.get('Config',{}).get('Healthcheck') or {}).get('Retries',0)))==oldhealth.get('Retries',0),name+' healthcheck retries mismatch'
            labels=svc.get('labels') or {}; oldlabels=snap['Config'].get('Labels') or {}; oldimg=docker_json('image','inspect',snap['Image']); inherited=oldimg.get('Config',{}).get('Labels') or {}
            custom={k:v for k,v in oldlabels.items() if not k.startswith('com.docker.compose.') and k not in inherited}
            assert all(labels.get(k)==v for k,v in custom.items()),name+' custom labels mismatch'
        yield cmd,env,wanted
    finally: path.unlink(missing_ok=True)

def main(mode):
    if mode=='dry-check-current':
        snapshots=current(); imgs={n:docker_json('image','inspect',snapshots[n]['Image']) for n in CANDIDATES}; dummy=secrets.token_urlsafe(48)
        trial={'snapshots':snapshots,'candidates':imgs,'key':secrets.token_urlsafe(48)}
        assert dummy != trial['key']
        plan=compose_plan(trial,{n:imgs[n]['Id'] for n in CANDIDATES},dummy)
        try:
            next(plan);print('Dry-run validated both existing configurations with a disposable in-memory key; no snapshot or service change.')
        finally: plan.close()
        return
    if mode=='prepare':
        if SNAPSHOT.exists(): raise RuntimeError('Protected snapshot already exists; preserve it and choose a new path')
        snapshots=current(); imgs=candidates(); key=secrets.token_urlsafe(48)
        assert all(COMPAT not in env_of(s) for s in snapshots.values()), 'compat key already configured'
        execution=env_of(snapshots['execution-core']); assert execution.get('BROWSER_AGENT_POLICY_ENFORCED')=='true' and execution.get('EXECUTION_CORE_CAPABILITY_DECISION_PUBLIC_KEY') and execution.get('QUARRY_BROWSER_AGENT_PLANNER_MODE')=='allowlist' and execution.get('QUARRY_BROWSER_AGENT_LLM_ORGS'), 'browser policy preflight failed'
        payload=json.dumps({'snapshots':snapshots,'candidates':imgs,'key':key}).encode(); protected=crypt(payload,True)
        assert crypt(protected,False)==payload, 'protected snapshot verification failed'
        with SNAPSHOT.open('xb') as f: f.write(protected)
        print('Protected both service configurations, image IDs and a new dedicated key with user-bound DPAPI:', SNAPSHOT)
        return
    saved=snapshot_read(); now=current()
    if mode in ('reapply-check','reapply'): verify_reapply_baseline(saved,now)
    else: verify_baseline(saved,now,mode)
    # The dry-run secret is generated only in memory and is always distinct from
    # the protected apply key. It is never printed or stored in rendered files.
    dummy=secrets.token_urlsafe(48)
    while dummy==saved['key']: dummy=secrets.token_urlsafe(48)
    if mode in ('check','apply','reapply-check','reapply'):
        images={n:saved['candidates'][n]['Id'] for n in CANDIDATES}
        for n in CANDIDATES: assert docker_json('image','inspect',CANDIDATES[n])['Id']==images[n],n+' candidate tag changed'
        secret=dummy if mode in ('check','reapply-check') else saved['key']
    else: images={n:saved['snapshots'][n]['Image'] for n in CANDIDATES};secret=None
    plan=compose_plan(saved,images,secret)
    cmd,env,wanted=next(plan)
    try:
        print('Validated both service configurations, intended image/env changes, custom labels, networks, ports, mounts, user, resource limits, restart and security options.')
        if mode in ('apply','rollback','reapply'):
            order=['execution-core','model-gateway'] if mode=='rollback' else ['model-gateway','execution-core']
            for name in order:
                if mode=='reapply' and now[name]['Image']==images[name] and env_of(now[name])==wanted[name]:
                    print('Already current',name)
                    continue
                result=subprocess.run(cmd+['up','-d','--no-deps','--no-build',name],cwd=ROOT,env=env,capture_output=True)
                if result.returncode: raise RuntimeError(name+' recreation failed; inspect protected snapshot and rerun rollback if needed')
                observed=docker_json('inspect',CONTAINERS[name]);assert observed['Image']==images[name] and env_of(observed)==wanted[name],name+' post-switch mismatch'
                if mode in ('apply','reapply'): verify_reapply_baseline(saved,current())
                print('Recreated and validated',name)
    finally: plan.close()
if __name__=='__main__':
    if sys.argv[1:] not in ([x] for x in ('dry-check-current','prepare','check','apply','reapply-check','reapply','rollback-check','rollback')): raise SystemExit(__doc__)
    main(sys.argv[1])