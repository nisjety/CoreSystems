#!/usr/bin/env node
// Local deployment provisioning only: never contacts a provider or issues JWTs.
import { randomBytes } from 'node:crypto'
import { closeSync, constants, fstatSync, lstatSync, openSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join, normalize, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isDeepStrictEqual } from 'node:util'

const maximumBytes = 1024 * 1024
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const identityPattern = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/

function ownedPrivate(stats, mode) {
  return (stats.mode & 0o777) === mode && (typeof process.getuid !== 'function' || stats.uid === process.getuid())
}

function privateDirectory(path) {
  let stats
  try { stats = lstatSync(path) } catch { /* handled below */ }
  if (!isAbsolute(path) || normalize(path) !== path || basename(path) !== 'secrets' || !stats?.isDirectory() || !ownedPrivate(stats, 0o700)) {
    throw new Error('Use an owned mode-0700 private secrets directory named secrets')
  }
}

function privateFile(path) {
  let descriptor
  try {
    const original = lstatSync(path)
    if (!original.isFile() || !ownedPrivate(original, 0o600) || original.size > maximumBytes) throw new Error()
    descriptor = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW)
    const opened = fstatSync(descriptor)
    if (opened.dev !== original.dev || opened.ino !== original.ino || !ownedPrivate(opened, 0o600) || opened.size > maximumBytes) throw new Error()
    return readFileSync(descriptor, 'utf8')
  } catch {
    throw new Error('Secret input must be an owned mode-0600 private regular file')
  } finally {
    if (descriptor !== undefined) closeSync(descriptor)
  }
}

function exists(path) {
  try { lstatSync(path); return true } catch (error) {
    if (error.code === 'ENOENT') return false
    throw new Error('Could not inspect protected output')
  }
}

function stringList(value) {
  return Array.isArray(value) && value.length > 0 && value.every((entry) => typeof entry === 'string' && entry.length > 0 && entry === entry.trim()) && new Set(value).size === value.length
}

function validPrincipal(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || typeof value.credential !== 'string' || value.credential.length < 32 || value.credential.length > 512 || !stringList(value.audiences) || !stringList(value.scopes) || !Array.isArray(value.orgIds)) return false
  if (value.orgIds.some((id) => typeof id !== 'string' || !identityPattern.test(id)) || (!value.allowAnyOrg && value.orgIds.length === 0)) return false
  if (value.allowAnyOrg !== undefined && typeof value.allowAnyOrg !== 'boolean') return false
  if (value.scopesByAudience !== undefined) {
    const entries = Object.entries(value.scopesByAudience ?? {})
    if (entries.length !== value.audiences.length || entries.some(([audience, scopes]) => !value.audiences.includes(audience) || !stringList(scopes) || scopes.some((scope) => !value.scopes.includes(scope)))) return false
    const mapped = new Set(entries.flatMap(([, scopes]) => scopes))
    if (mapped.size !== value.scopes.length) return false
  }
  if (value.retentionByAudience !== undefined) {
    const entries = Object.entries(value.retentionByAudience ?? {})
    if (entries.length !== value.audiences.length || entries.some(([audience, posture]) => !value.audiences.includes(audience) || !['persistent', 'zdr'].includes(posture))) return false
  }
  return true
}

function parseRegistry(raw) {
  let registry
  try { registry = JSON.parse(raw) } catch { throw new Error('The protected service registry is not valid JSON') }
  if (!registry || typeof registry !== 'object' || Array.isArray(registry) || Object.keys(registry).length > 128 || Object.entries(registry).some(([name, principal]) => !identityPattern.test(name) || !validPrincipal(principal))) {
    throw new Error('The protected service registry contains an invalid principal')
  }
  return registry
}

function registryFromEnvironment(path) {
  const entries = privateFile(path).split(/\r?\n/).filter((line) => line.startsWith('PLANE_SERVICE_PRINCIPALS_JSON='))
  if (entries.length !== 1) throw new Error('Expected exactly one service registry in the protected deployment source')
  let raw = entries[0].slice(entries[0].indexOf('=') + 1).trim()
  if (raw.startsWith("'") && raw.endsWith("'")) raw = raw.slice(1, -1)
  return parseRegistry(raw)
}

function install(path, value) {
  let descriptor
  try {
    descriptor = openSync(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    writeFileSync(descriptor, value, 'utf8')
  } catch {
    throw new Error('Could not install protected output without overwriting an existing file')
  } finally {
    if (descriptor !== undefined) closeSync(descriptor)
  }
}

export function provisionMarketingPrincipal({ organizationId, secretsDirectory = join(studioRoot, '.local/secrets'), sourceEnvFile = join(secretsDirectory, 'control_plane_compose') }) {
  if (typeof organizationId !== 'string' || !identityPattern.test(organizationId)) throw new Error('A valid explicit organization ID is required')
  privateDirectory(secretsDirectory)
  const registry = registryFromEnvironment(sourceEnvFile)
  const keyFile = join(secretsDirectory, 'marketing_plane_service_key')
  const registryFile = join(secretsDirectory, 'marketing_auth_plane_principals.json')
  const keyExists = exists(keyFile)
  const registryExists = exists(registryFile)
  if (!keyExists && (registryExists || Object.hasOwn(registry, 'marketing-core'))) throw new Error('An existing Marketing principal requires its matching protected key')
  const credential = keyExists ? privateFile(keyFile).trim() : randomBytes(32).toString('hex')
  if (!/^[a-f0-9]{64}$/.test(credential)) throw new Error('The existing Marketing principal key is invalid')
  const marketing = {
    credential, audiences: ['ingestion'], orgIds: [organizationId],
    scopes: ['integration:read'], scopesByAudience: { ingestion: ['integration:read'] },
    retentionByAudience: { ingestion: 'persistent' },
  }
  if (Object.hasOwn(registry, 'marketing-core') && !isDeepStrictEqual(registry['marketing-core'], marketing)) throw new Error('The existing Marketing principal has a different credential or authorization')
  const merged = { ...registry, 'marketing-core': marketing }
  if (registryExists && !isDeepStrictEqual(parseRegistry(privateFile(registryFile)), merged)) throw new Error('The existing Marketing principal or merged registry differs from the requested deployment')
  if (!keyExists) install(keyFile, credential)
  if (!registryExists) install(registryFile, JSON.stringify(merged) + '\n')
  return { keyFile, registryFile, organizationId, created: !registryExists }
}

function cli() {
  const options = {}
  const names = { '--org': 'organizationId', '--secrets-dir': 'secretsDirectory', '--source-env': 'sourceEnvFile' }
  for (let index = 2; index < process.argv.length; index += 2) {
    const name = names[process.argv[index]]
    if (!name || options[name] || !process.argv[index + 1]) throw new Error('Usage: provision-marketing-principal.mjs --org ORGANIZATION_ID [--secrets-dir ABSOLUTE_DIRECTORY] [--source-env PROTECTED_FILE]')
    options[name] = process.argv[index + 1]
  }
  const result = provisionMarketingPrincipal(options)
  console.log(JSON.stringify(result))
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { cli() } catch (error) { console.error(error.message); process.exitCode = 1 }
}
