#!/usr/bin/env node
// Local-only canonical Compose runner. Reads credentials as data, never shell code.
import { constants, closeSync, existsSync, fstatSync, lstatSync, openSync, readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const controlNames = ['SESSION_CORE_SERVICE_TOKEN', 'USER_CORE_GATEWAY_TOKEN',
  'GATEWAY_AUDIT_CORE_SERVICE_TOKEN', 'GATEWAY_BILLING_CORE_SERVICE_TOKEN',
  'GATEWAY_ORG_CORE_SERVICE_TOKEN', 'VEREVON_GATEWAY_GDPR_NATS_PASSWORD']
const localNames = ['INTERNAL_API_KEY', 'CONVERSATION_GATEWAY_SERVICE_TOKEN',
  'NOTIFICATION_GATEWAY_SERVICE_TOKEN', 'VEREVON_NATS_TOKEN']
const leafNames = ['studio', 'control', 'ingestion']

function protectedText(file) {
  let descriptor
  try {
    descriptor = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK)
    const info = fstatSync(descriptor)
    if (!info.isFile() || (info.mode & 0o777) !== 0o600 || info.uid !== process.getuid() || info.size > 1024 * 1024) throw new Error()
    return readFileSync(descriptor, 'utf8')
  } catch { throw new Error('Required protected deployment input is missing or unsafe') }
  finally { if (descriptor !== undefined) closeSync(descriptor) }
}

export function readSelectedEnvironment(file, names) {
  const allowed = new Set(names)
  return protectedText(file).split(/\r?\n/).reduce((result, line) => {
    const separator = line.indexOf('=')
    const name = line.slice(0, separator)
    if (separator < 0 || !allowed.has(name)) return result
    const value = line.slice(separator + 1)
    if (name in result || !value || /[\s"'`$\\\x00-\x1f\x7f]/.test(value)) throw new Error('Selected deployment value must be a unique non-empty literal')
    return { ...result, [name]: value }
  }, {})
}

/** Mirror Studio's canonical single-org deployment, never a browser choice or
 * fallback. Missing configuration keeps the API honestly unavailable. */
export function readMarketingDeployment(studioRoot) {
  const configuration = join(studioRoot, '.local', 'marketing-integration.json')
  if (!existsSync(configuration)) {
    // A broken symlink is not an absent optional configuration.
    try { if (lstatSync(configuration).isSymbolicLink()) throw new Error('Unsafe marketing deployment scope') }
    catch (error) { if (error.code !== 'ENOENT') throw error }
    return {}
  }
  let settings
  try { settings = JSON.parse(protectedText(configuration)) } catch { throw new Error('Invalid protected marketing deployment scope') }
  if (settings.version !== 1 || typeof settings.organizationId !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(settings.organizationId)) throw new Error('Invalid marketing deployment scope')
  const key = protectedText(join(studioRoot, '.local', 'secrets', 'marketing_internal_api_key')).trim()
  if (!/^[A-Za-z0-9._~-]{16,256}$/.test(key)) throw new Error('Invalid protected marketing deployment credential')
  return { MARKETING_CORE_SERVICE_KEY: key, MARKETING_CORE_ORGANIZATION_ID: settings.organizationId }
}

/** The anbud core's key is Studio's internal key, read from protected Studio secrets as the Marketing credential is.
 * Absent, the tender routes answer not_configured. */
export function readAnbudDeployment(studioRoot) {
  const file = join(studioRoot, '.local', 'secrets', 'anbud_internal_api_key')
  if (!existsSync(file)) return {}
  const key = protectedText(file).trim()
  if (!/^[A-Za-z0-9._~-]{16,256}$/.test(key)) throw new Error('Invalid protected anbud deployment credential')
  return { ANBUD_API_KEY: key }
}

export function frontendEnvironment({ ambient, local, control, ingestion }) {
  const processNames = ['PATH', 'HOME', 'TMPDIR', 'DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_CONFIG', 'SSH_AUTH_SOCK', 'BUILDKIT_PROGRESS']
  const execution = Object.fromEntries(processNames.filter(name => ambient[name]).map(name => [name, ambient[name]]))
  return { ...execution, ...local, ...ingestion, ...control,
    ALLOW_DEV_AUTH_BYPASS: '0', VITE_ALLOW_DEV_AUTH_BYPASS: 'false', CONTROL_PLANE_ENFORCEMENT: 'strict',
    VEREVON_BIND_ADDRESS: '127.0.0.1', VEREVON_PUBLIC_ORIGIN: 'http://localhost:5173',
    VEREVON_GATEWAY_GDPR_NATS_URL: 'nats://control-shared-nats:4222',
    SOURCE_REVISION: 'local-unverified', BUILD_DATE: new Date().toISOString() }
}

export function run(args = process.argv.slice(2), options = {}) {
  const root = options.root ?? resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const secrets = resolve(root, '../../Engagement plane/.local/secrets')
  if (!args.length || args.some(value => /docker-compose.production/.test(value) || /^(?:--env-file|--file|--project-directory|--project-name)(?:=|$)|^-f/.test(value))) throw new Error('Use this local runner with a Compose operation, without alternate configuration inputs')
  if (args.includes('config') && !args.includes('--quiet') && !args.includes('--services')) throw new Error('Use config --quiet or config --services to keep resolved secrets out of output')
  const local = readSelectedEnvironment(join(root, '.env'), localNames)
  const control = readSelectedEnvironment(join(secrets, 'control_plane_compose'), controlNames)
  const marketing = readMarketingDeployment(resolve(root, '../../Engagement plane'))
  for (const name of [...localNames, ...controlNames]) if (!(name in local) && !(name in control)) throw new Error(`Missing required deployment variable: ${name}`)
  const leaves = Object.fromEntries(leafNames.map(plane => {
    const value = protectedText(join(secrets, `shared_nats_${plane}_leaf_password`)).trim()
    if (!/^[A-Za-z0-9_-]{32,256}$/.test(value)) throw new Error('Leaf deployment credential has an invalid format')
    return [`NATS_${plane.toUpperCase()}_LEAF_PASSWORD`, value]
  }))
  const selected = { ...frontendEnvironment({ ambient: process.env, local, control, ingestion: leaves }), ...marketing, ...readAnbudDeployment(resolve(root, '../../Engagement plane')) }
  // NATS expands environment references as configuration tokens. Encode only
  // broker secrets as strings so leading digits/exponents cannot be parsed as
  // numbers. This does not alter the stored credential or other service keys.
  const brokerNames = ['VEREVON_NATS_TOKEN', ...Object.keys(leaves)]
  const env = { ...selected, ...Object.fromEntries(brokerNames.map(name => [name, JSON.stringify(selected[name])])) }
  const result = (options.spawn ?? spawnSync)('docker', ['compose', '--env-file', '/dev/null', '-f', join(root, 'docker-compose.yml'), ...args], { cwd: root, env, stdio: 'inherit' })
  if (result.error) throw new Error('Docker Compose could not be started')
  return result.status ?? 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = run() } catch (error) { console.error(error.message); process.exitCode = 1 }
}
