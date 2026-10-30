import assert from 'node:assert/strict'
import { chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import test from 'node:test'
import { planStudioPrincipals, provisionStudioPrincipals } from './provision-studio-principals.mjs'

function fixture(t) {
 const root=mkdtempSync(join(tmpdir(),'studio-principals-test-'));t.after(()=>rmSync(root,{recursive:true,force:true}))
 const secretsDirectory=join(root,'secrets');mkdirSync(secretsDirectory,{mode:0o700})
 const key='a'.repeat(64)
 const original={unrelated:{credential:'unrelated-private-credential-value',audiences:['ingestion'],orgIds:['other-org'],scopes:['integration:read']},'marketing-core':{credential:key,audiences:['ingestion'],orgIds:['org-test'],scopes:['integration:read'],scopesByAudience:{ingestion:['integration:read']},retentionByAudience:{ingestion:'persistent'}}}
 const registryFile=join(secretsDirectory,'marketing_auth_plane_principals.json');writeFileSync(registryFile,JSON.stringify(original)+'\n',{mode:0o600});writeFileSync(join(secretsDirectory,'marketing_plane_service_key'),key,{mode:0o600})
 return {root,secretsDirectory,registryFile,original,options:{organizationId:'org-test',secretsDirectory}}
}

test('dry run plans exact fixed-tenant capabilities without writing or exposing secrets',t=>{
 const {options,original,secretsDirectory,registryFile}=fixture(t);const before=readdirSync(secretsDirectory);const bytes=readFileSync(registryFile)
 const plan=planStudioPrincipals(options);assert.equal(plan.created,3);assert.equal(plan.changed,4);assert.deepEqual(readdirSync(secretsDirectory),before);assert.deepEqual(readFileSync(registryFile),bytes)
 assert.equal(JSON.stringify(plan).includes(original['marketing-core'].credential),false);assert.equal(JSON.stringify(plan).includes(original.unrelated.credential),false)
 provisionStudioPrincipals(options);assert.deepEqual(readdirSync(secretsDirectory),before)
})

test('apply extends the mounted registry, preserves credentials and unrelated policy, with protected backup',t=>{
 const {options,registryFile,secretsDirectory,original}=fixture(t);const originalBytes=readFileSync(registryFile)
 const result=provisionStudioPrincipals({...options,apply:true});assert.equal(result.created,3);assert.equal(result.changed,4);assert.ok(result.backupFilename)
 const registry=JSON.parse(readFileSync(registryFile,'utf8'));assert.deepEqual(registry.unrelated,original.unrelated);assert.equal(registry['marketing-core'].credential,original['marketing-core'].credential)
 assert.deepEqual(readFileSync(join(secretsDirectory,result.backupFilename)),originalBytes)
 for(const core of ['marketing','social','leads','anbud']){const key=readFileSync(join(secretsDirectory,`${core}_plane_service_key`),'utf8');assert.match(key,/^[a-f0-9]{64}$/);const principal=registry[`${core}-core`];assert.equal(principal.credential,key);assert.deepEqual(principal.orgIds,['org-test']);assert.equal(principal.allowAnyOrg,undefined);assert.deepEqual(principal.scopesByAudience,{ingestion:['integration:read','integration:write'],quarry:['map:read','scrape:read','scrape:write'],'model-gateway':['models:invoke'],'inference-core':['inference:invoke']});assert.deepEqual(principal.retentionByAudience,{ingestion:'persistent',quarry:'persistent','model-gateway':'zdr','inference-core':'zdr'});assert.equal(lstatSync(join(secretsDirectory,`${core}_plane_service_key`)).mode&0o777,0o600)}
 assert.equal(lstatSync(registryFile).mode&0o777,0o600);assert.equal(lstatSync(join(secretsDirectory,result.backupFilename)).mode&0o777,0o600)
 const after=readFileSync(registryFile);const files=readdirSync(secretsDirectory);const again=provisionStudioPrincipals({...options,apply:true});assert.equal(again.created,0);assert.equal(again.changed,0);assert.equal(again.backupFilename,null);assert.deepEqual(readFileSync(registryFile),after);assert.deepEqual(readdirSync(secretsDirectory),files)
})

test('rejects mismatched org, any-org, unexpected scopes or credential mismatch without writes',t=>{
 const {options,registryFile,secretsDirectory,original}=fixture(t)
 for(const update of [{orgIds:['other-org']},{orgIds:['org-test','other-org']},{allowAnyOrg:true},{scopes:['integration:read','integration:admin']},{audiences:['ingestion','data-plane']},{credential:'b'.repeat(64)}]){writeFileSync(registryFile,JSON.stringify({...original,'marketing-core':{...original['marketing-core'],...update}}));const before=readdirSync(secretsDirectory);assert.throws(()=>provisionStudioPrincipals({...options,apply:true}));assert.deepEqual(readdirSync(secretsDirectory),before)}
})

test('rejects unsafe permissions, symlinks and missing matching Marketing key',t=>{
 const {options,secretsDirectory,registryFile}=fixture(t)
 chmodSync(secretsDirectory,0o755);assert.throws(()=>planStudioPrincipals(options),/private secrets directory/);chmodSync(secretsDirectory,0o700)
 chmodSync(registryFile,0o644);assert.throws(()=>planStudioPrincipals(options),/private regular file/);chmodSync(registryFile,0o600)
 const keyFile=join(secretsDirectory,'marketing_plane_service_key');chmodSync(keyFile,0o644);assert.throws(()=>planStudioPrincipals(options),/private regular file/);chmodSync(keyFile,0o600)
 const key=readFileSync(keyFile);rmSync(keyFile);symlinkSync(registryFile,keyFile);assert.throws(()=>planStudioPrincipals(options),/private regular file/);rmSync(keyFile);writeFileSync(keyFile,key,{mode:0o600})
 const original=readFileSync(registryFile);rmSync(registryFile);symlinkSync(keyFile,registryFile);assert.throws(()=>planStudioPrincipals(options),/private regular file/);rmSync(registryFile);writeFileSync(registryFile,original,{mode:0o600});rmSync(keyFile);assert.throws(()=>planStudioPrincipals(options),/matching protected key/)
})

test('rejects malformed or oversized registry and invalid organization without creating files',t=>{
 const {options,registryFile,secretsDirectory}=fixture(t)
 for(const body of ['private-malformed-json','[]','null','{"bad":{}}','x'.repeat(1024*1024+1)]){writeFileSync(registryFile,body);assert.throws(()=>provisionStudioPrincipals({...options,apply:true}));assert.equal(existsSync(join(secretsDirectory,'social_plane_service_key')),false)}
 for(const organizationId of ['','../org','*','org id','org\n'])assert.throws(()=>planStudioPrincipals({...options,organizationId}),/organization/)
})

test('CLI defaults to dry run and reports only safe summary',t=>{
 const {options,secretsDirectory}=fixture(t);const before=readdirSync(secretsDirectory)
 const args=[fileURLToPath(new URL('./provision-studio-principals.mjs',import.meta.url)),'--org',options.organizationId,'--secrets-dir',secretsDirectory]
 const dry=spawnSync(process.execPath,args,{encoding:'utf8'});assert.equal(dry.status,0,dry.stderr);assert.deepEqual(readdirSync(secretsDirectory),before);assert.equal(dry.stdout.includes('a'.repeat(64)),false)
 const applied=spawnSync(process.execPath,[...args,'--apply'],{encoding:'utf8'});assert.equal(applied.status,0,applied.stderr);assert.equal(applied.stdout.includes(readFileSync(join(secretsDirectory,'social_plane_service_key'),'utf8')),false)
 const bad=spawnSync(process.execPath,[...args,'--unknown'],{encoding:'utf8'});assert.notEqual(bad.status,0)
})

test('successive approved updates use distinct backup names without rotating keys',t=>{
 const {options,registryFile,secretsDirectory}=fixture(t);const one=provisionStudioPrincipals({...options,apply:true});const registry=JSON.parse(readFileSync(registryFile,'utf8'));const keys=['marketing','social','leads','anbud'].map(core=>readFileSync(join(secretsDirectory,`${core}_plane_service_key`)))
 const p=registry['social-core'];const scopesByAudience={...p.scopesByAudience,ingestion:['integration:read']};const scopes=[...new Set(Object.values(scopesByAudience).flat())];writeFileSync(registryFile,JSON.stringify({...registry,'social-core':{...p,scopesByAudience,scopes}}))
 const two=provisionStudioPrincipals({...options,apply:true});assert.notEqual(one.backupFilename,two.backupFilename);assert.equal(two.created,0);assert.equal(two.changed,1);assert.deepEqual(['marketing','social','leads','anbud'].map(core=>readFileSync(join(secretsDirectory,`${core}_plane_service_key`))),keys)
})
