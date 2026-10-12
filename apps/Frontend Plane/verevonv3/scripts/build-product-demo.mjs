#!/usr/bin/env node
/**
 * Builds the interactive demo (stage 04 of the product loop) and publishes it as a
 * committed static artifact: apps/verevon-web/public/product-demo, with a manifest at
 * apps/verevon-web/plans/product-demo/manifest.json.
 *
 *   node scripts/build-product-demo.mjs                           build, scan, enforce budgets, publish, write the manifest
 *   node scripts/build-product-demo.mjs --verify                  no build: check the manifest against the sources,
 *                                                                 the published files and apps/verevon-web/next.config.ts
 *   node scripts/build-product-demo.mjs --verify --web-root <dir> the same check for another verevon-web folder
 *
 * Options:
 *   --allow-dirty      local runs only: build although demo inputs have uncommitted changes.
 *                      The manifest then records dirty: true, and --verify refuses it.
 *   --web-root <dir>   verification only. Publishing always writes apps/verevon-web.
 *
 * Publishing always builds vite.demo.config.ts. No option selects another configuration: the manifest
 * could not record which one was used, so --verify could not tell a canonical build from any other.
 *
 * Without --allow-dirty the build refuses to publish while the demo's inputs have uncommitted changes,
 * so another session's work in progress never ships unnoticed.
 *
 * Text files are hashed and published with LF line endings, whatever core.autocrlf says about the
 * checkout, so the manifest is the same on Windows and on Linux (the CI job).
 *
 * The scan refuses forbidden names, absolute URLs to third-party hosts or to /api, root-relative
 * references to files (the demo is served under /product-demo/, so they would load from the site root),
 * personal e-mail addresses and secret-shaped strings. Relative /api/ paths are expected: the product
 * calls them and the demo answers them in memory. The publish also refuses a poster still that is missing or
 * older than the newest demo source, because such a still can show cases the demo no longer has.
 */
import { createHash } from 'node:crypto'
import { execFile } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { brotliCompressSync, constants as zlibConstants } from 'node:zlib'
import { build } from 'vite'

const execFileAsync = promisify(execFile)
const SPA_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const DEFAULT_WEB_ROOT = join(SPA_ROOT, 'apps', 'verevon-web')
const DEFAULT_CONFIG = join(SPA_ROOT, 'vite.demo.config.ts')
const BUILD_DIR = join(SPA_ROOT, 'node_modules', '.cache', 'product-demo', 'dist')
const INPUT_PATHS = [
  'src',
  'package.json',
  'pnpm-lock.yaml',
  'tsconfig.app.json',
  'vite.demo.config.ts',
  'scripts/build-product-demo.mjs',
]
const TEST_FILE = /\.(test|spec)\.[cm]?[jt]sx?$/
const TEXT_FILE = /\.(html|js|css|svg|json|txt|webmanifest|map)$/i
/** Files whose line endings are folded to LF before they are hashed or published (see canonicalBytes). */
const LINE_ENDED_TEXT = /\.(?:[cm]?[jt]sx?|json|css|html?|svg|md|txt|ya?ml|webmanifest|map)$/i
const KIB = 1024

/**
 * Budgets enforced at build time. The probe enforces the two cold-path budgets at run time.
 * The stylesheet budget is larger than the script budget because global.css is one product file
 * (about 866 KiB raw, about 103 KiB brotli) that the demo does not split.
 */
export const BUDGETS = {
  maxFiles: 120,
  maxTotalBytes: 3.5 * 1024 * 1024,
  maxFileBytes: 700 * KIB,
  maxCssFileBytes: 900 * KIB,
  maxEntryBrotliBytes: 280 * KIB,
  maxColdRequests: 25,
  maxColdBrotliBytes: 500 * KIB,
}

/** Headers and rewrites of the demo. next.config.ts must carry the same text (checked by --verify). */
export const HEADER_RULES = [
  {
    source: '/product-demo/:path*',
    headers: [
      {
        key: 'Content-Security-Policy',
        value: "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'none'; media-src 'self' blob:; worker-src 'none'; frame-src 'none'; object-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'",
      },
      { key: 'X-Robots-Tag', value: 'noindex, nofollow' },
      { key: 'X-Content-Type-Options', value: 'nosniff' },
      { key: 'Referrer-Policy', value: 'no-referrer' },
      {
        key: 'Permissions-Policy',
        value: 'geolocation=(), microphone=(), camera=(), payment=(), usb=(), serial=(), bluetooth=(), hid=(), midi=(), magnetometer=(), gyroscope=(), accelerometer=(), display-capture=()',
      },
      { key: 'Access-Control-Allow-Origin', value: '*' },
      { key: 'Cache-Control', value: 'public, max-age=0, must-revalidate' },
    ],
  },
  {
    source: '/product-demo/assets/:path*',
    headers: [{ key: 'Cache-Control', value: 'public, max-age=31536000, immutable' }],
  },
]
export const REWRITES = [{ source: '/product-demo', destination: '/product-demo/index.html' }]

/**
 * Text the published bundle must never contain: the owner's names, the retired film cases (PLAN_2026-10-11 and
 * CASES_2026-10-12) and the development leftovers. The retired films are identified by their company name and
 * their case numbers, not by ordinary words: «Bring» (a verb in the product's English copy and a carrier in a
 * hidden tool description) and «faktura» (Norwegian for invoice, in product navigation) say nothing about a case.
 * The numbers match when no digit touches them, so an identifier such as run_nordlys_4821 is a hit and a CSS
 * value such as .48216em is not.
 */
const FORBIDDEN_TEXT = [
  { rule: 'name Aquatiq', pattern: /aquatiq/i },
  { rule: 'name Velion', pattern: /velion/i },
  { rule: 'name Ima', pattern: /\bIma\b/ },
  { rule: 'name Nordlys', pattern: /nordlys/i },
  { rule: 'retired case number: 4821', pattern: /(?<!\d)4821(?!\d)/ },
  { rule: 'retired case number: 20471', pattern: /(?<!\d)20471(?!\d)/ },
  { rule: 'name CoresSystem', pattern: /coresystem/i },
  { rule: 'development host localhost:5173', pattern: /localhost:5173/ },
  { rule: 'marketing domain verevon.com', pattern: /verevon\.com/i },
  { rule: 'development HMR client', pattern: /@vite\/client/ },
  { rule: 'source map reference', pattern: /sourceMappingURL=/ },
]
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g
const ALLOWED_EMAIL_SUFFIX = '@fjordform.example'
/** The SIL Open Font License text that ships with the self-hosted Geist font. */
const LICENCE_FILE = /(^|\/)OFL\.txt$/
const ABSOLUTE_URL = /\b(?:https?|wss?):\/\/[^\s"'`<>\\)\]}]+/g
/**
 * Absolute URLs the bundle may contain. Each rule says why the string is not a request the demo makes.
 * Anything else is a finding, and an allowlist entry is never added to make a finding go away.
 */
const ALLOWED_URL_RULES = [
  { host: 'www.w3.org', why: 'XML namespace identifiers' },
  { host: 'purl.org', why: 'metadata namespace identifiers' },
  { host: 'json-schema.org', why: 'schema identifiers' },
  { host: 'schemas.openxmlformats.org', why: 'office document namespaces' },
  { host: 'www.googleapis.com', pathPrefix: '/auth/', why: 'OAuth scope identifiers of the integrations catalogue, never requested in the demo' },
]
/**
 * Root-relative references to files. The demo is served under /product-demo/, so a reference that starts
 * at the site root (for example /imagens/x.jpeg) would load from the marketing site instead. Vite rewrites
 * the references it resolves; the rest are findings. The patterns exclude protocol-relative // and the
 * demo's own folder.
 */
const ROOT_RELATIVE_REFERENCES = [
  { rule: 'root-relative url() in a stylesheet or script', pattern: /url\(\s*(['"]?)\/(?![/])(?!product-demo\/)/gi },
  { rule: 'root-relative src or href in an HTML document', pattern: /\b(?:src|href)\s*=\s*(['"])\/(?![/])(?!product-demo\/)/gi, htmlOnly: true },
  {
    rule: 'root-relative file path in a string',
    pattern: /(['"`])\/(?![/])(?!product-demo\/)[^'"`\s]*\.(?:avif|css|gif|ico|jpe?g|js|json|mp3|mp4|ogg|otf|png|svg|ttf|webm|webp|woff2?)(?:[?#][^'"`\s]*)?\1/gi,
  },
]
/** Reserved names (RFC 2606, RFC 6761) are fictional by definition and cannot be registered as real sites. */
const RESERVED_SUFFIXES = ['.example', '.invalid', '.test', '.local']
/** Placeholder origins the router and the server-action helper use only to parse relative paths. */
const PARSING_BASE_HOSTS = new Set(['sr', 'action'])
const SECRET_SHAPES = [
  { rule: 'private key block', pattern: /-----BEGIN [A-Z ]*PRIVATE KEY-----/ },
  { rule: 'AWS access key id', pattern: /\bAKIA[0-9A-Z]{16}\b/ },
  { rule: 'GitHub token', pattern: /\bgh[pousr]_[A-Za-z0-9]{36,}\b/ },
  { rule: 'Slack token', pattern: /\bxox[abprs]-[A-Za-z0-9-]{10,}/ },
  { rule: 'live API key', pattern: /\b(?:sk|rk)_live_[A-Za-z0-9]{16,}\b/ },
  { rule: 'JSON web token', pattern: /\beyJ[A-Za-z0-9_-]{15,}\.eyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}/ },
]

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex')
const toPosix = (path) => path.split(sep).join('/')

/**
 * The bytes a file is hashed and published with. Text has its CR LF pairs folded to LF, so a checkout with
 * core.autocrlf=true hashes the same as one with LF. Latin-1 keeps every byte, so only those pairs change.
 */
export function canonicalBytes(relPath, bytes) {
  if (!LINE_ENDED_TEXT.test(relPath) || !bytes.includes(0x0d)) return bytes
  return Buffer.from(bytes.toString('latin1').replace(/\r\n/g, '\n'), 'latin1')
}

function publishPaths(webRoot) {
  return {
    webRoot,
    outDir: join(webRoot, 'public', 'product-demo'),
    manifestPath: join(webRoot, 'plans', 'product-demo', 'manifest.json'),
    nextConfig: join(webRoot, 'next.config.ts'),
  }
}

function parseArgs(argv) {
  const options = { verify: false, allowDirty: false, allowStale: false, help: false, webRoot: undefined }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--verify') options.verify = true
    else if (arg === '--allow-dirty') options.allowDirty = true
    else if (arg === '--allow-stale') options.allowStale = true
    else if (arg === '--help' || arg === '-h') options.help = true
    else if (arg === '--web-root') {
      const value = argv[++i]
      if (!value) throw new Error('--web-root needs a folder')
      options.webRoot = value
    } else if (arg === '--config') throw new Error('--config is not accepted: the demo is always built from vite.demo.config.ts')
    else throw new Error(`unknown option ${arg}`)
  }
  return options
}

async function walk(dir) {
  const files = []
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) files.push(...(await walk(full)))
    else if (entry.isFile()) files.push(full)
  }
  return files
}

async function git(args) {
  try {
    const { stdout } = await execFileAsync('git', ['--no-optional-locks', '-C', SPA_ROOT, ...args], {
      maxBuffer: 64 * 1024 * 1024,
    })
    return stdout
  } catch {
    return null
  }
}

/** The demo's inputs: every source file under src/ and the build configuration, without tests. */
async function inputFiles() {
  const found = []
  for (const entry of INPUT_PATHS) {
    const full = join(SPA_ROOT, entry)
    if ((await stat(full)).isDirectory()) found.push(...(await walk(full)))
    else found.push(full)
  }
  return found.map((file) => toPosix(relative(SPA_ROOT, file))).filter((rel) => !TEST_FILE.test(rel)).sort()
}

async function computeSourceHash() {
  const rels = await inputFiles()
  const hash = createHash('sha256')
  for (const rel of rels) {
    const digest = sha256(canonicalBytes(rel, await readFile(join(SPA_ROOT, rel))))
    hash.update(`${rel}\n${digest}\n`)
  }
  return { sourceHash: hash.digest('hex'), inputFileCount: rels.length }
}

/** Uncommitted changes to the demo's inputs, or null when git cannot say. */
async function dirtyInputs() {
  const status = await git(['status', '--porcelain=v1', '--untracked-files=normal', '--', ...INPUT_PATHS])
  if (status === null) return null
  return status
    .split('\n')
    .map((line) => line.slice(3).trim())
    .filter((path) => path !== '' && !TEST_FILE.test(path))
}

async function runBuild(configFile) {
  await rm(BUILD_DIR, { recursive: true, force: true })
  await build({
    configFile,
    logLevel: 'warn',
    build: { outDir: BUILD_DIR, emptyOutDir: true, manifest: true },
  })
}

/** The demo has one HTML entry; its chunk carries main.tsx and everything it imports statically. */
function findEntryKey(viteManifest) {
  const entries = Object.keys(viteManifest).filter((name) => viteManifest[name].isEntry)
  if (entries.length !== 1) throw new Error(`expected one demo entry in the Vite manifest, found ${entries.length}`)
  return entries[0]
}

/** Every file the entry loads before it can render: its static imports, their CSS and their assets. */
function entryClosure(viteManifest, entryKey) {
  const files = new Set()
  const seen = new Set()
  const visit = (key) => {
    const chunk = viteManifest[key]
    if (seen.has(key) || !chunk) return
    seen.add(key)
    files.add(chunk.file)
    for (const css of chunk.css ?? []) files.add(css)
    for (const asset of chunk.assets ?? []) files.add(asset)
    for (const next of chunk.imports ?? []) visit(next)
  }
  visit(entryKey)
  return [...files]
}

function brotliSize(bytes) {
  return brotliCompressSync(bytes, {
    params: {
      [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
      [zlibConstants.BROTLI_PARAM_SIZE_HINT]: bytes.length,
    },
  }).length
}

/** KaTeX ships woff and ttf fallbacks behind its woff2 faces. Browsers always take woff2, so the rest are dropped. */
async function pruneFallbackFonts(dir) {
  const pruned = []
  for (const file of await walk(dir)) {
    if (!/\.(woff|ttf)$/.test(file)) continue
    await rm(file)
    pruned.push(toPosix(relative(dir, file)))
  }
  return pruned
}

/** The manifest's view of a published folder: canonical byte counts and hashes by path. */
async function listPublished(outDir) {
  const files = new Map()
  if (!existsSync(outDir)) return files
  for (const file of await walk(outDir)) {
    const rel = toPosix(relative(outDir, file))
    const bytes = canonicalBytes(rel, await readFile(file))
    files.set(rel, { bytes: bytes.length, sha256: sha256(bytes) })
  }
  return files
}

function snippetAround(text, index) {
  return text.slice(Math.max(0, index - 40), index + 60).replace(/\s+/g, ' ')
}

/**
 * How an absolute URL found in the bundle is treated: 'allowed' (listed above), 'template' (a URL
 * assembled at run time, counted for review) or 'finding' (refused).
 */
function absoluteUrlVerdict(raw) {
  let url
  try {
    url = new URL(raw)
  } catch {
    return { kind: 'template' }
  }
  // A trailing full stop in running text is part of the match, not of the host.
  const host = url.hostname.replace(/\.$/, '')
  if (/[${}]/.test(host) || /[${}]/.test(url.pathname)) return { kind: 'template' }
  if (url.pathname.includes('/api/')) return { kind: 'finding', reason: `absolute /api URL: "${raw.slice(0, 90)}"` }
  if (RESERVED_SUFFIXES.some((suffix) => host.endsWith(suffix)) || PARSING_BASE_HOSTS.has(host)) {
    return { kind: 'allowed' }
  }
  if (ALLOWED_URL_RULES.some((rule) => rule.host === host && url.pathname.startsWith(rule.pathPrefix ?? '/'))) {
    return { kind: 'allowed' }
  }
  return { kind: 'finding', reason: `absolute URL to a host outside the allowlist (${host}): "${raw.slice(0, 90)}"` }
}

/** Findings for one published text file, by its path in the folder. An empty list means the file passed. */
export function scanText(text, relPath) {
  const findings = []
  let templates = 0
  for (const { rule, pattern } of FORBIDDEN_TEXT) {
    const match = pattern.exec(text)
    if (match) findings.push(`${rule}: "${snippetAround(text, match.index)}"`)
  }
  for (const match of text.matchAll(EMAIL)) {
    if (!match[0].toLowerCase().endsWith(ALLOWED_EMAIL_SUFFIX)) {
      findings.push(`e-mail address other than ${ALLOWED_EMAIL_SUFFIX}: "${match[0]}"`)
    }
  }
  // The font licence is shipped with the font and names the project's address in its copyright line, as text.
  const urls = LICENCE_FILE.test(relPath) ? [] : [...text.matchAll(ABSOLUTE_URL)]
  for (const match of urls) {
    const verdict = absoluteUrlVerdict(match[0])
    if (verdict.kind === 'template') templates += 1
    else if (verdict.kind === 'finding') findings.push(verdict.reason)
  }
  for (const { rule, pattern, htmlOnly } of ROOT_RELATIVE_REFERENCES) {
    if (htmlOnly && !/\.html?$/i.test(relPath)) continue
    for (const match of text.matchAll(pattern)) findings.push(`${rule}: "${snippetAround(text, match.index)}"`)
  }
  for (const { rule, pattern } of SECRET_SHAPES) {
    if (pattern.test(text)) findings.push(`secret-shaped string (${rule})`)
  }
  return { findings: findings.slice(0, 12), templates }
}

async function scanPublished(records, bytesByPath) {
  const rows = []
  let templates = 0
  for (const record of records.filter((item) => TEXT_FILE.test(item.path))) {
    const scanned = scanText(bytesByPath.get(record.path).toString('utf8'), record.path)
    templates += scanned.templates
    for (const finding of scanned.findings) rows.push(`${record.path}: ${finding}`)
  }
  return { findings: rows, templates }
}

function budgetProblems(records, totalBytes, entryBrotliBytes) {
  const problems = []
  if (records.length > BUDGETS.maxFiles) problems.push(`${records.length} files, budget is ${BUDGETS.maxFiles}`)
  if (totalBytes > BUDGETS.maxTotalBytes) problems.push(`${totalBytes} bytes, budget is ${BUDGETS.maxTotalBytes}`)
  for (const record of records) {
    const limit = record.path.endsWith('.css') ? BUDGETS.maxCssFileBytes : BUDGETS.maxFileBytes
    if (record.bytes > limit) problems.push(`${record.path} is ${record.bytes} bytes, budget per file is ${limit}`)
  }
  if (entryBrotliBytes > BUDGETS.maxEntryBrotliBytes) {
    problems.push(`entry closure is ${entryBrotliBytes} bytes brotli, budget is ${BUDGETS.maxEntryBrotliBytes}`)
  }
  return problems
}

/**
 * The two stills the site shows while the demo loads or sits on a phone card (capture-demo-poster.mjs). Each must
 * exist, and they must have been captured from the sources that are being built: an older still may show cases the demo
 * no longer has. The capture writes the hash of the sources it used into POSTER_SOURCE_FILE. The hash is content, not
 * file times, so it holds in a fresh checkout and in CI. Only the stills and that record are left out of the sources,
 * because they are outputs of a capture from this build.
 */
const POSTER_DIR = 'src/demo/product-demo/public'
const POSTER_FILES = ['poster-desktop.webp', 'poster-phone.webp']
export const POSTER_SOURCE_FILE = 'src/demo/product-demo/poster-source.json'

/** The hash of the demo sources without the stills and their record: what a capture records and a build compares. */
export async function posterSourceHash() {
  const left = new Set([...POSTER_FILES.map((name) => `${POSTER_DIR}/${name}`), POSTER_SOURCE_FILE])
  const hash = createHash('sha256')
  for (const rel of (await inputFiles()).filter((item) => !left.has(item))) {
    const digest = sha256(canonicalBytes(rel, await readFile(join(SPA_ROOT, rel))))
    hash.update(`${rel}\n${digest}\n`)
  }
  return hash.digest('hex')
}

async function posterProblems() {
  const problems = []
  for (const name of POSTER_FILES) {
    if (!existsSync(join(SPA_ROOT, POSTER_DIR, name))) problems.push(`${POSTER_DIR}/${name} is missing. Capture the posters: node scripts/capture-demo-poster.mjs`)
  }
  if (problems.length > 0) return problems
  let recorded
  try {
    recorded = JSON.parse(await readFile(join(SPA_ROOT, POSTER_SOURCE_FILE), 'utf8')).sourceHash
  } catch {
    return [`${POSTER_SOURCE_FILE} is missing. Capture the posters: node scripts/capture-demo-poster.mjs`]
  }
  if (recorded !== (await posterSourceHash())) {
    problems.push('the posters were captured from other sources than these. Capture them again from this build: node scripts/capture-demo-poster.mjs')
  }
  return problems
}

async function publish(paths, options) {
  const dirty = await dirtyInputs()
  if (dirty === null && !options.allowDirty) {
    console.error('demo:build: git cannot confirm that the demo inputs are clean. Commit first, or pass --allow-dirty for a local build.')
    return 1
  }
  const dirtyList = dirty ?? []
  const isDirty = dirty === null || dirtyList.length > 0
  if (isDirty && !options.allowDirty) {
    console.error(`demo:build: ${dirtyList.length} uncommitted change(s) in the demo inputs. Commit them, or pass --allow-dirty for a local build:`)
    for (const path of dirtyList.slice(0, 25)) console.error(`  ${path}`)
    return 1
  }

  const commit = (await git(['rev-parse', 'HEAD']))?.trim() ?? null
  const commitDate = (await git(['log', '-1', '--format=%cI']))?.trim() ?? null
  const { sourceHash, inputFileCount } = await computeSourceHash()

  console.log(`demo:build: building ${relative(SPA_ROOT, DEFAULT_CONFIG)}`)
  await runBuild(DEFAULT_CONFIG)

  const viteManifest = JSON.parse(await readFile(join(BUILD_DIR, '.vite', 'manifest.json'), 'utf8'))
  const entryKey = findEntryKey(viteManifest)
  const pruned = await pruneFallbackFonts(BUILD_DIR)
  await rm(join(BUILD_DIR, '.vite'), { recursive: true, force: true })

  // Scripts and stylesheets only: the browser fetches a font face when text uses it, so faces are not cold-path weight.
  const closureFiles = entryClosure(viteManifest, entryKey).filter(
    (file) => /\.(js|css)$/.test(file) && existsSync(join(BUILD_DIR, file)),
  )
  let entryBrotliBytes = 0
  for (const file of closureFiles) entryBrotliBytes += brotliSize(canonicalBytes(file, await readFile(join(BUILD_DIR, file))))

  // The exact bytes to publish, by path. Hashes, sizes and the scan all read these.
  const bytesByPath = new Map()
  for (const file of await walk(BUILD_DIR)) {
    const rel = toPosix(relative(BUILD_DIR, file))
    bytesByPath.set(rel, canonicalBytes(rel, await readFile(file)))
  }
  const records = [...bytesByPath.keys()].sort().map((path) => {
    const bytes = bytesByPath.get(path)
    return { path, bytes: bytes.length, sha256: sha256(bytes) }
  })
  const totalBytes = records.reduce((sum, record) => sum + record.bytes, 0)

  const problems = budgetProblems(records, totalBytes, entryBrotliBytes)
  const scan = await scanPublished(records, bytesByPath)
  problems.push(...scan.findings)
  problems.push(...(await posterProblems()))
  if (problems.length > 0) {
    console.error(`demo:build: refused, ${problems.length} problem(s). The published bundle was not changed.`)
    for (const problem of problems.slice(0, 60)) console.error(`  - ${problem}`)
    return 1
  }

  await rm(paths.outDir, { recursive: true, force: true })
  for (const record of records) {
    const target = join(paths.outDir, record.path)
    await mkdir(dirname(target), { recursive: true })
    await writeFile(target, bytesByPath.get(record.path))
  }

  const manifest = {
    schema: 1,
    generatedBy: 'scripts/build-product-demo.mjs',
    entry: 'index.html',
    buildDate: new Date().toISOString(),
    sourceCommit: commit,
    sourceCommitDate: commitDate,
    sourceHash,
    inputFileCount,
    dirty: isDirty,
    dirtyPathCount: dirtyList.length,
    dirtyPaths: dirtyList.slice(0, 200),
    budgets: BUDGETS,
    totals: {
      files: records.length,
      bytes: totalBytes,
      entryBrotliBytes,
      prunedFallbackFontFiles: pruned.length,
      dynamicUrlTemplates: scan.templates,
    },
    hosting: { headerRules: HEADER_RULES, rewrites: REWRITES },
    files: records,
  }
  await mkdir(dirname(paths.manifestPath), { recursive: true })
  await writeFile(paths.manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

  console.log(`demo:build: published ${records.length} files, ${totalBytes} bytes, entry ${entryBrotliBytes} bytes brotli`)
  console.log(`demo:build: manifest ${relative(SPA_ROOT, paths.manifestPath)} (dirty: ${isDirty})`)
  return 0
}

async function verify(paths, options = {}) {
  const problems = []
  const warnings = []
  let manifest
  try {
    manifest = JSON.parse(await readFile(paths.manifestPath, 'utf8'))
  } catch {
    console.error(`demo:verify: no manifest at ${relative(SPA_ROOT, paths.manifestPath)}. Run pnpm demo:build.`)
    return 1
  }

  if (manifest.dirty) problems.push('the manifest was written from uncommitted demo inputs. Rebuild from a clean tree.')
  const current = await computeSourceHash()
  if (current.sourceHash !== manifest.sourceHash) {
    // The bundle carries the product's code, so any product change makes it older than the sources. CI passes --allow-stale
    // so that an unrelated product change is not blocked by a showcase bundle: the files and headers must still be intact.
    const message = 'the demo sources changed since the manifest was written. Run pnpm demo:build before the site next ships the demo.'
    if (options.allowStale) warnings.push(message)
    else problems.push(message)
  }
  if (manifest.totals.bytes > manifest.budgets.maxTotalBytes) problems.push('the manifest records a size over budget')

  const published = await listPublished(paths.outDir)
  const expected = new Set()
  for (const file of manifest.files) {
    expected.add(file.path)
    const found = published.get(file.path)
    if (!found) problems.push(`published file missing: ${file.path}`)
    else if (found.bytes !== file.bytes || found.sha256 !== file.sha256) problems.push(`published file changed: ${file.path}`)
  }
  for (const path of published.keys()) {
    if (!expected.has(path)) problems.push(`published file not in the manifest: ${path}`)
  }

  const nextText = await readFile(paths.nextConfig, 'utf8')
  for (const rule of manifest.hosting.headerRules) {
    if (!nextText.includes(`source: "${rule.source}"`)) problems.push(`next.config.ts has no header rule for ${rule.source}`)
    for (const header of rule.headers) {
      const line = `{ key: "${header.key}", value: "${header.value}" }`
      if (!nextText.includes(line)) problems.push(`next.config.ts differs from the manifest for ${header.key}`)
    }
  }
  for (const rewrite of manifest.hosting.rewrites) {
    const line = `{ source: "${rewrite.source}", destination: "${rewrite.destination}" }`
    if (!nextText.includes(line)) problems.push(`next.config.ts differs from the manifest for the rewrite ${rewrite.source}`)
  }

  for (const warning of warnings) console.warn(`demo:verify: warning, ${warning}`)
  if (problems.length > 0) {
    console.error(`demo:verify: stale, ${problems.length} problem(s):`)
    for (const problem of problems) console.error(`  - ${problem}`)
    return 1
  }
  console.log(`demo:verify: the manifest matches the sources, ${manifest.files.length} published files and next.config.ts`)
  return 0
}

function usage() {
  console.log('usage: node scripts/build-product-demo.mjs [--verify [--allow-stale]] [--allow-dirty] [--web-root <dir> (with --verify only)]')
}

async function main() {
  const options = parseArgs(process.argv.slice(2))
  if (options.help) {
    usage()
    return 0
  }
  if (options.webRoot !== undefined && !options.verify) {
    console.error('demo:build: --web-root is accepted only with --verify. Publishing always writes apps/verevon-web.')
    return 1
  }
  const paths = publishPaths(resolve(options.webRoot ?? DEFAULT_WEB_ROOT))
  if (options.verify) return verify(paths, options)
  return publish(paths, options)
}

// Run as a script, not when a test imports the helpers. Matching the file name is robust to drive-letter case.
if (process.argv[1] !== undefined && /[\\/]build-product-demo\.mjs$/.test(process.argv[1])) {
  main().then(
    (code) => {
      process.exitCode = code
    },
    (error) => {
      console.error(`demo:build: ${error instanceof Error ? error.stack : String(error)}`)
      process.exitCode = 1
    },
  )
}
