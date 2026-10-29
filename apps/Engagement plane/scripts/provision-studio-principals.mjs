#!/usr/bin/env node
// Deployment-only command. It never contacts Auth Core/providers or mints JWTs.
// Default is a read-only plan; --apply is the explicit authorization boundary.
import { randomBytes } from 'node:crypto'
import { closeSync, constants, fstatSync, fsyncSync, lstatSync, openSync, readSync, renameSync, unlinkSync, writeFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join, normalize, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isDeepStrictEqual } from 'node:util'

const maximumBytes = 1024 * 1024
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const registryName = 'marketing_auth_plane_principals.json'
const cores = Object.freeze(['marketing-core', 'social-core', 'leads-core', 'anbud-core'])
const capabilities = Object.freeze({ ingestion: ['integration:read', 'integration:write'], quarry: ['map:read', 'scrape:read', 'scrape:write'], 'model-gateway': ['models:invoke'], 'inference-core': ['inference:invoke'] })
const retention = Object.freeze({ ingestion: 'persistent', quarry: 'persistent', 'model-gateway': 'zdr', 'inference-core': 'zdr' })
const identity = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/
const record = value => typeof value === 'object' && value !== null && !Array.isArray(value)
const list = value => Array.isArray(value) && value.length > 0 && value.every(item => typeof item === 'string' && item.length > 0 && item === item.trim() && !/[\x00-\x1f\x7f]/.test(item)) && new Set(value).size === value.length
const keyName = core => `${core.slice(0, -5)}_plane_service_key`
const owned = (stats, mode) => (stats.mode & 0o777) === mode && (typeof process.getuid !== 'function' || stats.uid === process.getuid())

function privateDirectory(path) {
  let stats
  try { stats = lstatSync(path) } catch { /* report only the safe contract below */ }
  if (!isAbsolute(path) || normalize(path) !== path || basename(path) !== 'secrets' || !stats?.isDirectory() || !owned(stats, 0o700)) throw new Error('Use an owned mode-0700 private secrets directory named secrets')
}

function privateFile(path) {
  let descriptor
  try {
    const before = lstatSync(path)
    if (!before.isFile() || !owned(before, 0o600) || before.size > maximumBytes) throw new Error()
    descriptor = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK)
    const after = fstatSync(descriptor)
    if (!after.isFile() || !owned(after, 0o600) || before.dev !== after.dev || before.ino !== after.ino || after.size > maximumBytes) throw new Error()
    const buffer = Buffer.alloc(maximumBytes + 1)
    let length = 0
    while (length < buffer.length) { const read = readSync(descriptor, buffer, length, buffer.length - length, null); if (!read) break; length += read }
    if (length > maximumBytes) throw new Error()
    return buffer.subarray(0, length).toString('utf8')
  } catch { throw new Error('Secret input must be an owned mode-0600 private regular file') }
  finally { if (descriptor !== undefined) closeSync(descriptor) }
}

function exists(path) {
  try { lstatSync(path); return true } catch (error) { if (error.code === 'ENOENT') return false; throw new Error('Could not inspect protected output') }
}

function validPrincipal(principal) {
  if (!record(principal) || typeof principal.credential !== 'string' || principal.credential.length < 16 || principal.credential.length > 512 || !list(principal.audiences) || !list(principal.scopes) || !Array.isArray(principal.orgIds) || principal.orgIds.some(org => typeof org !== 'string' || !identity.test(org)) || (!principal.allowAnyOrg && principal.orgIds.length === 0) || principal.allowAnyOrg !== undefined && typeof principal.allowAnyOrg !== 'boolean' || Object.hasOwn(principal, 'allowPersistentData')) return false
  if (principal.scopesByAudience !== undefined) {
    if (!record(principal.scopesByAudience)) return false
    const entries = Object.entries(principal.scopesByAudience)
    if (entries.length !== principal.audiences.length || entries.some(([audience, scopes]) => !principal.audiences.includes(audience) || !list(scopes) || scopes.some(scope => !principal.scopes.includes(scope)))) return false
    if (new Set(entries.flatMap(([, scopes]) => scopes)).size !== principal.scopes.length) return false
  }
  if (principal.retentionByAudience !== undefined) {
    if (!record(principal.retentionByAudience)) return false
    const entries = Object.entries(principal.retentionByAudience)
    if (entries.length !== principal.audiences.length || entries.some(([audience, posture]) => !principal.audiences.includes(audience) || !['zdr', 'persistent'].includes(posture))) return false
  }
  return true
}

function parseRegistry(raw) {
  let registry
  try { registry = JSON.parse(raw) } catch { throw new Error('The protected service registry is not valid JSON') }
  if (!record(registry) || Object.keys(registry).length > 128 || Object.entries(registry).some(([service, principal]) => !identity.test(service) || !validPrincipal(principal))) throw new Error('The protected service registry contains an invalid principal')
  return registry
}

function targetPrincipal(principal, organizationId) {
  if (!principal) return
  if (principal.allowAnyOrg === true || principal.orgIds.length !== 1 || principal.orgIds[0] !== organizationId) throw new Error('An existing Studio principal has a different organization policy')
  const approvedScopes = Object.values(capabilities).flat()
  if (principal.audiences.some(audience => !Object.hasOwn(capabilities, audience)) || principal.scopes.some(scope => !approvedScopes.includes(scope))) throw new Error('An existing Studio principal has unapproved capabilities')
  if (principal.scopesByAudience && Object.entries(principal.scopesByAudience).some(([audience, scopes]) => scopes.some(scope => !capabilities[audience].includes(scope)))) throw new Error('An existing Studio principal has unapproved audience scopes')
}

function inspect({ organizationId, secretsDirectory = join(root, '.local/secrets') }) {
  if (typeof organizationId !== 'string' || !identity.test(organizationId) || organizationId.includes('*')) throw new Error('A valid explicit organization ID is required')
  privateDirectory(secretsDirectory)
  const registryFile = join(secretsDirectory, registryName)
  const original = privateFile(registryFile)
  const registry = parseRegistry(original)
  const targets = cores.map(core => {
    const previous = registry[core]
    targetPrincipal(previous, organizationId)
    const keyFile = join(secretsDirectory, keyName(core))
    const keyExists = exists(keyFile)
    if (previous && !keyExists) throw new Error('An existing Studio principal requires its matching protected key')
    const credential = keyExists ? privateFile(keyFile).trim() : null
    if (credential !== null && !/^[a-f0-9]{64}$/.test(credential)) throw new Error('An existing Studio service key is invalid')
    if (previous && previous.credential !== credential) throw new Error('An existing Studio principal requires its matching protected key')
    const requested = { ...(previous ?? {}), credential, audiences: Object.keys(capabilities), orgIds: [organizationId], scopes: Object.values(capabilities).flat(), scopesByAudience: structuredClone(capabilities), retentionByAudience: { ...retention } }
    if (requested.allowAnyOrg === false) delete requested.allowAnyOrg
    return { core, keyFile, keyExists, requested, changed: !isDeepStrictEqual(previous, requested) }
  })
  return { organizationId, secretsDirectory, registryFile, original, registry, targets }
}

function summary(state, backupFilename = null) {
  return { created: state.targets.filter(target => !target.keyExists).length, changed: state.targets.filter(target => target.changed).length, capabilities: structuredClone(capabilities), backupFilename }
}

/** Safe plan excludes both registry contents and existing/new credential values. */
export function planStudioPrincipals(options) { return summary(inspect(options)) }

function install(path, contents) {
  let descriptor
  try {
    descriptor = openSync(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    writeFileSync(descriptor, contents, 'utf8'); fsyncSync(descriptor)
  } catch { throw new Error('Could not install protected output without overwriting an existing file') }
  finally { if (descriptor !== undefined) closeSync(descriptor) }
}

/** --apply extends the existing mounted registry atomically. Never rotates keys. */
export function provisionStudioPrincipals(options) {
  if (options.apply !== undefined && typeof options.apply !== 'boolean') throw new Error('Apply must be an explicit boolean')
  if (!options.apply) return planStudioPrincipals(options)
  const planned = inspect(options)
  if (!planned.targets.some(target => target.changed)) return summary(planned)
  const lock = join(planned.secretsDirectory, '.studio-principals.lock')
  install(lock, 'studio-principals provisioning\n')
  let temporary
  try {
    const state = inspect(options)
    if (state.original !== planned.original) throw new Error('The protected service registry changed; plan again')
    const nonce = randomBytes(12).toString('hex')
    const backupFilename = `marketing_auth_plane_principals.backup-${Date.now()}-${nonce}.json`
    install(join(state.secretsDirectory, backupFilename), state.original)
    const merged = { ...state.registry }
    for (const target of state.targets) {
      const credential = target.requested.credential ?? randomBytes(32).toString('hex')
      if (!target.keyExists) install(target.keyFile, credential)
      merged[target.core] = { ...target.requested, credential }
    }
    const encoded = JSON.stringify(merged) + '\n'
    if (Buffer.byteLength(encoded) > maximumBytes) throw new Error('The extended protected registry is too large')
    privateDirectory(state.secretsDirectory)
    if (privateFile(state.registryFile) !== state.original) throw new Error('The protected service registry changed; restore from the backup if needed')
    temporary = join(state.secretsDirectory, `.studio-principals-${nonce}.tmp`)
    install(temporary, encoded)
    renameSync(temporary, state.registryFile)
    temporary = undefined
    return summary(state, backupFilename)
  } finally {
    if (temporary && exists(temporary)) unlinkSync(temporary)
    unlinkSync(lock)
  }
}

function cli() {
  const options = {}
  const names = { '--org': 'organizationId', '--secrets-dir': 'secretsDirectory' }
  for (let index = 2; index < process.argv.length; index++) {
    const argument = process.argv[index]
    if (argument === '--apply' && options.apply === undefined) { options.apply = true; continue }
    const name = names[argument]
    if (!name || options[name] !== undefined || !process.argv[index + 1] || process.argv[index + 1].startsWith('--')) throw new Error('Usage: provision-studio-principals.mjs --org ORGANIZATION_ID [--secrets-dir ABSOLUTE_DIRECTORY] [--apply]')
    options[name] = process.argv[++index]
  }
  console.log(JSON.stringify(provisionStudioPrincipals(options)))
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { cli() } catch (error) { console.error(error.message); process.exitCode = 1 }
}
