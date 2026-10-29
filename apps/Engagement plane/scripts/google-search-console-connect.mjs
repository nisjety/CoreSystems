#!/usr/bin/env node
// Local consent operator. All provider token handling remains in Integration
// Core; this process holds only the org bearer and temporary authorization URL.
import { execFile as execFileCallback } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { constants, closeSync, fstatSync, lstatSync, openSync, readFileSync, renameSync, unlinkSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, isAbsolute, join, normalize, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const scopeRead = 'https://www.googleapis.com/auth/webmasters.readonly'
const allowedScopes = new Set(['openid', 'profile', 'email', 'https://www.googleapis.com/auth/userinfo.email', 'https://www.googleapis.com/auth/userinfo.profile', scopeRead])
const capabilities = ['profile.read', 'search_console.read']
const identity = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/
const defaultSession = join(root, '.local/google-search-console-connect.json')

function privateDirectory(path) {
  let info
  try { info = lstatSync(path) } catch {}
  if (!isAbsolute(path) || normalize(path) !== path || !info?.isDirectory() || (info.mode & 0o777) !== 0o700 || (process.getuid && info.uid !== process.getuid())) throw new Error('Consent directory must be an owned mode-0700 directory')
}

function privateRead(path) {
  let descriptor
  try {
    const original = lstatSync(path)
    if (!original.isFile() || (original.mode & 0o777) !== 0o600 || original.size > 65536 || (process.getuid && original.uid !== process.getuid())) throw new Error()
    descriptor = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW)
    const current = fstatSync(descriptor)
    if (original.ino !== current.ino || original.dev !== current.dev) throw new Error()
    return JSON.parse(readFileSync(descriptor, 'utf8'))
  } catch { throw new Error('Consent state must be a protected mode-0600 regular JSON file') }
  finally { if (descriptor !== undefined) closeSync(descriptor) }
}

function privateSave(path, value) {
  privateDirectory(dirname(path))
  try { const info=lstatSync(path); if (!info.isFile() || (info.mode&0o777)!==0o600 || (process.getuid && info.uid!==process.getuid())) throw new Error('Existing output is not a protected regular file') }
  catch(error) { if(error.code!=='ENOENT')throw error }
  const temporary=join(dirname(path),`.consent-${randomBytes(12).toString('hex')}`)
  try { writeFileSync(temporary,JSON.stringify(value,null,2)+'\n',{mode:0o600,flag:'wx'});renameSync(temporary,path) }
  catch { throw new Error('Could not save protected consent state') }
  finally { try{unlinkSync(temporary)}catch{} }
}

function validOrg(value) {
  if (typeof value!=='string'||!identity.test(value))throw new Error('An explicit valid organization ID is required')
  return value
}

function optionsWithDefaults(options={}) {
  return {sessionFile:defaultSession,configFile:join(root,'.local/marketing-integration.json'),now:()=>new Date(),fetch:globalThis.fetch,execFile:promisify(execFileCallback),...options}
}

async function bearer(options,org) {
  let token
  try { const result=await options.execFile('docker',['exec','engagement-plane-marketing-bearer-refresh-1','cat','/run/marketing-bearer/token'],{encoding:'utf8',maxBuffer:8192,timeout:10000});token=result.stdout.trim() }
  catch { throw new Error('Could not read the running Marketing bearer privately') }
  try {
    if (token.length>8192||/\s/.test(token)||token.split('.').length!==3)throw new Error()
    const claims=JSON.parse(Buffer.from(token.split('.')[1],'base64url').toString('utf8'))
    if(claims.org_id!==org||claims.aud!=='ingestion'||claims.principal_type!=='service'||claims.sub!=='service:marketing-core'||claims.exp*1000<=options.now().getTime()+15000)throw new Error()
  }catch{throw new Error('Marketing bearer is expired or has the wrong organization binding')}
  return token
}

async function api(options,token,path,body) {
  let response
  try { response=await options.fetch(`http://localhost:3026${path}`,{method:body===undefined?'GET':'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)}),redirect:'manual',signal:AbortSignal.timeout(15000)}) }
  catch{throw new Error('Integration Core request failed')}
  if(!response.ok)throw new Error(`Integration Core request was refused (${response.status})`)
  const length=Number(response.headers.get('content-length')??0)
  if(length>2*1024*1024)throw new Error('Integration response is too large')
  const reader=response.body.getReader();const chunks=[];let size=0
  try { for(;;){const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>2*1024*1024){await reader.cancel();throw new Error()}chunks.push(value)} }
  catch{throw new Error('Integration response is unreadable or too large')}
  let envelope
  try{envelope=JSON.parse(Buffer.concat(chunks).toString('utf8'))}catch{throw new Error('Integration response is invalid')}
  if(envelope.success!==true||!envelope.data||typeof envelope.data!=='object')throw new Error('Integration response is invalid')
  return envelope.data
}

export function validateAuthorization(value,now=new Date()) {
  if(!value||!identity.test(value.sessionToken??'')||value.provider?.key!=='google'||!Array.isArray(value.scopes)||!value.scopes.includes(scopeRead)||value.scopes.some(scope=>!allowedScopes.has(scope))||!Array.isArray(value.capabilities)||value.capabilities.length!==2||capabilities.some(key=>!value.capabilities.includes(key)))throw new Error('Consent must request only Google profile and read-only Search Console access')
  const expires=Date.parse(value.expiresAt)
  if(!Number.isFinite(expires)||expires<=now.getTime()||expires>now.getTime()+3600000)throw new Error('Consent session is expired or invalid')
  let url
  try{url=new URL(value.authorizationUrl)}catch{throw new Error('Consent authorization URL is invalid')}
  if(url.origin!=='https://accounts.google.com'||url.username||url.password||url.hash||!['/o/oauth2/v2/auth','/o/oauth2/auth'].includes(url.pathname))throw new Error('Consent must use the exact Google authorization origin')
  const parameters=url.searchParams
  if(parameters.get('redirect_uri')!=='http://localhost:3026/oauth/callback/google'||parameters.get('response_type')!=='code'||parameters.get('code_challenge_method')!=='S256'||!/^[A-Za-z0-9_-]{43}$/.test(parameters.get('code_challenge')??'')||!/^[A-Za-z0-9_-]{32,128}$/.test(parameters.get('state')??'')||!parameters.get('client_id')?.endsWith('.apps.googleusercontent.com'))throw new Error('Consent callback or PKCE configuration does not match local Integration Core')
  const urlScopes=parameters.get('scope')?.split(/\s+/)??[]
  if(urlScopes.length!==value.scopes.length||urlScopes.some(scope=>!allowedScopes.has(scope))||value.scopes.some(scope=>!urlScopes.includes(scope)))throw new Error('Authorization URL requests unexpected scopes')
  return value
}

export async function prepareConsent(input) {
  const options=optionsWithDefaults(input);const organizationId=validOrg(options.organizationId)
  privateDirectory(dirname(options.sessionFile))
  const token=await bearer(options,organizationId)
  const existing=await api(options,token,`/api/v1/connections?providerKey=google&organizationId=${encodeURIComponent(organizationId)}`)
  if(!Array.isArray(existing.connections))throw new Error('Integration connections response is invalid')
  const session=validateAuthorization(await api(options,token,'/api/v1/providers/google/connect-session',{organizationId,bundles:['search_console'],returnUrl:'http://127.0.0.1:5174/studio/pulse'}),options.now())
  privateSave(options.sessionFile,{version:1,organizationId,preparedAt:options.now().toISOString(),existingConnectionIds:existing.connections.map(value=>value.id),sessionToken:session.sessionToken,authorizationUrl:session.authorizationUrl,expiresAt:session.expiresAt,provider:session.provider,capabilities:session.capabilities,scopes:session.scopes})
  return {status:'consent_prepared',sessionFile:options.sessionFile,scopes:session.scopes,expiresAt:session.expiresAt}
}

export function createConsentHandler(session,now=()=>new Date()) {
  return (request,response)=>{
    response.setHeader('Cache-Control','no-store');response.setHeader('Referrer-Policy','no-referrer');response.setHeader('X-Content-Type-Options','nosniff')
    const host=request.headers.host
    if(!['localhost:33027','127.0.0.1:33027'].includes(host)||request.headers['sec-fetch-site']==='cross-site'||(request.headers.origin&&request.headers.origin!==`http://${host}`)){response.statusCode=403;response.end('Local consent navigation required');return}
    if(request.method!=='GET'||request.url!=='/connect'){response.statusCode=404;response.end('Not found');return}
    try{validateAuthorization(session,now())}catch{response.statusCode=410;response.end('Consent session expired; prepare a new session');return}
    response.statusCode=302;response.setHeader('Location',session.authorizationUrl);response.end()
  }
}

export async function finalizeConsent(input) {
  const options=optionsWithDefaults(input);const session=privateRead(options.sessionFile)
  const organizationId=validOrg(options.organizationId??session.organizationId)
  if(session.organizationId!==organizationId||!identity.test(session.sessionToken??'')||!Array.isArray(session.existingConnectionIds))throw new Error('Saved consent organization or session is invalid')
  const token=await bearer(options,organizationId)
  const status=await api(options,token,`/api/v1/connect-sessions/${encodeURIComponent(session.sessionToken)}/status`)
  if(status.id!==session.sessionToken||status.providerKey!=='google'||status.status!=='completed'||status.errorCode||!status.consumedAt)throw new Error('The saved Google consent session has not completed successfully')
  const connections=await api(options,token,`/api/v1/connections?providerKey=google&organizationId=${encodeURIComponent(organizationId)}`)
  if(!Array.isArray(connections.connections))throw new Error('Integration connections response is invalid')
  const candidates=connections.connections.filter(value=>identity.test(value.id??'')&&value.organizationId===organizationId&&value.providerKey==='google'&&value.status==='active'&&!value.deletedAt&&value.capabilities?.includes('search_console.read')&&!session.existingConnectionIds.includes(value.id)&&Date.parse(value.createdAt)>=Date.parse(session.preparedAt)-5000)
  if(candidates.length!==1)throw new Error('A unique new active Google Search Console connection could not be verified')
  const googleConnectionId=candidates[0].id
  const action=(await api(options,token,`/api/v1/connections/${encodeURIComponent(googleConnectionId)}/actions`,{operation:'gsc.sites.list'})).action
  if(action?.providerKey!=='google'||action.operation!=='gsc.sites.list'||!Array.isArray(action.result?.siteEntry))throw new Error('Search Console verification response is invalid')
  const property=action.result.siteEntry.find(value=>['sc-domain:aquatiq.com','https://aquatiq.com/'].includes(value.siteUrl)&&['siteOwner','siteFullUser','siteRestrictedUser'].includes(value.permissionLevel))
  if(!property)throw new Error('The Google account does not expose a verified aquatiq.com property')
  const config={version:1,organizationId,googleConnectionId};privateSave(options.configFile,config)
  return {status:'connection_verified',...config,property:property.siteUrl,configFile:options.configFile}
}

async function main() {
  const [command,...args]=process.argv.slice(2);let organizationId
  if(args.length){if(args.length!==2||args[0]!=='--org')throw new Error('Usage: prepare --org ORGANIZATION_ID | serve | finalize [--org ORGANIZATION_ID]');organizationId=args[1]}
  if(command==='prepare'){console.log(JSON.stringify(await prepareConsent({organizationId})));return}
  if(command==='finalize'){console.log(JSON.stringify(await finalizeConsent({organizationId})));return}
  if(command==='serve'){
    const session=privateRead(defaultSession);validateAuthorization(session)
    const server=createServer(createConsentHandler(session));server.listen(33027,'127.0.0.1',()=>console.log('Google consent ready at http://localhost:33027/connect'))
    server.on('error',()=>{console.error('Could not start the local consent redirect server');process.exitCode=1})
    for(const signal of ['SIGINT','SIGTERM'])process.on(signal,()=>server.close())
    return
  }
  throw new Error('Usage: prepare --org ORGANIZATION_ID | serve | finalize [--org ORGANIZATION_ID]')
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))main().catch(error=>{console.error(error.message);process.exitCode=1})
