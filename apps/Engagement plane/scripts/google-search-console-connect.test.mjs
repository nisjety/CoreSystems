import assert from 'node:assert/strict'
import { chmodSync, lstatSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { createConsentHandler, finalizeConsent, prepareConsent, validateAuthorization } from './google-search-console-connect.mjs'

const now = new Date('2026-10-21T11:00:00Z')
const scopeRead = 'https://www.googleapis.com/auth/webmasters.readonly'
const scopes = ['openid', 'profile', 'email', scopeRead]
function bundle() {
  const authorization = new URL('https://accounts.google.com/o/oauth2/v2/auth')
  for (const [key, value] of Object.entries({client_id:'test.apps.googleusercontent.com',redirect_uri:'http://localhost:3026/oauth/callback/google',response_type:'code',scope:scopes.join(' '),state:'s'.repeat(43),code_challenge:'p'.repeat(43),code_challenge_method:'S256'})) authorization.searchParams.set(key,value)
  return {sessionToken:'session-test-123',authorizationUrl:authorization.href,expiresAt:new Date(now.getTime()+600000).toISOString(),provider:{key:'google'},capabilities:['profile.read','search_console.read'],scopes:[...scopes]}
}
function fixture(t, responses) {
  const root=mkdtempSync(join(tmpdir(),'marketing-consent-'));chmodSync(root,0o700)
  t.after(()=>rmSync(root,{recursive:true,force:true}))
  const payload=Buffer.from(JSON.stringify({org_id:'org-aquatiq',aud:'ingestion',principal_type:'service',sub:'service:marketing-core',exp:Math.floor(now.getTime()/1000)+300})).toString('base64url')
  const token=`header.${payload}.signature`
  const calls=[]
  const execFile=async(file,args,options)=>{assert.equal(file,'docker');assert.deepEqual(args,['exec','engagement-plane-marketing-bearer-refresh-1','cat','/run/marketing-bearer/token']);return {stdout:token}}
  const fetch=async(url,options)=>{assert.equal(options.headers.Authorization,`Bearer ${token}`);assert.equal(options.redirect,'manual');calls.push({url,options});const response=responses.shift();assert.ok(response,`unexpected request ${url}`);return new Response(JSON.stringify(response),{status:200,headers:{'content-type':'application/json'}})}
  return {root,calls,options:{organizationId:'org-aquatiq',sessionFile:join(root,'consent.json'),configFile:join(root,'marketing.json'),now:()=>now,execFile,fetch}}
}
test('prepare uses scoped bearer privately and saves only bounded consent metadata',async(t)=>{
  const value=bundle();const {options,calls}=fixture(t,[{success:true,data:{connections:[]}},{success:true,data:value}])
  const result=await prepareConsent(options)
  assert.deepEqual(JSON.parse(calls[1].options.body),{organizationId:'org-aquatiq',bundles:['search_console'],returnUrl:'http://127.0.0.1:5174/studio/pulse'})
  assert.equal(lstatSync(options.sessionFile).mode&0o777,0o600)
  const saved=JSON.parse(readFileSync(options.sessionFile,'utf8'));assert.equal(saved.sessionToken,value.sessionToken)
  assert.equal(JSON.stringify(result).includes(value.authorizationUrl),false);assert.equal(JSON.stringify(saved).includes('signature'),false)
})
test('authorization rejects broader scopes, callback mismatches, and weak PKCE',()=>{
  for(const mutate of [v=>v.scopes.push('https://www.googleapis.com/auth/gmail.readonly'),v=>v.authorizationUrl=v.authorizationUrl.replace('localhost%3A3026','localhost%3A33026'),v=>v.authorizationUrl=v.authorizationUrl.replace('S256','plain'),v=>v.authorizationUrl=v.authorizationUrl.replace('accounts.google.com','evil.example'),v=>v.capabilities.push('gmail.read')]){const value=bundle();mutate(value);assert.throws(()=>validateAuthorization(value,now))}
})
test('serve redirects only local same-origin consent navigation',()=>{
  const value={...bundle(),organizationId:'org-aquatiq'}
  const handler=createConsentHandler(value,()=>now)
  function invoke(headers,path='/connect'){const res={statusCode:0,headers:{},setHeader(k,v){this.headers[k]=v},end(v){this.body=v}};handler({method:'GET',url:path,headers},res);return res}
  const result=invoke({host:'localhost:33027','sec-fetch-site':'none'});assert.equal(result.statusCode,302);assert.equal(result.headers.Location,value.authorizationUrl);assert.equal(result.headers['Cache-Control'],'no-store')
  assert.equal(invoke({host:'evil.example'}).statusCode,403)
  assert.equal(invoke({host:'localhost:33027','sec-fetch-site':'cross-site'}).statusCode,403)
  assert.equal(invoke({host:'localhost:33027'},'/other').statusCode,404)
})
test('finalize requires completed session and verified exact site before saving connection ID',async(t)=>{
  const connection={id:'google-connection-new',organizationId:'org-aquatiq',providerKey:'google',status:'active',capabilities:['profile.read','search_console.read'],createdAt:now.toISOString()}
  const {options}=fixture(t,[{success:true,data:{id:'session-test-123',providerKey:'google',status:'completed',consumedAt:now.toISOString(),errorCode:''}},{success:true,data:{connections:[connection]}},{success:true,data:{action:{providerKey:'google',operation:'gsc.sites.list',result:{siteEntry:[{siteUrl:'sc-domain:aquatiq.com',permissionLevel:'siteFullUser'}]}}}}])
  writeFileSync(options.sessionFile,JSON.stringify({...bundle(),organizationId:'org-aquatiq',preparedAt:now.toISOString(),existingConnectionIds:[]}),{mode:0o600})
  const result=await finalizeConsent(options);assert.equal(result.googleConnectionId,connection.id)
  assert.deepEqual(JSON.parse(readFileSync(options.configFile,'utf8')),{version:1,organizationId:'org-aquatiq',googleConnectionId:connection.id})
  assert.equal(lstatSync(options.configFile).mode&0o777,0o600)
})
test('finalize refuses pending consent and never persists configuration',async(t)=>{
  const {options}=fixture(t,[{success:true,data:{id:'session-test-123',providerKey:'google',status:'pending'}}])
  writeFileSync(options.sessionFile,JSON.stringify({...bundle(),organizationId:'org-aquatiq',preparedAt:now.toISOString(),existingConnectionIds:[]}),{mode:0o600})
  await assert.rejects(finalizeConsent(options),/completed/)
  assert.throws(()=>readFileSync(options.configFile))
})

test('prepare rejects a wrong org, expired bearer, bad audience, and malformed capture before any API call',async(t)=>{
  for(const claims of [
    {org_id:'another-org',aud:'ingestion',principal_type:'service',sub:'service:marketing-core',exp:Math.floor(now.getTime()/1000)+300},
    {org_id:'org-aquatiq',aud:'ingestion',principal_type:'service',sub:'service:marketing-core',exp:Math.floor(now.getTime()/1000)-1},
    {org_id:'org-aquatiq',aud:'data-plane',principal_type:'service',sub:'service:marketing-core',exp:Math.floor(now.getTime()/1000)+300},
    {org_id:'org-aquatiq',aud:'ingestion',principal_type:'user',sub:'another-user',exp:Math.floor(now.getTime()/1000)+300},
    null,
  ]){
    const {options,calls}=fixture(t,[])
    options.execFile=async()=>({stdout:claims?`header.${Buffer.from(JSON.stringify(claims)).toString('base64url')}.signature`:'not-a-token'})
    await assert.rejects(prepareConsent(options),/expired|binding/);assert.equal(calls.length,0)
  }
  const {options,calls}=fixture(t,[])
  options.execFile=async()=>{throw new Error('synthetic private subprocess secret')}
  await assert.rejects(prepareConsent(options),error=>/privately/.test(error.message)&&!error.message.includes('secret'))
  assert.equal(calls.length,0)
})

test('prepare requires explicit org and a private local directory',async(t)=>{
  const {options,calls,root}=fixture(t,[])
  await assert.rejects(prepareConsent({...options,organizationId:'not a valid org'}),/organization ID/)
  chmodSync(root,0o755)
  await assert.rejects(prepareConsent(options),/mode-0700/);assert.equal(calls.length,0)
})

test('prepare handles refused, redirected, malformed, and oversized Integration responses without secret output',async(t)=>{
  for(const response of [new Response('private upstream detail',{status:403}),new Response('private redirect detail',{status:302,headers:{location:'https://evil.example'}}),new Response('not-json'),new Response(JSON.stringify({success:false,data:{private:'value'}})),new Response('{}',{headers:{'content-length':String(2*1024*1024+1)}}),new Response('x'.repeat(2*1024*1024+1))]){
    const {options}=fixture(t,[]);options.fetch=async()=>response
    await assert.rejects(prepareConsent(options),error=>/Integration/.test(error.message)&&!error.message.includes('private'))
    assert.throws(()=>readFileSync(options.sessionFile))
  }
  const {options}=fixture(t,[]);options.fetch=async()=>{throw new Error('private network failure')}
  await assert.rejects(prepareConsent(options),error=>error.message==='Integration Core request failed')
})

test('authorization refuses missing capabilities, expired state, wrong callback host and malformed Google parameters',()=>{
  for(const mutate of [
    value=>value.capabilities=['profile.read'],
    value=>value.scopes=['openid','profile','email'],
    value=>value.expiresAt=new Date(now.getTime()-1).toISOString(),
    value=>value.expiresAt=new Date(now.getTime()+3600001).toISOString(),
    value=>value.provider.key='microsoft',
    value=>value.sessionToken='bad token',
    value=>value.authorizationUrl='not-a-url',
    value=>{const url=new URL(value.authorizationUrl);url.pathname='/wrong';value.authorizationUrl=url.href},
    value=>{const url=new URL(value.authorizationUrl);url.searchParams.set('redirect_uri','http://127.0.0.1:3026/oauth/callback/google');value.authorizationUrl=url.href},
    value=>{const url=new URL(value.authorizationUrl);url.searchParams.set('response_type','token');value.authorizationUrl=url.href},
    value=>{const url=new URL(value.authorizationUrl);url.searchParams.delete('state');value.authorizationUrl=url.href},
    value=>{const url=new URL(value.authorizationUrl);url.searchParams.delete('client_id');value.authorizationUrl=url.href},
    value=>{const url=new URL(value.authorizationUrl);url.searchParams.set('scope',scopeRead);value.authorizationUrl=url.href},
  ]){const value=bundle();mutate(value);assert.throws(()=>validateAuthorization(value,now))}
})

test('serve refuses cross-origin requests, wrong methods, and expired state',()=>{
  function invoke(handler,request){const response={statusCode:0,headers:{},setHeader(name,value){this.headers[name]=value},end(value){this.body=value}};handler(request,response);return response}
  const handler=createConsentHandler(bundle(),()=>now)
  assert.equal(invoke(handler,{method:'GET',url:'/connect',headers:{host:'localhost:33027',origin:'https://evil.example'}}).statusCode,403)
  assert.equal(invoke(handler,{method:'POST',url:'/connect',headers:{host:'localhost:33027'}}).statusCode,404)
  assert.equal(invoke(createConsentHandler(bundle(),()=>new Date(now.getTime()+600001)),{method:'GET',url:'/connect',headers:{host:'127.0.0.1:33027'}}).statusCode,410)
})

const completedStatus=()=>({success:true,data:{id:'session-test-123',providerKey:'google',status:'completed',consumedAt:now.toISOString(),errorCode:''}})
const activeConnection=()=>({id:'google-connection-new',organizationId:'org-aquatiq',providerKey:'google',status:'active',capabilities:['profile.read','search_console.read'],createdAt:now.toISOString()})
function savedConsent(options,overrides={}){writeFileSync(options.sessionFile,JSON.stringify({...bundle(),organizationId:'org-aquatiq',preparedAt:now.toISOString(),existingConnectionIds:[],...overrides}),{mode:0o600})}

test('finalize rejects unsafe private state, symlinks, and mismatched saved organization',async(t)=>{
  const {options,root}=fixture(t,[])
  const original=join(root,'original-state.json');writeFileSync(original,JSON.stringify(bundle()),{mode:0o600});symlinkSync(original,options.sessionFile)
  await assert.rejects(finalizeConsent(options),/protected/)
  rmSync(options.sessionFile);savedConsent(options);chmodSync(options.sessionFile,0o644)
  await assert.rejects(finalizeConsent(options),/protected/)
  chmodSync(options.sessionFile,0o600);savedConsent(options,{organizationId:'another-org'})
  await assert.rejects(finalizeConsent(options),/organization/)
  assert.throws(()=>readFileSync(options.configFile))
})

test('finalize rejects failed or mismatched completion receipts',async(t)=>{
  for(const values of [{id:'another-session'},{providerKey:'meta'},{status:'failed',errorCode:'denied'},{consumedAt:null}]){
    const status=completedStatus();Object.assign(status.data,values)
    const {options}=fixture(t,[status]);savedConsent(options)
    await assert.rejects(finalizeConsent(options),/completed/);assert.throws(()=>readFileSync(options.configFile))
  }
})

test('finalize refuses missing, ambiguous, preexisting, broader-provider, and unauthorized connection candidates',async(t)=>{
  for(const connections of [[],[activeConnection(),{...activeConnection(),id:'another-new-google'}],[{...activeConnection(),organizationId:'another-org'}],[{...activeConnection(),providerKey:'meta'}],[{...activeConnection(),status:'needs_refresh'}],[{...activeConnection(),deletedAt:now.toISOString()}],[{...activeConnection(),capabilities:['profile.read','gmail.read']}],[{...activeConnection(),createdAt:new Date(now.getTime()-60000).toISOString()}]]){
    const {options}=fixture(t,[completedStatus(),{success:true,data:{connections}}]);savedConsent(options)
    await assert.rejects(finalizeConsent(options),/unique new active/);assert.throws(()=>readFileSync(options.configFile))
  }
  const {options}=fixture(t,[completedStatus(),{success:true,data:{connections:[activeConnection()]}}]);savedConsent(options,{existingConnectionIds:[activeConnection().id]})
  await assert.rejects(finalizeConsent(options),/unique new active/)
})

test('finalize requires exact aquatiq host and verified Search Console property permissions',async(t)=>{
  for(const action of [
    {providerKey:'meta',operation:'gsc.sites.list',result:{siteEntry:[]}},
    {providerKey:'google',operation:'different-operation',result:{siteEntry:[]}},
    {providerKey:'google',operation:'gsc.sites.list',result:{}},
    {providerKey:'google',operation:'gsc.sites.list',result:{siteEntry:[{siteUrl:'sc-domain:another-domain.com',permissionLevel:'siteOwner'}]}},
    {providerKey:'google',operation:'gsc.sites.list',result:{siteEntry:[{siteUrl:'https://aquatiq.com/path',permissionLevel:'siteOwner'}]}},
    {providerKey:'google',operation:'gsc.sites.list',result:{siteEntry:[{siteUrl:'sc-domain:aquatiq.com',permissionLevel:'siteUnverifiedUser'}]}},
  ]){
    const {options}=fixture(t,[completedStatus(),{success:true,data:{connections:[activeConnection()]}},{success:true,data:{action}}]);savedConsent(options)
    await assert.rejects(finalizeConsent(options),/verification response|verified aquatiq/);assert.throws(()=>readFileSync(options.configFile))
  }
})
