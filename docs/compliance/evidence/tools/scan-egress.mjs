#!/usr/bin/env node
// scan-egress.mjs -- derive the outbound recipients of the code base from the code
// itself, so the sub-processor list can be regenerated and diffed instead of
// remembered.
//
// What it does
//   1. Lists the files of the repository (git: tracked plus untracked-not-ignored;
//      otherwise a directory walk) and keeps runtime source and configuration:
//      non-test, non-doc, non-generated, non-tooling. Every excluded file is counted
//      under its reason, so the rule is visible in the output.
//   2. Finds outbound hostnames: URL literals (http, https, ws, wss) in that source,
//      split into CODE hits and COMMENT hits (a URL in a comment, or on a line that names
//      it as an identifier such as a JWT claim, documents a vendor but proves no egress).
//      Rust blocks under `#[cfg(test)]` are skipped.
//   3. Matches each host against the vendor catalogue below (VENDORS). A host the
//      catalogue does not know, and no ignore rule explains, is reported as
//      UNCLASSIFIED; `--strict` turns any such host into a failure, so a new vendor
//      cannot arrive unnoticed.
//   4. Looks for declared SDK dependencies in manifests (package.json, Cargo.toml,
//      go.mod, requirements, pyproject.toml): an SDK hides its host (the Resend
//      client, for one), so the host scan alone would miss it.
//   5. For each vendor, checks the environment variables that gate it: the names
//      listed in the catalogue are looked up in the source and in a Rust config struct
//      (a name found nowhere fails --strict, so a stale hint cannot stay), and a window
//      of lines around each code hit is searched for other key, endpoint or token names
//      the catalogue does not list. The report also says whether a compose file or
//      example env file mentions each name, and whether it gives it a default (only
//      "empty" or "non-empty": a value is never printed). A credential-like name that
//      no vendor claims must be reviewed as internal plumbing, or --strict fails.
//
// Output is deterministic: sorted, relative paths with forward slashes, no timestamps,
// no absolute paths. Run it twice on the same tree and the output is identical, so it
// can be diffed.
//
// Usage
//   node scan-egress.mjs [--root DIR] [--json | --summary] [--strict] [--verify-doc FILE]
//                        [--catalogue FILE.json] [--max-evidence N]
//   --json            machine-readable result (the contract a drift test for the trust page reads)
//   --summary         the diffable form: one line per vendor, host, variable and dependency, with
//                     no line numbers and no counts, so unrelated code motion does not show
//   --strict          exit 1 when a host is UNCLASSIFIED, a catalogue vendor has no evidence at
//                     all (stale entry), a curated variable is found nowhere, or a credential-
//                     like variable is claimed by no vendor and reviewed as nothing
//   --verify-doc FILE exit 1 when FILE (a markdown register) does not name every vendor
//                     the scan found, by id, with a disposition; see verifyDocument()
//   --catalogue FILE  replace the built-in catalogue with a JSON array (tests use this)
//   Without --json or --summary the full text report (with file:line evidence) is printed.
//
// Exit status: 0 clean, 1 a check failed, 2 usage error.
//
// Limits (subprocessors-from-code.md repeats them)
//   * A URL literal is evidence that the code can address a host. It is not evidence
//     that the host is called in production, or with customer content.
//   * Names built at run time (a host from a variable, a database row, a user's own
//     setting) are invisible; the count of "dynamic" URL literals is reported.
//   * The catalogue's class, region and DPA fields are the engineer's reading, not
//     contract facts. This tool never writes a region or DPA status of its own.

import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const HERE = path.dirname(fileURLToPath(import.meta.url))

// ---------------------------------------------------------------------------
// File selection
// ---------------------------------------------------------------------------

/** Directory names never entered: copies, builds, dependencies, caches, generated code. */
export const SKIPPED_DIRECTORIES = [
  '.git', '.claude', 'node_modules', 'target', '.next', 'dist', 'build', 'out', 'coverage',
  '__pycache__', '.venv', 'venv', 'vendor', '.turbo', '.cache', '.idea', '.vscode',
  'playwright-report', 'test-results', 'gen', 'generated',
]

/** Files larger than this are exports or bundles, not hand-written source (an IDE inspection report is 22 MB). */
export const MAX_FILE_BYTES = 1_500_000

/** Extensions scanned for URL literals. */
export const SCANNED_EXTENSIONS = [
  'rs', 'go', 'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'py', 'yml', 'yaml', 'toml', 'json',
  'html', 'css', 'conf', 'ini', 'cfg', 'properties', 'env', 'example',
]
// SQL is not scanned: the only URL literals in the migrations are documentation links in the
// capability-core seed rows (source_urls metadata), which are not call sites.

const COMPOSE_NAME = /^(docker-)?compose[^/]*\.ya?ml$|^docker-compose[^/]*\.ya?ml$/i
const ENV_FILE_NAME = /^\.env(\..*)?$|\.env(\.example|\.sample|\.template)?$/i
const MANIFEST_NAMES = ['package.json', 'Cargo.toml', 'go.mod', 'pyproject.toml']
const LOCK_NAMES = new Set([
  'package-lock.json', 'pnpm-lock.yaml', 'yarn.lock', 'Cargo.lock', 'go.sum', 'bun.lockb',
  'poetry.lock', 'uv.lock', 'composer.lock',
])
/** Manifests and configs that carry homepage, repository or schema URLs, not egress. */
const URL_NOISE_JSON = new Set(['tsconfig.json', 'package.json', 'components.json', 'biome.json', 'turbo.json'])

const TEST_DIRECTORY = /^(tests?|__tests__|testdata|test-data|fixtures?|e2e|__mocks__|mocks?|stubs?|examples?|benches|bench|benchmarks?|evals?|evaluations?|e2e-tests?)$/i
const DOC_DIRECTORY = /^(docs?|plans|documentation|adr|adrs|runbooks?)$/i
const TOOLING_DIRECTORY = /^(scripts?|\.github|\.circleci|\.husky|tools|hack|ci|\.storybook|storybook)$/i
const TEST_FILE = /(_test\.go|\.test\.[cm]?[jt]sx?|\.spec\.[cm]?[jt]sx?|_test\.py|^test_[^/]*\.py|^conftest\.py|\.stories\.[jt]sx?|_tests?\.(rs|json)|[_.-]e2e\.[a-z]+|^benchmark\.[a-z]+$|\.fixture\.[a-z]+|\.mock\.[a-z]+)$/i
const DOC_FILE = /\.(md|mdx|txt|rst|adoc)$/i
/** Generated API client libraries for this platform's own APIs (OpenAPI generator output). */
const GENERATED_DIRECTORY = /^sdks?$/i
const GENERATED_FILE = /(\.pb\.go|_pb\.[jt]s|_pb\.d\.ts|\.pb\.rs|_grpc\.pb\.go|\.gen\.[a-z]+|\.generated\.[a-z]+|\.min\.js|\.d\.ts)$/i
const TOOLING_FILE = /^(Dockerfile[^/]*|Makefile|makefile|\.dockerignore|\.gitlab-ci\.ya?ml|\.travis\.ya?ml|\.pre-commit-config\.ya?ml|renovate\.json|dependabot\.ya?ml|\.(eslintrc|prettierrc)[^/]*|.*\.(sh|ps1|bat|cmd))$/i

/**
 * Decide whether a repo-relative path is runtime source or configuration.
 * Returns { scan: true, kind } or { scan: false, reason }.
 * kind: 'source' | 'compose' | 'env' | 'compose-legacy' | 'env-legacy' | 'manifest'
 * (a compose or env file at the repository root belongs to the legacy monolith stack, whose
 * own header says canonical services are the plane-level compose files, so it is kept apart)
 */
export function classifyFile(relative) {
  const parts = relative.split('/')
  const name = parts[parts.length - 1]
  const directories = parts.slice(0, -1)
  if (directories.some((part) => SKIPPED_DIRECTORIES.includes(part))) return { scan: false, reason: 'skipped-directory' }
  if (LOCK_NAMES.has(name)) return { scan: false, reason: 'lockfile' }
  const isRequirements = /^requirements[^/]*\.txt$/i.test(name)
  if (directories.some((part) => DOC_DIRECTORY.test(part)) || (DOC_FILE.test(name) && !isRequirements)) return { scan: false, reason: 'doc' }
  if (directories.some((part) => TEST_DIRECTORY.test(part)) || TEST_FILE.test(name)) return { scan: false, reason: 'test' }
  if (GENERATED_FILE.test(name) || directories.some((part) => GENERATED_DIRECTORY.test(part))) return { scan: false, reason: 'generated' }
  if (directories.some((part) => TOOLING_DIRECTORY.test(part)) || TOOLING_FILE.test(name)) return { scan: false, reason: 'tooling' }
  if (MANIFEST_NAMES.includes(name) || /^requirements[^/]*\.txt$/i.test(name)) return { scan: true, kind: 'manifest' }
  if (COMPOSE_NAME.test(name)) return { scan: true, kind: parts.length === 1 ? 'compose-legacy' : 'compose' }
  if (ENV_FILE_NAME.test(name)) return { scan: true, kind: parts.length === 1 ? 'env-legacy' : 'env' }
  if (URL_NOISE_JSON.has(name)) return { scan: false, reason: 'manifest-noise' }
  const extension = name.includes('.') ? name.split('.').pop().toLowerCase() : ''
  if (SCANNED_EXTENSIONS.includes(extension)) return { scan: true, kind: 'source' }
  return { scan: false, reason: 'not-source' }
}

export function isConfigKind(kind) {
  return kind === 'compose' || kind === 'env' || kind === 'compose-legacy' || kind === 'env-legacy'
}

function walk(root, relative, found) {
  const absolute = path.join(root, relative)
  let entries
  try {
    entries = fs.readdirSync(absolute, { withFileTypes: true })
  } catch {
    return
  }
  for (const entry of entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))) {
    if (entry.isDirectory()) {
      if (SKIPPED_DIRECTORIES.includes(entry.name)) continue
      walk(root, relative ? `${relative}/${entry.name}` : entry.name, found)
    } else if (entry.isFile()) {
      found.push(relative ? `${relative}/${entry.name}` : entry.name)
    }
  }
}

/** List repo-relative file paths, sorted. Uses git when `root` is a git work tree. */
export function listFiles(root, options = {}) {
  const useGit = options.git !== false && fs.existsSync(path.join(root, '.git'))
  if (useGit) {
    try {
      const output = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
        cwd: root,
        maxBuffer: 1 << 28,
        stdio: ['ignore', 'pipe', 'ignore'],
      })
      const files = [...new Set(output.toString('utf8').split('\0').filter(Boolean))].sort()
      return { files, method: 'git' }
    } catch {
      // fall through to the directory walk
    }
  }
  const files = []
  walk(root, '', files)
  return { files: files.sort(), method: 'walk' }
}

// ---------------------------------------------------------------------------
// Reading a file: lines, comment or code, test blocks skipped
// ---------------------------------------------------------------------------

const SLASH_COMMENT_EXTENSIONS = new Set(['rs', 'go', 'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'css', 'sql'])
const HASH_COMMENT_EXTENSIONS = new Set(['py', 'yml', 'yaml', 'toml', 'conf', 'ini', 'cfg', 'properties', 'env', 'example'])

function extensionOf(relative) {
  const name = relative.split('/').pop()
  if (/^\.env/.test(name)) return 'env'
  return name.includes('.') ? name.split('.').pop().toLowerCase() : ''
}

/** True when column `index` of `line` sits inside a comment. Approximate by design. */
export function isCommentAt(line, index, extension) {
  const trimmed = line.trim()
  if (SLASH_COMMENT_EXTENSIONS.has(extension)) {
    if (extension === 'sql' && trimmed.startsWith('--')) return true
    if (trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*')) return true
    const trailing = line.indexOf(' // ')
    return trailing !== -1 && trailing < index
  }
  if (HASH_COMMENT_EXTENSIONS.has(extension)) {
    if (trimmed.startsWith('#')) return true
    const trailing = line.indexOf(' # ')
    return trailing !== -1 && trailing < index
  }
  if (extension === 'html') return trimmed.startsWith('<!--')
  return false
}

/** Column of a block-comment opener that is code, not inside a string and not after a line comment. -1 if none. */
function blockCommentStart(line) {
  let quote = null
  for (let index = 0; index < line.length - 1; index += 1) {
    const char = line[index]
    if (quote) {
      if (char === '\\') index += 1
      else if (char === quote) quote = null
    } else if (char === '"' || char === "'" || char === '`') {
      quote = char
    } else if (char === '/' && line[index + 1] === '/') {
      return -1
    } else if (char === '/' && line[index + 1] === '*') {
      return index
    }
  }
  return -1
}

/**
 * Marks lines that sit inside a multi-line comment: a Python docstring (triple quotes) or a
 * C-style block comment. Crude on purpose: it flips on the first marker it sees on a line.
 */
export function blockCommentMask(lines, extension) {
  const mask = new Array(lines.length).fill(false)
  if (extension === 'py') {
    let open = null
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index]
      const quotes = [...line.matchAll(/"""|'''/g)].map((match) => match[0])
      if (open !== null) {
        mask[index] = true
        if (quotes.includes(open) && quotes.filter((quote) => quote === open).length % 2 === 1) open = null
      } else if (quotes.length > 0) {
        const first = quotes[0]
        if (quotes.filter((quote) => quote === first).length % 2 === 1) {
          open = first
          mask[index] = true
        }
      }
    }
  } else if (SLASH_COMMENT_EXTENSIONS.has(extension)) {
    let inside = false
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index]
      if (inside) {
        mask[index] = true
        if (line.includes('*/')) inside = false
      } else {
        const start = blockCommentStart(line)
        if (start !== -1 && !line.includes('*/', start + 2)) {
          inside = true
          mask[index] = true
        }
      }
    }
  }
  return mask
}

/**
 * Marks the lines of a Rust file that belong to a `#[cfg(test)]` item. A column-0
 * attribute starts the skip; it ends at the item's own `;` (an external module or a
 * use) or at the next line that is exactly `}` at column 0.
 */
export function rustTestLineMask(lines) {
  const mask = new Array(lines.length).fill(false)
  for (let index = 0; index < lines.length; index += 1) {
    if (!/^#\[cfg\(test\)\]\s*$/.test(lines[index])) continue
    mask[index] = true
    let cursor = index + 1
    // further attributes on the same item
    while (cursor < lines.length && /^\s*#\[/.test(lines[cursor])) {
      mask[cursor] = true
      cursor += 1
    }
    if (cursor >= lines.length) break
    const head = lines[cursor]
    mask[cursor] = true
    if (/;\s*$/.test(head) && !/\{\s*$/.test(head)) {
      index = cursor
      continue
    }
    cursor += 1
    while (cursor < lines.length) {
      mask[cursor] = true
      if (lines[cursor] === '}') break
      cursor += 1
    }
    index = cursor
  }
  return mask
}

// ---------------------------------------------------------------------------
// Host extraction
// ---------------------------------------------------------------------------

const URL_PATTERN = /\b(https?|wss?):\/\/([^\s"'`<>()[\]\\,;|]+)/gi

/**
 * Normalise the authority part of a URL literal. Placeholders ({region}, ${X}, %s, $X)
 * become "*". Returns null when the host is entirely dynamic.
 */
export function normaliseHost(rawAuthority) {
  let authority = rawAuthority
  const slash = authority.search(/[/?#]/)
  if (slash !== -1) authority = authority.slice(0, slash)
  const at = authority.lastIndexOf('@')
  if (at !== -1) authority = authority.slice(at + 1)
  authority = authority.replace(/\$\{[^}]*\}|\{[^}]*\}|%[sv]|\$[A-Za-z_][A-Za-z0-9_]*|<[^>]*>/g, '*')
  authority = authority.replace(/[{}]+/g, '')
  if (authority.startsWith('[')) return authority.replace(/\]:\d+$/, ']').toLowerCase()
  authority = authority.replace(/:\d+$/, '').replace(/:\*$/, '').replace(/\.+$/, '')
  authority = authority.toLowerCase()
  if (authority === '' || /^\*+(\.\*+)*$/.test(authority)) return null
  return authority
}

/** Find URL literals in one line. Returns [{ host, scheme, index, dynamic }]. */
export function findUrlsInLine(line) {
  const found = []
  URL_PATTERN.lastIndex = 0
  let match
  while ((match = URL_PATTERN.exec(line)) !== null) {
    const host = normaliseHost(match[2])
    found.push({ host, scheme: match[1].toLowerCase(), index: match.index, dynamic: host === null })
  }
  return found
}

// ---------------------------------------------------------------------------
// Ignore rules: hosts that are not recipients, each with its reason
// ---------------------------------------------------------------------------

const SCHEMA_HOSTS = new Set([
  'www.w3.org', 'json-schema.org', 'schemas.xmlsoap.org', 'schemas.microsoft.com',
  'schemas.openxmlformats.org', 'purl.org', 'ns.adobe.com', 'www.sitemaps.org', 'xmlns.com',
  'opentelemetry.io', 'schema.org', 'swagger.io', 'spec.openapis.org', 'www.apache.org',
  'www.gnu.org', 'creativecommons.org', 'www.iana.org', 'tools.ietf.org', 'datatracker.ietf.org',
  'www.rfc-editor.org', 'www.ietf.org', 'json.schemastore.org', 'schemastore.org', 'openapi.vercel.sh', 'graphml.graphdrawing.org',
])

/** The operator's own domains: links to its own sites, not a third party. */
const FIRST_PARTY_DOMAINS = ['verevon.ai', 'verevon.io', 'verevon.no', 'verevon.com', 'coresystem.com', 'triodelab.com', 'triodelab.no', 'idknuten.no']

/**
 * Each rule: { id, reason, test(host) }. Order matters: the first match wins.
 * These hosts are never recipients of content, so they are not vendors.
 */
export const IGNORE_RULES = [
  {
    id: 'loopback-or-private',
    reason: 'loopback, private-range or link-local address, or host.docker.internal: never leaves the machine',
    test: (host) =>
      host === 'localhost' ||
      host === 'host.docker.internal' ||
      host === 'gateway.docker.internal' ||
      /^127\./.test(host) ||
      host === '0.0.0.0' ||
      host.startsWith('[') ||
      /^10\./.test(host) ||
      /^192\.168\./.test(host) ||
      /^172\.(1[6-9]|2\d|3[01])\./.test(host) ||
      /^169\.254\./.test(host),
  },
  {
    id: 'in-cluster-name',
    reason: 'a name with no dot (a compose or cluster service name) or under .internal, .local, .svc, .localhost: resolved inside the deployment',
    test: (host) => !host.includes('.') || /\.(internal|local|svc|localhost|cluster\.local|lan)$/.test(host),
  },
  {
    id: 'reserved-placeholder',
    reason: 'reserved or placeholder name (RFC 2606: example, test, invalid) or an example-style name used in samples',
    test: (host) =>
      /(^|\.)example(\.(com|org|net|io|no|dev))?$/.test(host) ||
      /(^|\.)(test|invalid)$/.test(host) ||
      /^(a|b|c|foo|bar|baz|acme)\.(com|io|no|org|net|dev|example)$/.test(host) ||
      /^(contoso|your-[a-z0-9-]+|my-[a-z0-9-]+)\.[a-z0-9.-]+$/.test(host),
  },
  {
    id: 'first-party-domain',
    reason: 'the operator\'s own domain (a link to its own site), not a third-party recipient',
    test: (host) => FIRST_PARTY_DOMAINS.some((domain) => host === domain || host.endsWith(`.${domain}`)),
  },
  {
    id: 'schema-or-namespace',
    reason: 'XML or JSON-schema namespace, specification or licence URL: an identifier, not a request target',
    test: (host) => SCHEMA_HOSTS.has(host),
  },
  {
    id: 'unresolved-template',
    reason: 'every label is a run-time placeholder, so no host name exists in the source',
    test: (host) => /^[*.]+$/.test(host),
  },
]

export function ignoreReasonFor(host, rules = IGNORE_RULES) {
  for (const rule of rules) if (rule.test(host)) return rule
  return null
}

// ---------------------------------------------------------------------------
// Vendor matching
// ---------------------------------------------------------------------------

/** pattern "a.b.com" matches exactly; "*.b.com" matches any subdomain of b.com. A "*" inside a label is a placeholder. */
export function hostMatches(pattern, host) {
  if (pattern.startsWith('*.')) {
    const suffix = pattern.slice(1)
    return host.endsWith(suffix) && host.length > suffix.length
  }
  if (pattern === host) return true
  if (host.includes('*')) {
    const escaped = host.split('*').map((part) => part.replace(/[.+?^${}()|[\]\\]/g, '\\$&'))
    return new RegExp(`^${escaped.join('[a-z0-9-]+')}$`).test(pattern)
  }
  return false
}

export function vendorForHost(host, vendors) {
  let best = null
  let bestScore = -1
  for (const vendor of vendors) {
    for (const pattern of vendor.hosts ?? []) {
      if (!hostMatches(pattern, host)) continue
      // exact beats wildcard; a longer pattern beats a shorter one
      const score = (pattern.startsWith('*.') ? 0 : 1000) + pattern.length
      if (score > bestScore) {
        best = vendor
        bestScore = score
      }
    }
  }
  return best
}

// ---------------------------------------------------------------------------
// Environment variable names
// ---------------------------------------------------------------------------

const ENV_NAME = /\b[A-Z][A-Z0-9]*(?:_{1,2}[A-Z0-9]+)+\b/g
const GATING_WORD = /(KEY|TOKEN|SECRET|ENDPOINT|URL|BASE|HOST|REGION|ENABLED|DEPLOYMENT|ACCOUNT|SID|PASSWORD|DSN|DOMAIN|PROVIDER|MODE)/
// How source code reads an environment variable. The first pattern is deliberately broad:
// any call whose first argument is an ALL_CAPS_WITH_UNDERSCORE string literal (os.Getenv,
// env::var, getString("X", d), parse_env::<u64>("X"), os.getenv('X'), Deno.env.get("X")).
// It also matches an error code passed to a function, which is harmless: the names are only
// used when they match a catalogue entry or look like a credential.
const NAME_LITERAL = '[A-Z][A-Z0-9]*(?:_{1,2}[A-Z0-9]+)+'
const ENV_USE = [
  new RegExp(`\\b[A-Za-z_][A-Za-z0-9_]*(?:::<[^>]*>)?\\(\\s*["'](${NAME_LITERAL})["']`, 'g'),
  /process\.env\.([A-Z][A-Z0-9_]+)/g,
  new RegExp(`process\\.env\\[\\s*["'](${NAME_LITERAL})["']\\s*\\]`, 'g'),
  /import\.meta\.env\.([A-Z][A-Z0-9_]+)/g,
  /\benv\.([A-Z][A-Z0-9]*_[A-Z0-9_]+)/g,
  new RegExp(`os\\.environ\\[\\s*["'](${NAME_LITERAL})["']\\s*\\]`, 'g'),
  /\b(?:env|envconfig):"([A-Z][A-Z0-9_]+)/g,
  /\benv\s*=\s*"([A-Z][A-Z0-9_]+)"/g,
]

/** Names in a line that look like environment variables, with how they were recognised. */
export function envNamesInLine(line) {
  const found = new Map()
  for (const pattern of ENV_USE) {
    pattern.lastIndex = 0
    let match
    while ((match = pattern.exec(line)) !== null) found.set(match[1], 'read')
  }
  const interpolation = /\$\{([A-Z][A-Z0-9_]+)/g
  let match
  while ((match = interpolation.exec(line)) !== null) if (!found.has(match[1])) found.set(match[1], 'interpolated')
  return found
}

/** All-caps tokens near a hit that look like a key or endpoint name (heuristic, reported as "seen near"). */
export function gatingNamesInLine(line) {
  const names = new Set()
  ENV_NAME.lastIndex = 0
  let match
  while ((match = ENV_NAME.exec(line)) !== null) {
    if (GATING_WORD.test(match[0])) names.add(match[0])
  }
  return names
}

// ---------------------------------------------------------------------------
// The vendor catalogue
// ---------------------------------------------------------------------------
//
// class
//   subprocessor     Verevon sends it customer content or personal data under Verevon's own
//                    account or contract. Candidate for the Art. 28 list; counsel decides.
//   user-authorised  the customer or user connects their own account (Graph, Google, the
//                    ChatGPT subscription); data moves at their instruction under their terms.
//   infrastructure   hosting, fonts, scripts, security checks: no customer content by design,
//                    but a recipient of connection metadata such as an IP address.
//   public-source    a public data feed or API the platform reads. The request leaves (a query
//                    term, a place name) and the platform's egress address; no customer file.
//
// `role` is the engineer's reading of the legal relationship, for counsel to confirm or
// overturn: processor, independent-recipient, customer-directed, no-content, unknown.
//
// `region` and `dpa` are never guessed. They stay "TBC" until a contract or a deployment
// record says otherwise; the report prints them as written here.

export const VENDORS = [
  // ----- model and AI services --------------------------------------------------------------
  {
    id: 'microsoft-azure-ai',
    name: 'Microsoft Azure AI services (Azure OpenAI, AI Foundry Claude and Cohere, Document Intelligence, Language, Translator, Speech, video, fine-tuning)',
    class: 'subprocessor',
    role: 'processor',
    hosts: [
      '*.openai.azure.com', '*.cognitiveservices.azure.com', '*.services.ai.azure.com', 'ai.azure.com',
      '*.tts.speech.microsoft.com', '*.stt.speech.microsoft.com', 'api.cognitive.microsofttranslator.com',
      '*.cognitive.microsofttranslator.com', 'management.azure.com',
    ],
    gateEnv: [
      'AZURE_OPENAI_ENDPOINT', 'AZURE_OPENAI_API_KEY', 'AZURE_OPENAI_REGION', 'AZURE_OPENAI_DEPLOYMENT_TYPE',
      'AZURE_OPENAI_VIDEO_ENDPOINT', 'AZURE_ANTHROPIC_ENDPOINT', 'AZURE_ANTHROPIC_API_KEY', 'AZURE_ANTHROPIC_REGION',
      'AZURE_COHERE_ENDPOINT', 'AZURE_COHERE_API_KEY', 'AZURE_COHERE_REGION', 'AZURE_DOCUMENT_INTELLIGENCE_ENDPOINT',
      'AZURE_DOCUMENT_INTELLIGENCE_KEY', 'AZURE_AI_LANGUAGE_ENDPOINT', 'AZURE_AI_LANGUAGE_KEY', 'AZURE_TRANSLATOR_ENDPOINT',
      'AZURE_TRANSLATOR_KEY', 'AZURE_TRANSLATOR_API_KEY', 'AZURE_TRANSLATOR_REGION', 'AZURE_SPEECH_ENDPOINT', 'AZURE_SPEECH_KEY',
      'AZURE_SPEECH_REGION', 'AZURE_OPENAI_STT_API_KEY', 'AZURE_OPENAI_TTS_API_KEY', 'AZURE_OPENAI_VIDEO_API_KEY',
      'AZURE_OPENAI_REALTIME_API_KEY', 'COHERE_EMBED_V4_ENDPOINT', 'COHERE_EMBED_V4_API_KEY', 'RERANK_ENDPOINT', 'AZURE_API_BASE',
    ],
    note: 'One vendor, several services. The endpoints are operator-supplied variables, so most hosts never appear as literals; the variables are the evidence. The Cohere embed and rerank endpoints used by the Data Plane are Azure AI Foundry deployments on the dev stack, and RERANK_ENDPOINT may be pointed at public Cohere instead (see cohere-api).',
  },
  {
    id: 'openai-api',
    name: 'OpenAI API (api.openai.com), direct provider',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.openai.com'],
    gateEnv: ['OPENAI_API_KEY', 'OPENAI_API_BASE', 'OPENAI_BASE_URL'],
    note: 'Registered only when OPENAI_API_KEY is set; the same key also enables direct-OpenAI fallbacks for speech, translation, image, language and realtime.',
  },
  {
    id: 'openai-chatgpt-subscription',
    name: 'OpenAI ChatGPT backend through the user\'s own ChatGPT subscription (Codex broker)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['chatgpt.com', 'auth.openai.com'],
    gateEnv: ['CODEX_SUBSCRIPTION_TRANSPORT', 'CODEX_APP_SERVER_HARDENED', 'CODEX_SUBSCRIPTION_ENABLED', 'CODEX_SUBSCRIPTION_DIRECT_BASE_URL', 'CODEX_SUBSCRIPTION_DIRECT_PROMPT_MODE'],
    note: 'The user connects their own ChatGPT account; prompts travel on that account, under that account\'s OpenAI terms, not on a Verevon contract. The OAuth refresh goes to auth.openai.com.',
  },
  {
    id: 'anthropic-api',
    name: 'Anthropic API (api.anthropic.com), direct provider',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.anthropic.com'],
    gateEnv: ['ANTHROPIC_API_KEY'],
    note: 'Registered only when Azure AI Foundry Claude is not configured; the key being set is not enough on a stack where Foundry Claude is registered.',
  },
  {
    id: 'cohere-api',
    name: 'Cohere public API (api.cohere.ai), rerank fallback',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.cohere.ai', 'api.cohere.com'],
    gateEnv: ['COHERE_API_KEY', 'COHERE_URL'],
    note: 'The retrieval engine falls back to the public rerank endpoint when RERANK_ENDPOINT is unset.',
  },

  // ----- web search and browsing ------------------------------------------------------------
  {
    id: 'brave-search',
    name: 'Brave Search API',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: ['api.search.brave.com', 'search.brave.com'],
    gateEnv: ['QUARRY_EDGE__BRAVE_SEARCH_KEY', 'QUARRY_EDGE__ZERO_SAAS_SEARCH'],
    note: 'Paid fallback of the Quarry search chain; also needs a per-request permission that agent runs never grant.',
  },
  {
    id: 'serper-search',
    name: 'Serper (Google results API)',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: ['google.serper.dev'],
    gateEnv: ['QUARRY_EDGE__SERPER_KEY'],
    note: 'Adapter exists; no compose file in this repository passes a Serper key.',
  },
  {
    id: 'searxng-upstream-engines',
    name: 'Search engines behind the self-hosted SearXNG (Google CSE, Bing, DuckDuckGo, Yandex and others)',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: [],
    gateEnv: ['QUARRY_EDGE__SEARXNG_URL'],
    signals: [{ label: 'engine entries in the SearXNG settings', path: 'config/searxng/settings\\.yml$', pattern: '^\\s*-?\\s*name:\\s*(google|bing|duckduckgo|yandex|startpage|brave|naver|seznam|qwant|mojeek)' }],
    note: 'The free search chain forwards the query text to external engines from the platform\'s egress address. The ZDR and zero-SaaS flags do not cover this path.',
  },
  {
    id: 'letta-tool-search',
    name: 'Letta tool-definition search (operator-supplied HTTPS endpoint)',
    class: 'subprocessor',
    role: 'unknown',
    hosts: [],
    gateEnv: ['LETTA_TOOL_SEARCH_ENABLED', 'LETTA_TOOL_SEARCH_URL', 'LETTA_API_KEY'],
    note: 'capability-core can send a tool-search query (up to 4096 bytes) to a remote Letta endpoint. Off by default (LETTA_TOOL_SEARCH_ENABLED is false in the compose file) and the host is whatever the operator sets, so the recipient is unknown from code.',
  },
  {
    id: 'quarry-proxy-pool',
    name: 'Operator-supplied proxy pool for Quarry fetches (vendor not named in the code)',
    class: 'subprocessor',
    role: 'unknown',
    hosts: [],
    gateEnv: ['QUARRY_PROXY_POOL', 'QUARRY_PROXY_FIRST_PARTY', 'QUARRY_PROXY_PROCESSOR_ID'],
    note: 'QUARRY_PROXY_POOL is a list of proxy URIs the operator supplies; the code names no proxy or unblocker vendor. A configured pool is classified as a third-party processor needing approval unless QUARRY_PROXY_FIRST_PARTY is set. Empty means direct egress.',
  },
  {
    id: 'browserbase',
    name: 'Browserbase (hosted browser)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['www.browserbase.com', '*.browserbase.com'],
    gateEnv: ['QUARRY_EDGE__BROWSERBASE_API_KEY', 'QUARRY_BROWSERBASE_PROCESSOR_ID'],
    note: 'The compose file leaves it unset on purpose: the comment there calls it third-party processing, ineligible for governed runs.',
  },
  {
    id: 'browserless',
    name: 'Browserless (hosted browser)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['chrome.browserless.io', 'live.browserless.io', 'docs.browserless.io', '*.browserless.io'],
    gateEnv: [],
    note: 'Only named in comments of the browser adapter and in an evaluation matrix; the compose file says the browser driver is chosen at build time and the Browserless variables were removed.',
  },

  // ----- payments and billing ---------------------------------------------------------------
  {
    id: 'stripe-billing',
    name: 'Stripe (Verevon\'s own billing account)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.stripe.com'],
    gateEnv: ['STRIPE_API_KEY', 'STRIPE_API_KEY_SECRET', 'STRIPE_BASE_URL', 'STRIPE_WEBHOOK_SECRET'],
    note: 'Billing email, organisation name and id as customer metadata, and checkout parameters. Card details go to the hosted checkout.',
  },
  {
    id: 'stripe-connect',
    name: 'Stripe Connect (the customer connects their own Stripe account)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['connect.stripe.com'],
    gateEnv: ['STRIPE_CLIENT_ID', 'STRIPE_CLIENT_SECRET', 'STRIPE_API_BASE_URL', 'STRIPE_AUTHORIZATION_URL', 'STRIPE_TOKEN_URL'],
    note: 'An integration-core connector; the account is the customer\'s.',
  },
  {
    id: 'nexi-checkout',
    name: 'Nexi (Dibs) checkout',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.dibspayment.eu', 'test.api.dibspayment.eu', 'checkout.dibspayment.eu', 'test.checkout.dibspayment.eu'],
    gateEnv: ['NEXI_SECRET_KEY', 'NEXI_SECRET_API_KEY', 'NEXI_BASE_URL', 'NEXI_WEBHOOK_SECRET', 'NEXI_CHECKOUT_JS_URL'],
    note: 'Order items, amount and currency; the browser also loads the checkout script from the Nexi host.',
  },
  {
    id: 'hyperswitch',
    name: 'Hyperswitch (payment orchestration)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['beta.hyperswitch.io', 'sandbox.hyperswitch.io', '*.hyperswitch.io'],
    gateEnv: ['HYPERSWITCH_API_KEY', 'HYPERSWITCH_API_KEY_SECRET', 'HYPERSWITCH_BASE_URL', 'HYPERSWITCH_BACKEND_URL', 'HYPERSWITCH_CLIENT_URL', 'HYPERSWITCH_PUBLISHABLE_KEY'],
    note: 'An adapter in billing-core; the pack says billing processors are "not confirmed in the codebase", but this one is in it.',
  },

  // ----- notifications, email and SMS -------------------------------------------------------
  {
    id: 'resend',
    name: 'Resend (sign-in and verification email)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.resend.com'],
    sdks: [{ ecosystem: 'npm', name: 'resend' }],
    gateEnv: ['RESEND_API_KEY'],
    note: 'Recipient address and message body; the SDK hides the host, so the dependency is the evidence.',
  },
  {
    id: 'twilio-verify',
    name: 'Twilio Verify (SMS one-time codes)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['verify.twilio.com', 'api.twilio.com'],
    sdks: [{ ecosystem: 'npm', name: 'twilio' }],
    gateEnv: ['TWILIO_ACCOUNT_SID', 'TWILIO_AUTH_TOKEN', 'TWILIO_VERIFY_SERVICE_SID'],
    note: 'Phone number in E.164 form; Twilio generates and sends the code.',
  },
  {
    id: 'novu',
    name: 'Novu (notification delivery)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['api.novu.co', 'eu.api.novu.co', '*.novu.co'],
    sdks: [{ ecosystem: 'go', name: 'github.com/novuhq/novu-go*' }],
    gateEnv: ['NOVU_SECRET_KEY', 'NOVU_BASE_URL', 'NOTIFICATION_DELIVERY_MODE', 'NOTIFICATION_DELIVERY_CALLBACK_SECRET'],
    note: 'Delivery mode is "disabled" unless set. The EU host appears only in a comment ("set NOVU_BASE_URL"); nothing in the code selects the EU region, so with the variable unset the SDK\'s own default host applies (not read here).',
  },
  {
    id: 'postmark',
    name: 'Postmark (inbound mail-to-task processor)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['postmarkapp.com', '*.postmarkapp.com'],
    gateEnv: ['MAIL_TASK_PROVIDER', 'MAIL_TASK_WEBHOOK_SECRET', 'MAIL_TASK_RECEIVING_DOMAIN'],
    signals: [{ label: 'Postmark receiver in the mail-task package', path: 'integration-corev2/internal/mailtask/[^/]+\\.go$', pattern: 'postmark' }],
    note: 'New. The forwarded email passes through Postmark before it reaches our webhook. Not wired by any compose file yet; enabling it for customer organisations needs the sub-processor list, the transfer assessment and the trust page to include it first.',
  },

  // ----- sign-in providers and identity services --------------------------------------------
  {
    id: 'microsoft-365-entra-graph',
    name: 'Microsoft Entra sign-in and Microsoft Graph (the user\'s own Microsoft 365 account)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['graph.microsoft.com', 'login.microsoftonline.com', '*.sharepoint.com'],
    gateEnv: ['MICROSOFT_CLIENT_ID', 'MICROSOFT_CLIENT_SECRET', 'MICROSOFT_GRAPH_BASE_URL', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_TENANT_ID', 'MICROSOFT_TOKEN_ORIGIN', 'MICROSOFT_CLIENT_AUTH_MODE', 'GRAPH_BASE_URL'],
    note: 'Sign-in and, for the capabilities the user grants, mail, Teams, SharePoint and calendar content. model-gateway also uses login.microsoftonline.com for an Azure AD client-credentials token against the Azure management plane (fine-tune deployments): that is the platform\'s own principal, see microsoft-azure-ai.',
  },
  {
    id: 'google-accounts-apis',
    name: 'Google sign-in and Google APIs (Gmail, Drive, Calendar, Analytics, Ads) on the user\'s own account',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['accounts.google.com', 'oauth2.googleapis.com', 'www.googleapis.com', 'gmail.googleapis.com', 'analyticsdata.googleapis.com', 'googleads.googleapis.com', 'developers.google.com'],
    gateEnv: ['GOOGLE_CLIENT_ID', 'GOOGLE_CLIENT_SECRET', 'GOOGLE_API_BASE_URL', 'GOOGLE_ADS_DEVELOPER_TOKEN', 'GOOGLE_ADS_API_BASE_URL', 'GOOGLE_ANALYTICS_DATA_API_BASE_URL', 'GOOGLE_AUTHORIZATION_URL', 'GOOGLE_SEARCH_CONSOLE_API_BASE_URL', 'GOOGLE_TOKEN_URL'],
    note: 'Read scopes for Gmail, Drive and Calendar, a send scope exists; analytics and ads connectors in insight-core and social-core.',
  },
  {
    id: 'apple-sign-in',
    name: 'Sign in with Apple',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['appleid.apple.com'],
    gateEnv: ['APPLE_CLIENT_ID', 'APPLE_CLIENT_SECRET', 'APPLE_APP_BUNDLE_IDENTIFIER', 'APPLE_KEY_ID', 'APPLE_PRIVATE_KEY', 'APPLE_TEAM_ID'],
  },
  {
    id: 'vipps-login',
    name: 'Vipps login',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['api.vipps.no', 'apitest.vipps.no'],
    gateEnv: ['VIPPS_CLIENT_ID', 'VIPPS_CLIENT_SECRET', 'VIPPS_ENVIRONMENT'],
    note: 'Requests name, email, address and phone number scopes. The auth service writes the returned user-info object to the console log (auth.ts), which is a retention matter for J12.',
  },
  {
    id: 'okta-oidc',
    name: 'Okta (customer\'s own tenant: OIDC sign-in and directory connector)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['*.okta.com'],
    gateEnv: ['OKTA_DOMAIN', 'OKTA_CLIENT_ID', 'OKTA_CLIENT_SECRET', 'OKTA_API_TOKEN'],
    note: 'The host is built from OKTA_DOMAIN at run time, so no literal exists.',
  },
  {
    id: 'cloudflare-turnstile',
    name: 'Cloudflare Turnstile (sign-up human check)',
    class: 'subprocessor',
    role: 'processor',
    hosts: ['challenges.cloudflare.com'],
    gateEnv: ['CAPTCHA_SECRET_KEY', 'TURNSTILE_SECRET_KEY', 'CAPTCHA_ENABLED'],
    note: 'The browser loads the Turnstile script and the auth service verifies the token; visitor IP address and browser signals reach Cloudflare. Switched on when a secret key is set.',
  },
  {
    id: 'hibp-pwned-passwords',
    name: 'Have I Been Pwned range API (breached-password check)',
    class: 'infrastructure',
    role: 'no-content',
    hosts: ['api.pwnedpasswords.com'],
    gateEnv: [],
    note: 'Sends the first five hex characters of the password\'s SHA-1 with response padding on (k-anonymity). Ungated: runs whenever the check runs.',
  },

  // ----- customer-connected business and social accounts (user-authorised) ------------------
  {
    id: 'slack-api',
    name: 'Slack (the customer\'s workspace)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['slack.com', '*.slack.com'],
    gateEnv: ['SLACK_CLIENT_ID', 'SLACK_CLIENT_SECRET', 'SLACK_SIGNING_SECRET', 'SLACK_API_BASE_URL', 'SLACK_AUTHORIZATION_URL', 'SLACK_TOKEN_URL'],
  },
  {
    id: 'notion-api',
    name: 'Notion (the customer\'s workspace)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['api.notion.com', '*.notion.com'],
    sdks: [{ ecosystem: 'pypi', name: 'notion-client' }],
    gateEnv: ['NOTION_CLIENT_ID', 'NOTION_CLIENT_SECRET', 'NOTION_API_BASE_URL', 'NOTION_AUTHORIZATION_URL', 'NOTION_TOKEN_URL'],
    note: 'Two paths: the integration-core connector (OAuth) and the imports service, which reads a Notion workspace with the notion-client library.',
  },
  {
    id: 'crm-erp-import-sources',
    name: 'HubSpot, Salesforce and Odoo import connectors (the customer\'s own instance)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: [],
    sdks: [{ ecosystem: 'pypi', name: 'hubspot-api-client' }, { ecosystem: 'pypi', name: 'simple-salesforce' }, { ecosystem: 'pypi', name: 'odoorpc' }],
    signals: [{ label: 'import connectors in the imports service', path: 'imports-core/app/connectors\\.py$', pattern: 'import_from_(hubspot|salesforce|odoo)' }],
    gateEnv: [],
    wiring: 'per-user-connection',
    gatedBy: 'the user supplies the connection (host and credentials) per import; no platform variable',
    note: 'The SDKs hide the hosts and the instance host is the customer\'s, so no literal exists. The imports service reads from the customer\'s system; it sends credentials and queries to it.',
  },
  {
    id: 'hugging-face-hub',
    name: 'Hugging Face Hub (model weights downloaded at container start)',
    class: 'public-source',
    role: 'no-content',
    hosts: ['huggingface.co', '*.huggingface.co', '*.hf.co'],
    sdks: [{ ecosystem: 'pypi', name: 'transformers' }, { ecosystem: 'pypi', name: 'colpali-engine' }],
    signals: [{ label: 'model weights loaded by id with from_pretrained', path: 'Data Plane v2/services/(colqwen-reranker|media-embedder)/app\\.py$', pattern: 'from_pretrained' }],
    gateEnv: [],
    note: 'The visual reranker and the media embedder load models by Hub id into a cache volume. Inference runs in the container; the download request (model id, container address) is what leaves. The reranker container is behind COLQWEN_ENDPOINT_URL.',
  },
  {
    id: 'github-api',
    name: 'GitHub (the customer\'s account)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['api.github.com', 'github.com'],
    gateEnv: ['GITHUB_CLIENT_ID', 'GITHUB_CLIENT_SECRET', 'GITHUB_WEBHOOK_SECRET', 'GITHUB_API_BASE_URL', 'GITHUB_AUTHORIZATION_URL', 'GITHUB_TOKEN_URL'],
  },
  {
    id: 'meta-platforms',
    name: 'Meta (Facebook, Instagram, Threads, Messenger) APIs and the Facebook JavaScript SDK',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['graph.facebook.com', 'www.facebook.com', 'graph.instagram.com', 'api.instagram.com', 'www.instagram.com', 'graph.threads.net', 'connect.facebook.net'],
    gateEnv: ['META_APP_CLIENT_SECRET', 'META_CLIENT_SECRET', 'META_WEBHOOK_SECRET', 'META_WEBHOOK_VERIFY_TOKEN', 'META_JS_SDK_APP_ID', 'FACEBOOK_CLIENT_SECRET', 'FACEBOOK_GRAPH_API_BASE_URL', 'INSTAGRAM_CLIENT_SECRET', 'INSTAGRAM_GRAPH_API_BASE_URL', 'THREADS_API_BASE_URL', 'FACEBOOK_AUTHORIZATION_URL', 'FACEBOOK_TOKEN_URL', 'INSTAGRAM_AUTHORIZATION_URL', 'INSTAGRAM_TOKEN_URL', 'META_AUTHORIZATION_URL', 'META_GRAPH_API_BASE_URL', 'META_TOKEN_URL'],
    note: 'Publishing and reading on pages and accounts the customer connects. The browser loads the Facebook SDK script from connect.facebook.net.',
  },
  {
    id: 'linkedin-api',
    name: 'LinkedIn',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['api.linkedin.com', 'www.linkedin.com'],
    gateEnv: ['LINKEDIN_CLIENT_SECRET', 'LINKEDIN_API_BASE_URL', 'LINKEDIN_API_VERSION', 'LINKEDIN_AUTHORIZATION_URL', 'LINKEDIN_TOKEN_URL'],
  },
  {
    id: 'x-api',
    name: 'X (Twitter)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['api.x.com', 'x.com'],
    gateEnv: ['X_CLIENT_SECRET', 'X_API_BASE_URL', 'X_AUTHORIZATION_URL', 'X_TOKEN_URL'],
  },
  {
    id: 'tiktok-api',
    name: 'TikTok',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['open.tiktokapis.com', 'www.tiktok.com'],
    gateEnv: ['TIKTOK_CLIENT_SECRET', 'TIKTOK_API_BASE_URL', 'TIKTOK_CLIENT_KEY', 'TIKTOK_AUTHORIZATION_URL', 'TIKTOK_TOKEN_URL'],
  },
  {
    id: 'snapchat-api',
    name: 'Snapchat',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['accounts.snapchat.com', 'adsapi.snapchat.com', 'businessapi.snapchat.com'],
    gateEnv: ['SNAPCHAT_CLIENT_SECRET', 'SNAPCHAT_API_BASE_URL', 'SNAPCHAT_AUTHORIZATION_URL', 'SNAPCHAT_BUSINESS_API_BASE_URL', 'SNAPCHAT_REDIRECT_BASE_URL', 'SNAPCHAT_TOKEN_URL'],
  },
  {
    id: 'discord-api',
    name: 'Discord',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['discord.com', '*.discord.com'],
    gateEnv: ['DISCORD_CLIENT_SECRET', 'DISCORD_BOT_TOKEN', 'DISCORD_API_BASE_URL', 'DISCORD_AUTHORIZATION_URL', 'DISCORD_TOKEN_URL'],
  },
  {
    id: 'shopify',
    name: 'Shopify (the customer\'s shop)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['*.myshopify.com'],
    gateEnv: ['SHOPIFY_CLIENT_SECRET', 'SHOPIFY_WEBHOOK_SECRET'],
    note: 'The shop host is built at run time.',
  },
  {
    id: 'visma-net',
    name: 'Visma.net (ERP) and a Visma MCP server',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['api.visma.net', 'connect.visma.com', 'mcp.finance.visma.net'],
    gateEnv: [],
    gatedBy: 'providers.visma.enabled in the core-gateway config.yaml (false in the shipped file)',
    note: 'The credentials come from the gateway\'s own config keys, which the shipped file leaves empty; the MCP host is an example URL in the settings page.',
  },
  {
    id: 'superoffice',
    name: 'SuperOffice (CRM)',
    class: 'user-authorised',
    role: 'customer-directed',
    hosts: ['online.superoffice.com', 'sod.superoffice.com'],
    gateEnv: [],
    gatedBy: 'providers.superoffice.enabled in the core-gateway config.yaml (false in the shipped file)',
    note: 'The credentials come from the gateway\'s own config keys, which the shipped file leaves empty.',
  },

  // ----- freight carriers -------------------------------------------------------------------
  {
    id: 'bring-carrier',
    name: 'Bring (Posten) freight booking',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: ['api.bring.com', 'developer.bring.com'],
    gateEnv: ['BRING_API_KEY', 'BRING_API_UID', 'BRING_LIVE_BOOKING'],
    note: 'Consignee name, address and contact details. The adapter is a mock when its variables are unset; bookings carry testIndicator unless BRING_LIVE_BOOKING is "true".',
  },
  {
    id: 'dhl-express',
    name: 'DHL Express (MyDHL API)',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: ['express.api.dhl.com'],
    gateEnv: ['DHL_API_KEY', 'DHL_API_SECRET', 'DHL_API_BASE_URL'],
    note: 'Consignee data. Mock when the variables are unset.',
  },
  {
    id: 'ups-carrier',
    name: 'UPS',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: ['onlinetools.ups.com', 'wwwcie.ups.com'],
    gateEnv: ['UPS_CLIENT_ID', 'UPS_CLIENT_SECRET'],
    note: 'Consignee data. Mock when the variables are unset.',
  },
  {
    id: 'fedex-carrier',
    name: 'FedEx',
    class: 'subprocessor',
    role: 'independent-recipient',
    hosts: ['apis.fedex.com', 'apis-sandbox.fedex.com'],
    gateEnv: ['FEDEX_CLIENT_ID', 'FEDEX_CLIENT_SECRET'],
    note: 'Consignee data. Mock when the variables are unset.',
  },

  // ----- public data sources (read-only; the request itself is what leaves) -----------------
  {
    id: 'brreg',
    name: 'Brønnøysundregistrene (Enhetsregisteret)',
    class: 'public-source',
    role: 'no-content',
    hosts: ['data.brreg.no'],
    gateEnv: ['BRREG_API_BASE_URL'],
    note: 'Company lookups by name or organisation number. The trust page cites this register as a grounding source.',
  },
  {
    id: 'no-public-authorities',
    name: 'Norwegian public data APIs (Lovdata, Stortinget, Riksarkivet, NAV, Digdir, SSB, Norges Bank, Kartverket, Geonorge, NVE, Miljødirektoratet, Entur, MET Norway, Statens vegvesen)',
    class: 'public-source',
    role: 'no-content',
    hosts: [
      'api.lovdata.no', 'data.stortinget.no', 'api.ra.no', 'pam-stilling-feed.nav.no', 'sparql.fellesdatakatalog.digdir.no',
      'data.ssb.no', 'www.ssb.no', 'data.norges-bank.no', 'api.kartverket.no', 'ws.geonorge.no', 'api01.nve.no',
      'api-luftmalinger.miljodirektoratet.no', 'api.entur.io', 'api.met.no', 'frost.met.no', 'nvdbapiles.atlas.vegvesen.no',
      'trafikkdata-api.atlas.vegvesen.no', 'datex-server-get-v3-1.atlas.vegvesen.no',
    ],
    gateEnv: ['LOVDATA_API_KEY', 'DATEX_URL', 'DATEX_USERNAME', 'DATEX_PASSWORD', 'FROST_URL', 'FROST_CLIENT_ID'],
    wiring: 'mixed',
    gatedBy: 'Lovdata, Statens vegvesen (DATEX) and MET (Frost) need a key or client id; the other authorities need none',
    note: 'information-core and the Quarry official-series collectors read public APIs. What leaves is the request (a query term, a place name, a table id) and the platform\'s egress address.',
  },
  {
    id: 'eu-procurement',
    name: 'EU TED and the Doffin notice store',
    class: 'public-source',
    role: 'no-content',
    hosts: ['api.ted.europa.eu', 'adaapnedataprodst.blob.core.windows.net'],
    gateEnv: [],
    note: 'A Quarry procurement collector; the TED query text is user-supplied.',
  },
  {
    id: 'osm-nominatim',
    name: 'OpenStreetMap Nominatim geocoder (public instance)',
    class: 'public-source',
    role: 'independent-recipient',
    hosts: ['nominatim.openstreetmap.org'],
    gateEnv: [],
    note: 'The place or address string to geocode is sent to the public instance.',
  },
  {
    id: 'no-news-feeds',
    name: 'Norwegian news sites (RSS read: NRK, VG, DN, E24, Aftenposten, Dagbladet, Tek, Digi)',
    class: 'public-source',
    role: 'no-content',
    hosts: ['www.nrk.no', 'www.vg.no', 'www.dn.no', 'e24.no', 'www.aftenposten.no', 'www.dagbladet.no', 'www.tek.no', 'www.digi.no'],
    gateEnv: [],
    note: 'Feed reads by information-core; nothing but the request leaves.',
  },
  {
    id: 'abuse-ch-feeds',
    name: 'abuse.ch URLhaus and PhishTank blocklist downloads',
    class: 'public-source',
    role: 'no-content',
    hosts: ['urlhaus.abuse.ch', 'www.phishtank.net', 'data.phishtank.com'],
    gateEnv: ['URLHAUS_CSV_URL', 'PHISHTANK_CSV_URL'],
    note: 'Downloads feed snapshots for the Quarry URL pre-flight check; nothing is sent.',
  },
  {
    id: 'geofabrik',
    name: 'Geofabrik OpenStreetMap extracts',
    class: 'public-source',
    role: 'no-content',
    hosts: ['download.geofabrik.de'],
    gateEnv: [],
    note: 'A self-owned data profile downloads map extracts; nothing is sent.',
  },

  // ----- browser-side assets and infrastructure ---------------------------------------------
  {
    id: 'google-fonts',
    name: 'Google Fonts (fonts.googleapis.com, fonts.gstatic.com)',
    class: 'infrastructure',
    role: 'no-content',
    hosts: ['fonts.googleapis.com', 'fonts.gstatic.com'],
    gateEnv: [],
    note: 'The web app\'s index.html links the stylesheet from Google, so each visitor\'s browser sends its IP address and user agent to Google. Ungated.',
  },
  {
    id: 'google-search-and-favicons',
    name: 'Google web search link and favicon service (www.google.com)',
    class: 'infrastructure',
    role: 'independent-recipient',
    hosts: ['www.google.com'],
    gateEnv: [],
    note: 'The browser asks Google\'s favicon service for the hostname of each web search result shown, and the omnibox falls back to a Google search URL for a typed query. Ungated.',
  },
  {
    id: 'unpkg-cdn',
    name: 'unpkg CDN (Swagger UI assets on the auth service\'s API documentation page)',
    class: 'infrastructure',
    role: 'no-content',
    hosts: ['unpkg.com'],
    gateEnv: [],
    note: 'The API documentation page loads its script and stylesheet from unpkg.',
  },
  {
    id: 'stock-media',
    name: 'Stock media hotlinks (Pexels, Unsplash)',
    class: 'infrastructure',
    role: 'no-content',
    hosts: ['images.pexels.com', 'videos.pexels.com', 'www.pexels.com', 'images.unsplash.com', 'unsplash.com', 'plus.unsplash.com'],
    gateEnv: [],
    note: 'Images and a video are loaded from the stock sites by the visitor\'s browser. Which pages ship to production was not checked.',
  },
  {
    id: 'marketing-site-assets',
    name: 'Marketing-site assets (Webflow CDN, Framer, Tailus, Cal.com)',
    class: 'infrastructure',
    role: 'no-content',
    hosts: ['cdn.prod.website-files.com', 'framerusercontent.com', 'html.tailus.io', 'cal.com'],
    gateEnv: [],
    note: 'Hotlinked assets and a booking link on the marketing sites.',
  },
  {
    id: 's3-compatible-object-storage',
    name: 'S3-compatible object storage clients (MinIO in the compose files)',
    class: 'infrastructure',
    role: 'unknown',
    hosts: [],
    sdks: [{ ecosystem: 'cargo', name: 'aws-sdk-s3' }, { ecosystem: 'cargo', name: 'aws-sigv4' }],
    gateEnv: ['MINIO_ENDPOINT'],
    note: 'The S3 client libraries talk to the MinIO container in every compose file read. If production points them at a cloud object store, that store is a sub-processor; the production endpoint is UNVERIFIED.',
  },
]

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/**
 * The component a file belongs to, for the "where in the code" column: the plane and the
 * service (or app) directory. Examples: "Model Plane/inference-core",
 * "Frontend Plane/verevonv3/gateway", "Ingestion Plane/integration-corev2".
 */
export function componentOf(relative) {
  const parts = relative.split('/')
  if (parts[0] !== 'apps' || parts.length < 3) return parts.length > 1 ? parts[0] : '(root)'
  const plane = parts[1]
  const directories = parts.slice(2, -1)
  const servicesAt = directories.indexOf('services')
  if (servicesAt !== -1 && directories[servicesAt + 1]) return `${plane}/${directories[servicesAt + 1]}`
  if (plane === 'Frontend Plane' && directories[0]) {
    if (directories[1] === 'apps' && directories[2]) return `${plane}/${directories[0]}/${directories[2]}`
    return directories[1] ? `${plane}/${directories[0]}/${directories[1]}` : `${plane}/${directories[0]}`
  }
  return directories[0] ? `${plane}/${directories[0]}` : plane
}

/** Names that only redirect a host (a base URL, an OAuth endpoint, an API version): not a switch. */
export const OVERRIDE_NAME = /(_BASE_URL|_API_BASE|_AUTHORIZATION_URL|_TOKEN_URL|_API_VERSION|_CHECKOUT_JS_URL|_CLIENT_URL|_BACKEND_URL|_TOKEN_ORIGIN|_REDIRECT_BASE_URL|_DIRECT_PROMPT_MODE)$/

/** Names that look like a credential. Used only to list names no vendor claims. */
export const CREDENTIAL_NAME = /(API_KEY|APIKEY|SECRET_KEY|PRIVATE_KEY|CLIENT_SECRET|ACCESS_TOKEN|AUTH_TOKEN|BOT_TOKEN|_SECRET|_TOKEN|_PASSWORD|_DSN)$/
/**
 * Credential-like names that were read and found to be this platform's own plumbing, not a
 * vendor: service-to-service tokens and passwords of self-hosted stores. Each carries the
 * reason, which the report prints, so "reviewed" is visible. A NEW credential-like name that
 * matches neither a vendor nor this list fails `--strict`: someone must look at it.
 */
export const INTERNAL_ENV = [
  { test: /(_SERVICE_(TOKEN|API_KEY|SECRET)|(^|_)INTERNAL_(API_KEY|TOKEN|SECRET|SERVICE_TOKEN))$/, reason: 'service-to-service credential between this platform\'s own services' },
  { test: /_CONTROL_[A-Z_]*TOKEN$|^(APPLICATION|ORCHESTRATOR)_[A-Z_]*TOKEN$|^CONTROL_(AUTH_TOKEN|SHARED_BRIDGE_PASSWORD)$/, reason: 'control-plane token between this platform\'s own planes' },
  { test: /NATS_(SHARED_)?(PASSWORD|TOKEN)$|NATS_AUTH_TOKEN$/, reason: 'credential for the platform\'s own NATS broker' },
  { test: /^(CACHE|DATABASE|DB|DRAGONFLY|REDIS|SONIC)_PASSWORD$|^MINIO_SECRET_KEY$|_DSN$|^LAGO_API_KEY$/, reason: 'password, key or DSN of a store this platform runs itself (Postgres, Redis, Dragonfly, Sonic, MinIO, Lago)' },
  { test: /^(JWT_SECRET|BETTER_AUTH_SECRET)$|_ATTESTATION_PRIVATE_KEY$/, reason: 'the platform\'s own signing secret or key (sessions, capability attestations)' },
  { test: /^(AGENT_MEMORY_TOKEN|AI_CORE_API_KEY|BROWSER_AGENT_API_KEY|CODEX_SUBSCRIPTION_MODEL_PLANE_API_KEY|CORE_INFRA_OPERATOR_TOKEN|EDGE_AUTH_TOKEN|RUNTIME_AUTH_TOKEN|QUARRY_EDGE__RUNTIME_AUTH_TOKEN|QUARRY_CONTROL_API_KEY|MAIL_TASK_IMPORTS_CORE_TOKEN|AUTH_CORE_INTERNAL_API_KEY)$/, reason: 'key between this platform\'s own services (model plane, Quarry, the imports service, the infra gateway)' },
  { test: /^FINSPO_API_KEY$/, reason: 'key for finspo-core, a service in this repository (its compose default is the in-cluster name finspo-api)' },
  { test: /^SCIM_BEARER_TOKEN$/, reason: 'inbound bearer token a customer\'s identity provider presents to our SCIM endpoint (we receive, we do not send)' },
  { test: /^VEREVON_AGENT_(PERMANENT_)?PASSWORD$/, reason: 'password of this platform\'s own endpoint agent (Endpoint Plane)' },
  { test: /^ZAMMAD_[A-Z_]*(TOKEN|SECRET)$/, reason: 'token of the Zammad help desk the repository deploys itself (docker-compose.zammad.yml)' },
]

function readText(absolute) {
  const buffer = fs.readFileSync(absolute)
  if (buffer.includes(0)) return null
  return buffer.toString('utf8')
}

function lineOrigin(relative, lineNumber) {
  return `${relative}:${lineNumber}`
}

/** A line that names its URL as an identifier: openAIAuthClaim = "https://api.openai.com/auth" is a claim name. */
const IDENTIFIER_LINE = /claim|namespace|xmlns|"\$schema"/i

/** Scan one file's text. Returns { hits: [...], dynamicCount }. Exported for tests. */
export function scanText(relative, text, kind = 'source') {
  const extension = extensionOf(relative)
  const lines = text.split('\n').map((line) => line.replace(/\r$/, ''))
  const mask = extension === 'rs' ? rustTestLineMask(lines) : null
  const blockMask = blockCommentMask(lines, extension)
  const hits = []
  let dynamicCount = 0
  for (let index = 0; index < lines.length; index += 1) {
    if (mask && mask[index]) continue
    const line = lines[index]
    if (!line.includes('://')) continue
    for (const url of findUrlsInLine(line)) {
      if (url.dynamic) {
        dynamicCount += 1
        continue
      }
      hits.push({
        host: url.host,
        scheme: url.scheme,
        line: index + 1,
        // a URL used as an identifier (a JWT claim name, an XML namespace, a $schema) is not a request target
        where: blockMask[index] || isCommentAt(line, url.index, extension) || IDENTIFIER_LINE.test(line) ? 'comment' : 'code',
        fileKind: kind,
      })
    }
  }
  return { hits, dynamicCount }
}

/** Declared SDK dependencies in one manifest. Returns [{ ecosystem, name, line, dev }]. */
export function scanManifest(relative, text) {
  const name = relative.split('/').pop()
  const lines = text.split('\n').map((line) => line.replace(/\r$/, ''))
  const found = []
  if (name === 'package.json') {
    let section = null
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index]
      const open = /^\s{1,4}"(dependencies|devDependencies|optionalDependencies|peerDependencies)"\s*:\s*\{/.exec(line)
      if (open) {
        section = open[1]
        continue
      }
      if (section && /^\s{1,4}\}/.test(line)) {
        section = null
        continue
      }
      if (section && section !== 'devDependencies') {
        const dependency = /^\s*"([^"]+)"\s*:\s*"/.exec(line)
        if (dependency) found.push({ ecosystem: 'npm', name: dependency[1], line: index + 1 })
      }
    }
  } else if (name === 'Cargo.toml') {
    let live = false
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index]
      const header = /^\s*\[([^\]]+)\]/.exec(line)
      if (header) {
        const title = header[1].trim()
        live = /(^|\.)dependencies$/.test(title) && !/dev-dependencies|build-dependencies/.test(title)
        const single = /^(?:workspace\.|target\.[^.]+\.)?dependencies\.(.+)$/.exec(title)
        if (single && !/dev-dependencies/.test(title)) found.push({ ecosystem: 'cargo', name: single[1].trim(), line: index + 1 })
        continue
      }
      if (live) {
        const dependency = /^\s*([A-Za-z0-9_-]+)\s*(?:=|\.)/.exec(line)
        if (dependency) found.push({ ecosystem: 'cargo', name: dependency[1], line: index + 1 })
      }
    }
  } else if (name === 'go.mod') {
    for (let index = 0; index < lines.length; index += 1) {
      const dependency = /^\s*(?:require\s+)?([a-z0-9.-]+\.[a-z]{2,}\/[^\s]+)\s+v[0-9]/i.exec(lines[index])
      if (dependency) found.push({ ecosystem: 'go', name: dependency[1], line: index + 1 })
    }
  } else if (/^requirements/i.test(name)) {
    for (let index = 0; index < lines.length; index += 1) {
      const dependency = /^\s*([A-Za-z0-9_.-]+)\s*(?:[=<>~!;\[]|$)/.exec(lines[index])
      if (dependency && !lines[index].trim().startsWith('#')) found.push({ ecosystem: 'pypi', name: dependency[1].toLowerCase(), line: index + 1 })
    }
  } else if (name === 'pyproject.toml') {
    let live = false
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index]
      if (/^\s*dependencies\s*=\s*\[/.test(line)) live = true
      else if (live && /^\s*\]/.test(line)) live = false
      if (live) {
        const dependency = /"([A-Za-z0-9_.-]+)\s*(?:[=<>~!;\[]|")/.exec(line)
        if (dependency) found.push({ ecosystem: 'pypi', name: dependency[1].toLowerCase(), line: index + 1 })
      }
    }
  }
  return found
}

function sdkMatches(sdk, dependency) {
  if (sdk.ecosystem !== dependency.ecosystem) return false
  if (sdk.name.endsWith('*')) return dependency.name.startsWith(sdk.name.slice(0, -1))
  return sdk.name === dependency.name
}

function firstN(list, n) {
  return list.slice(0, n)
}

/** Orders "path:line" origins by path, then by line number (so line 260 comes before line 1370). */
function compareOrigins(a, b) {
  const at = a.lastIndexOf(':')
  const bt = b.lastIndexOf(':')
  const pathOrder = compareStrings(a.slice(0, at), b.slice(0, bt))
  return pathOrder !== 0 ? pathOrder : Number(a.slice(at + 1)) - Number(b.slice(bt + 1))
}

function compareStrings(a, b) {
  return a < b ? -1 : a > b ? 1 : 0
}

/** Does a compose or env line give `name` a default, and is it empty? Never returns the value. */
export function defaultStateInLine(line, name) {
  const start = line.indexOf(`\${${name}`)
  if (start !== -1) {
    const after = line.slice(start + 2 + name.length)
    const operator = /^(:?[-?+])/.exec(after)
    if (!operator) return after.startsWith('}') ? 'no-default' : null
    let depth = 1
    let text = ''
    for (const char of after.slice(operator[1].length)) {
      if (char === '{') depth += 1
      else if (char === '}') {
        depth -= 1
        if (depth === 0) break
      }
      text += char
    }
    if (operator[1].includes('?')) return 'required'
    // a default that is itself a variable with an empty default is an empty default
    const emptyDefault = text.trim() === '' || /^\$\{[A-Z][A-Z0-9_]*(:?-)?\}$/.test(text.trim())
    return emptyDefault ? 'empty-default' : 'non-empty-default'
  }
  if (new RegExp(`\\$${name}\\b`).test(line)) return 'no-default'
  const assignment = new RegExp(`(^|\\s)${name}\\s*[:=]\\s*(.*)$`).exec(line)
  if (assignment) {
    const value = assignment[2].replace(/\s+#.*$/, '').trim().replace(/^["']|["']$/g, '')
    return value === '' ? 'empty-default' : 'non-empty-default'
  }
  return null
}

/**
 * Scan a repository. `vendors` is the catalogue. Returns the result object that the
 * renderers and the verifier read.
 */
export function scan(options) {
  const root = path.resolve(options.root)
  const vendors = options.vendors ?? VENDORS
  const ignoreRules = options.ignoreRules ?? IGNORE_RULES
  const window = options.window ?? 10
  const listing = listFiles(root, { git: options.git })

  const excluded = new Map()
  const hostIndex = new Map() // host -> { code, comment, evidence: [{origin, where}], files:Set, components:Set }
  const manifestDeps = []
  const envLines = new Map() // env name -> [{ origin, kind, state }]   (compose and env files)
  const sourceEnvReads = new Map() // env name -> [origin]            (source files)
  const rustFields = new Map() // struct field name -> origin (non-test Rust)
  const signalSpecs = [] // one per catalogue signal: { vendorId, label, path, pattern, hits }
  for (const vendor of vendors) {
    for (const signal of vendor.signals ?? []) {
      signalSpecs.push({ vendorId: vendor.id, label: signal.label, path: new RegExp(signal.path), pattern: new RegExp(signal.pattern, 'i'), hits: [] })
    }
  }
  let scanned = 0
  let dynamicTotal = 0
  const fileHostHits = new Map() // relative -> [{host, line, where}]
  const fileLines = new Map()

  for (const relative of listing.files) {
    const decision = classifyFile(relative)
    if (!decision.scan) {
      excluded.set(decision.reason, (excluded.get(decision.reason) ?? 0) + 1)
      continue
    }
    let text
    try {
      const absolute = path.join(root, relative)
      if (fs.statSync(absolute).size > MAX_FILE_BYTES) {
        excluded.set('oversize', (excluded.get('oversize') ?? 0) + 1)
        continue
      }
      text = readText(absolute)
    } catch {
      excluded.set('unreadable', (excluded.get('unreadable') ?? 0) + 1)
      continue
    }
    if (text === null) {
      excluded.set('binary-or-nul', (excluded.get('binary-or-nul') ?? 0) + 1)
      continue
    }
    scanned += 1

    if (decision.kind === 'manifest') {
      for (const dependency of scanManifest(relative, text)) manifestDeps.push({ ...dependency, origin: lineOrigin(relative, dependency.line) })
      continue
    }

    const lines = text.split('\n').map((line) => line.replace(/\r$/, ''))
    const extension = extensionOf(relative)
    const mask = extension === 'rs' ? rustTestLineMask(lines) : null
    const blockMask = blockCommentMask(lines, extension)

    // environment names: compose and env files are "wiring"; source files are "reads"
    const signalsForFile = signalSpecs.filter((spec) => spec.path.test(relative))
    for (let index = 0; index < lines.length; index += 1) {
      if (mask && mask[index]) continue
      const line = lines[index]
      if (isConfigKind(decision.kind)) {
        const names = new Set(envNamesInLine(line).keys())
        const assignment = /^\s*(?:export\s+)?(?:-\s+)?([A-Z][A-Z0-9_]+)\s*[:=]/.exec(line)
        if (assignment) names.add(assignment[1])
        for (const name of names) {
          if (!envLines.has(name)) envLines.set(name, [])
          envLines.get(name).push({ origin: lineOrigin(relative, index + 1), kind: decision.kind, state: defaultStateInLine(line, name) })
        }
      } else {
        for (const name of envNamesInLine(line).keys()) {
          if (!sourceEnvReads.has(name)) sourceEnvReads.set(name, [])
          sourceEnvReads.get(name).push(lineOrigin(relative, index + 1))
        }
        // Rust config structs: the config crate maps QUARRY_EDGE__BRAVE_SEARCH_KEY to the field brave_search_key
        const field = /^\s*pub\s+([a-z][a-z0-9_]*)\s*:/.exec(line)
        if (field && extension === 'rs' && !rustFields.has(field[1])) rustFields.set(field[1], lineOrigin(relative, index + 1))
        if (signalsForFile.length > 0 && !blockMask[index] && !isCommentAt(line, line.search(/\S/), extension)) {
          for (const spec of signalsForFile) {
            if (spec.pattern.test(line)) spec.hits.push(lineOrigin(relative, index + 1))
          }
        }
      }
    }

    const result = scanText(relative, text, decision.kind)
    dynamicTotal += result.dynamicCount
    if (result.hits.length > 0) {
      fileHostHits.set(relative, result.hits)
      fileLines.set(relative, lines)
    }
    for (const hit of result.hits) {
      if (!hostIndex.has(hit.host)) hostIndex.set(hit.host, { code: 0, config: 0, comment: 0, evidence: [], files: new Set(), components: new Set() })
      const entry = hostIndex.get(hit.host)
      // comment: in a comment; config: in a compose or env file; code: in source or a service config file
      const where = hit.where === 'comment' ? 'comment' : isConfigKind(hit.fileKind) ? 'config' : 'code'
      entry[where] += 1
      entry.files.add(relative)
      if (where !== 'comment') entry.components.add(componentOf(relative))
      entry.evidence.push({ origin: lineOrigin(relative, hit.line), where, fileKind: hit.fileKind })
    }
  }

  // classify hosts
  const ignoredByRule = new Map()
  const unclassified = []
  const commentOnly = []
  const vendorHosts = new Map() // vendor id -> [{host, entry}]
  for (const host of [...hostIndex.keys()].sort(compareStrings)) {
    const entry = hostIndex.get(host)
    const rule = ignoreReasonFor(host, ignoreRules)
    if (rule) {
      const bucket = ignoredByRule.get(rule.id) ?? { id: rule.id, reason: rule.reason, hosts: 0, hits: 0 }
      bucket.hosts += 1
      bucket.hits += entry.code + entry.config + entry.comment
      ignoredByRule.set(rule.id, bucket)
      continue
    }
    const vendor = vendorForHost(host, vendors)
    if (vendor) {
      if (!vendorHosts.has(vendor.id)) vendorHosts.set(vendor.id, [])
      vendorHosts.get(vendor.id).push({ host, entry })
    } else {
      const row = {
        host,
        code: entry.code,
        config: entry.config,
        comment: entry.comment,
        components: [...entry.components].sort(compareStrings),
        evidence: entry.evidence.slice().sort((a, b) => compareOrigins(a.origin, b.origin)),
      }
      // a host that only ever appears in comments is a documentation link, not an address the code uses
      if (entry.code + entry.config === 0) commentOnly.push(row)
      else unclassified.push(row)
    }
  }

  // vendors
  const maxEvidence = options.maxEvidence ?? 4
  const reportVendors = []
  const allClaimed = new Set(vendors.flatMap((vendor) => vendor.gateEnv ?? []))
  const spread = (origins) => firstN([...new Set(origins)].sort(compareOrigins), maxEvidence)
  for (const vendor of vendors) {
    const hostRows = (vendorHosts.get(vendor.id) ?? []).map(({ host, entry }) => ({
      host,
      codeHits: entry.code,
      configHits: entry.config,
      commentHits: entry.comment,
      files: entry.files.size,
      evidence: spread(entry.evidence.filter((item) => item.where === 'code').map((item) => item.origin)),
      configEvidence: spread(entry.evidence.filter((item) => item.where === 'config').map((item) => item.origin)),
      commentEvidence: spread(entry.evidence.filter((item) => item.where === 'comment').map((item) => item.origin)),
    }))
    const sdkRows = []
    for (const sdk of vendor.sdks ?? []) {
      for (const dependency of manifestDeps) if (sdkMatches(sdk, dependency)) sdkRows.push({ ecosystem: dependency.ecosystem, name: dependency.name, origin: dependency.origin })
    }
    sdkRows.sort((a, b) => compareOrigins(a.origin, b.origin))

    const signalRows = signalSpecs
      .filter((spec) => spec.vendorId === vendor.id)
      .map((spec) => ({ label: spec.label, hits: spec.hits.length, evidence: spread(spec.hits) }))
      .filter((row) => row.hits > 0)

    // gate env names: curated, verified in source, and looked up in compose and env files
    const gates = []
    for (const name of vendor.gateEnv ?? []) {
      const reads = (sourceEnvReads.get(name) ?? []).slice().sort(compareOrigins)
      let readKind = reads.length > 0 ? 'literal' : null
      let firstRead = reads[0] ?? null
      if (reads.length === 0 && name.includes('__')) {
        // the config crate maps PREFIX__FIELD_NAME to a struct field called field_name
        const field = name.split('__').pop().toLowerCase()
        if (rustFields.has(field)) {
          readKind = 'rust-config-field'
          firstRead = rustFields.get(field)
        }
      }
      const kindRank = { compose: 0, env: 1, 'compose-legacy': 2, 'env-legacy': 3 }
      const wiring = (envLines.get(name) ?? []).slice().sort((a, b) => kindRank[a.kind] - kindRank[b.kind] || compareOrigins(a.origin, b.origin))
      gates.push({
        name,
        read: readKind,
        readInSource: reads.length + (readKind === 'rust-config-field' ? 1 : 0),
        firstRead,
        wiredIn: wiring.length,
        firstWiring: wiring[0]?.origin ?? null,
        firstWiringKind: wiring[0]?.kind ?? null,
        // defaults of the canonical files when there are any; the legacy root files only otherwise
        defaultState: summariseDefault((wiring.some((item) => !item.kind.endsWith('legacy')) ? wiring.filter((item) => !item.kind.endsWith('legacy')) : wiring).map((item) => item.state)),
      })
    }

    // names seen near code hits that the catalogue does not list
    // (kept only when unclaimed by any vendor and the first word of the name belongs to this vendor)
    const known = new Set(vendor.gateEnv ?? [])
    const ownWords = new Set([...(vendor.gateEnv ?? []).map((name) => name.split('_')[0]), ...vendor.id.split('-').map((word) => word.toUpperCase())])
    const near = new Map()
    const hostSet = new Set((vendorHosts.get(vendor.id) ?? []).map((row) => row.host))
    for (const [relative, hits] of fileHostHits) {
      const lines = fileLines.get(relative)
      for (const hit of hits) {
        if (!hostSet.has(hit.host) || hit.where !== 'code' || isConfigKind(hit.fileKind)) continue
        const from = Math.max(0, hit.line - 1 - window)
        const to = Math.min(lines.length - 1, hit.line - 1 + window)
        for (let i = from; i <= to; i += 1) {
          for (const name of gatingNamesInLine(lines[i])) {
            if (known.has(name) || allClaimed.has(name) || !ownWords.has(name.split('_')[0])) continue
            if ((options.internalEnv ?? INTERNAL_ENV).some((rule) => rule.test.test(name))) continue
            if (!(sourceEnvReads.has(name) || envLines.has(name))) continue
            if (!near.has(name)) near.set(name, lineOrigin(relative, i + 1))
          }
        }
      }
    }
    const seenNear = [...near.entries()].sort((a, b) => compareStrings(a[0], b[0])).map(([name, origin]) => ({ name, origin }))

    const codeHits = hostRows.reduce((sum, row) => sum + row.codeHits, 0)
    const configHits = hostRows.reduce((sum, row) => sum + row.configHits, 0)
    const commentHits = hostRows.reduce((sum, row) => sum + row.commentHits, 0)
    // a variable that only redirects a host (a base URL, an OAuth endpoint) is not what switches a vendor on
    const switches = gates.filter((gate) => !OVERRIDE_NAME.test(gate.name))
    const envRead = switches.some((gate) => gate.readInSource > 0)
    const envWired = switches.some((gate) => gate.wiredIn > 0)
    const components = []
    for (const row of vendorHosts.get(vendor.id) ?? []) components.push(...row.entry.components)
    for (const gate of vendor.gateEnv ?? []) for (const origin of sourceEnvReads.get(gate) ?? []) components.push(componentOf(origin.split(':')[0]))
    for (const row of signalRows) for (const origin of row.evidence) components.push(componentOf(origin.split(':')[0]))
    let evidenceStatus = 'none'
    if (codeHits > 0) evidenceStatus = 'code'
    else if (configHits > 0) evidenceStatus = 'config-file'
    else if (envRead) evidenceStatus = 'env-only'
    else if (envWired) evidenceStatus = 'compose-only'
    else if (sdkRows.length > 0) evidenceStatus = 'sdk-only'
    else if (signalRows.length > 0) evidenceStatus = 'signal-only'
    else if (commentHits > 0) evidenceStatus = 'comment-only'

    // Is the vendor switched on by anything in this repository? "ungated": no gating variable,
    // so it is reachable whenever the feature runs. "config-flag": gated by a flag in a service
    // config file, not by a variable. "none": gated, and no compose or env file in this
    // repository mentions a gating variable. Otherwise "compose" or "env-example-only".
    let configWiring = vendor.wiring ?? (vendor.gatedBy ? 'config-flag' : evidenceStatus === 'comment-only' ? 'n/a' : 'ungated')
    if (switches.length > 0 && !vendor.wiring) {
      const kinds = new Set()
      for (const gate of switches) for (const item of envLines.get(gate.name) ?? []) kinds.add(item.kind)
      configWiring = kinds.has('compose') ? 'compose' : kinds.has('env') ? 'env-example-only' : kinds.has('compose-legacy') ? 'legacy-compose-only' : kinds.has('env-legacy') ? 'legacy-env-only' : 'none'
    }

    reportVendors.push({
      id: vendor.id,
      name: vendor.name,
      class: vendor.class,
      role: vendor.role ?? null,
      region: vendor.region ?? 'TBC',
      dpa: vendor.dpa ?? 'TBC',
      note: vendor.note ?? null,
      gatedBy: vendor.gatedBy ?? null,
      evidenceStatus,
      configWiring,
      components: [...new Set(components)].sort(compareStrings),
      hosts: hostRows,
      sdks: sdkRows,
      signals: signalRows,
      gates,
      seenNear,
    })
  }

  // credential-like names read in source that no catalogue vendor claims: each is either
  // reviewed as internal plumbing (with a reason) or listed here as needing a decision
  const claimed = new Set(vendors.flatMap((vendor) => vendor.gateEnv ?? []))
  const unmappedCredentials = []
  const reviewedInternal = []
  for (const [name, origins] of sourceEnvReads) {
    if (claimed.has(name) || !CREDENTIAL_NAME.test(name)) continue
    const first = origins.slice().sort(compareOrigins)[0]
    const reviewed = options.internalEnv === undefined ? INTERNAL_ENV : options.internalEnv
    const rule = reviewed.find((item) => item.test.test(name))
    if (rule) reviewedInternal.push({ name, reason: rule.reason, first })
    else unmappedCredentials.push({ name, reads: origins.length, first })
  }
  unmappedCredentials.sort((a, b) => compareStrings(a.name, b.name))
  reviewedInternal.sort((a, b) => compareStrings(a.name, b.name))

  const found = reportVendors.filter((vendor) => vendor.evidenceStatus !== 'none')
  const notFound = reportVendors.filter((vendor) => vendor.evidenceStatus === 'none').map((vendor) => vendor.id)
  return {
    tool: 'scan-egress',
    schema: 1,
    fileSelection: listing.method,
    filesListed: listing.files.length,
    filesScanned: scanned,
    excluded: Object.fromEntries([...excluded.entries()].sort((a, b) => compareStrings(a[0], b[0]))),
    dynamicUrlLiterals: dynamicTotal,
    vendors: found.sort((a, b) => compareStrings(a.id, b.id)),
    vendorsNotFound: notFound.sort(compareStrings),
    unclassified: unclassified.sort((a, b) => compareStrings(a.host, b.host)),
    commentOnlyHosts: commentOnly.sort((a, b) => compareStrings(a.host, b.host)),
    unmappedCredentialNames: unmappedCredentials,
    reviewedInternalNames: reviewedInternal,
    ignored: [...ignoredByRule.values()].sort((a, b) => compareStrings(a.id, b.id)),
  }
}

function summariseDefault(states) {
  const present = new Set(states.filter(Boolean))
  if (states.length === 0) return 'not-wired'
  if (present.size === 0) return 'wired-no-default-seen'
  const order = ['non-empty-default', 'empty-default', 'no-default', 'required']
  return order.filter((state) => present.has(state)).join('+')
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

export function renderJson(result) {
  return `${JSON.stringify(result, null, 2)}\n`
}

export function renderText(result, options = {}) {
  const limit = options.limit ?? 400
  const out = []
  out.push('scan-egress report (generated; do not edit)')
  out.push(`file selection: ${result.fileSelection}; listed ${result.filesListed}; scanned ${result.filesScanned}`)
  out.push(`excluded by rule: ${Object.entries(result.excluded).map(([reason, count]) => `${reason} ${count}`).join(', ') || 'none'}`)
  out.push(`dynamic URL literals (host built at run time, not listed): ${result.dynamicUrlLiterals}`)
  out.push('')
  out.push(`vendors found: ${result.vendors.length}`)
  for (const vendor of result.vendors) {
    out.push('')
    out.push(`[${vendor.id}] ${vendor.name}`)
    out.push(`  class=${vendor.class} role=${vendor.role ?? '-'} evidence=${vendor.evidenceStatus} config-wiring=${vendor.configWiring} region=${vendor.region} dpa=${vendor.dpa}`)
    out.push(`  components: ${vendor.components.join(', ') || '-'}`)
    for (const host of vendor.hosts) {
      out.push(`  host ${host.host}  code=${host.codeHits} config=${host.configHits} comment=${host.commentHits} files=${host.files}`)
      for (const origin of host.evidence) out.push(`    code     ${origin}`)
      for (const origin of host.configEvidence) out.push(`    config   ${origin}`)
      for (const origin of host.commentEvidence) out.push(`    comment  ${origin}`)
    }
    for (const sdk of vendor.sdks) out.push(`  sdk ${sdk.ecosystem}:${sdk.name}  ${sdk.origin}`)
    for (const signal of vendor.signals) out.push(`  signal "${signal.label}" x${signal.hits}  ${signal.evidence.join(' | ')}`)
    for (const gate of vendor.gates) {
      out.push(`  env ${gate.name}  read=${gate.read ?? 'no'}${gate.firstRead ? ` (${gate.firstRead})` : ''}  wired=${gate.wiredIn}${gate.firstWiring ? ` (${gate.firstWiring}, ${gate.firstWiringKind})` : ''}  default=${gate.defaultState}`)
    }
    for (const near of vendor.seenNear) out.push(`  seen near a code hit, not in the catalogue: ${near.name}  ${near.origin}`)
  }
  out.push('')
  out.push(`unclassified hosts (code or config hit, no vendor, no ignore rule): ${result.unclassified.length}`)
  for (const item of result.unclassified.slice(0, limit)) {
    out.push(`  ${item.host}  code=${item.code} config=${item.config} comment=${item.comment}  ${item.evidence.slice(0, 2).map((e) => e.origin).join(' | ')}`)
  }
  out.push('')
  out.push(`comment-only hosts (documentation links, no code uses them, not in the catalogue): ${result.commentOnlyHosts.length}`)
  for (const item of result.commentOnlyHosts.slice(0, limit)) out.push(`  ${item.host}  ${item.evidence[0]?.origin ?? ''}`)
  out.push('')
  out.push(`credential-like variable names read in source that no vendor claims and no review covers: ${result.unmappedCredentialNames.length}`)
  for (const item of result.unmappedCredentialNames.slice(0, limit)) out.push(`  ${item.name}  ${item.first}`)
  out.push('')
  out.push(`credential-like variable names reviewed as internal plumbing: ${result.reviewedInternalNames.length}`)
  for (const item of result.reviewedInternalNames.slice(0, limit)) out.push(`  ${item.name}  ${item.first}  -- ${item.reason}`)
  out.push('')
  out.push('ignored hosts, by rule:')
  for (const rule of result.ignored) out.push(`  ${rule.id}: ${rule.hosts} host(s), ${rule.hits} hit(s)  -- ${rule.reason}`)
  if (result.vendorsNotFound.length > 0) {
    out.push('')
    out.push(`catalogue entries with no evidence in this tree: ${result.vendorsNotFound.join(', ')}`)
  }
  out.push('')
  return out.join('\n')
}

/**
 * The diffable form: one line per fact, no line numbers and no hit counts, so it only changes
 * when a vendor, a host, a variable, a dependency or a classification changes, not when
 * unrelated code moves. Keep a copy, run the tool again later, and diff the two.
 */
export function renderSummary(result) {
  const out = []
  out.push('scan-egress summary (generated; no line numbers, no counts; diff two of these to see what changed)')
  out.push(`vendors ${result.vendors.length}`)
  for (const vendor of result.vendors) {
    out.push(`vendor ${vendor.id} class=${vendor.class} role=${vendor.role ?? '-'} evidence=${vendor.evidenceStatus} wiring=${vendor.configWiring}`)
    for (const host of vendor.hosts) {
      const kinds = [host.codeHits > 0 ? 'code' : null, host.configHits > 0 ? 'config' : null, host.commentHits > 0 ? 'comment' : null].filter(Boolean)
      out.push(`  host ${host.host} ${kinds.join('+')}`)
    }
    for (const sdk of [...new Set(vendor.sdks.map((item) => `${item.ecosystem}:${item.name}`))]) out.push(`  sdk ${sdk}`)
    for (const signal of vendor.signals) out.push(`  signal ${signal.label}`)
    for (const gate of vendor.gates) out.push(`  env ${gate.name} read=${gate.read ?? 'no'} wired=${gate.firstWiringKind ?? 'no'} default=${gate.defaultState}`)
    for (const near of vendor.seenNear) out.push(`  near ${near.name}`)
    out.push(`  components ${vendor.components.join('; ')}`)
  }
  for (const item of result.unclassified) out.push(`unclassified ${item.host}`)
  for (const item of result.commentOnlyHosts) out.push(`comment-only ${item.host}`)
  for (const item of result.unmappedCredentialNames) out.push(`unmapped-credential ${item.name}`)
  out.push(`reviewed-internal-credentials ${result.reviewedInternalNames.length}`)
  for (const rule of result.ignored) out.push(`ignored ${rule.id} hosts=${rule.hosts}`)
  for (const id of result.vendorsNotFound) out.push(`catalogue-no-evidence ${id}`)
  out.push('')
  return out.join('\n')
}

// ---------------------------------------------------------------------------
// Verifying a markdown register against the scan
// ---------------------------------------------------------------------------

/**
 * A register names every vendor the scan found. The register is the markdown table whose
 * header starts with the cell "Id"; its rows start with the vendor id in backticks, for example:
 *
 *   | `resend` | Resend | ... | LISTED | ... |
 *   | `w3-schemas` | ... | EXCLUDED: schema URL, not a request target | ... |
 *
 * Rules: every vendor id in `result.vendors` has a row; every row's id is a catalogue id
 * (or an id in `extraIds`); a row that says EXCLUDED must give a reason of at least 12
 * characters after "EXCLUDED:". Returns an array of problems (empty when clean).
 */
export function verifyDocument(result, markdown, options = {}) {
  const problems = []
  const rows = new Map()
  // Only a table whose header starts with the cell "Id" is the register; other tables of the
  // document may start their rows with a backticked word too and are not read.
  let inRegister = false
  for (const line of markdown.split('\n')) {
    const trimmed = line.replace(/\r$/, '').trim()
    if (!trimmed.startsWith('|')) {
      inRegister = false
      continue
    }
    const firstCell = trimmed.slice(1).split('|')[0].trim()
    if (/^id$/i.test(firstCell.replace(/[*_]/g, ''))) {
      inRegister = true
      continue
    }
    if (!inRegister) continue
    const id = /^`([a-z0-9][a-z0-9-]*)`$/.exec(firstCell)
    if (!id) continue
    if (!rows.has(id[1])) rows.set(id[1], [])
    rows.get(id[1]).push(trimmed)
  }
  const known = new Set([...(options.knownIds ?? []), ...result.vendors.map((vendor) => vendor.id), ...result.vendorsNotFound])
  for (const vendor of result.vendors) {
    if (!rows.has(vendor.id)) problems.push(`vendor "${vendor.id}" (${vendor.name}) was found by the scan and has no row in the register`)
  }
  for (const [id, lines] of rows) {
    if (!known.has(id)) problems.push(`register row "${id}" is not a catalogue vendor id`)
    for (const line of lines) {
      const excluded = /EXCLUDED\b(?::\s*([^|]*))?/.exec(line)
      if (excluded && (excluded[1] ?? '').trim().length < 12) {
        problems.push(`register row "${id}" is marked EXCLUDED without a reason (write "EXCLUDED: <why>", 12 characters at least)`)
      }
      if (!excluded && !/\bLISTED\b/.test(line)) {
        problems.push(`register row "${id}" has neither LISTED nor EXCLUDED: <reason>`)
      }
    }
  }
  return problems
}

// ---------------------------------------------------------------------------
// Strict checks
// ---------------------------------------------------------------------------

export function strictProblems(result) {
  const problems = []
  for (const item of result.unclassified) {
    problems.push(`unclassified host ${item.host} (${item.code} code hit(s), ${item.config} config hit(s)); first at ${item.evidence[0]?.origin ?? '?'}`)
  }
  for (const id of result.vendorsNotFound) problems.push(`catalogue vendor "${id}" has no evidence in this tree (stale entry, or the code moved)`)
  for (const vendor of result.vendors) {
    for (const gate of vendor.gates) {
      if (gate.readInSource === 0 && gate.wiredIn === 0) problems.push(`vendor "${vendor.id}": curated variable ${gate.name} is not found in source, in a Rust config field, or in any compose or env file`)
    }
  }
  for (const item of result.unmappedCredentialNames) {
    problems.push(`credential-like variable ${item.name} (${item.first}) is claimed by no vendor and reviewed as nothing: map it to a vendor or add it to INTERNAL_ENV with a reason`)
  }
  return problems
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

function usage(io) {
  io.err(
    [
      'usage: node scan-egress.mjs [--root DIR] [--json | --summary] [--strict] [--verify-doc FILE] [--catalogue FILE.json] [--max-evidence N]',
      '',
    ].join('\n'),
  )
}

export function findRepoRoot(start) {
  let dir = path.resolve(start)
  for (;;) {
    if (fs.existsSync(path.join(dir, '.git'))) return dir
    const parent = path.dirname(dir)
    if (parent === dir) return null
    dir = parent
  }
}

export function main(argv, io = { out: (text) => process.stdout.write(text), err: (text) => process.stderr.write(text) }) {
  const args = argv.slice()
  let root = null
  let json = false
  let summary = false
  let strict = false
  let verifyFile = null
  let catalogueFile = null
  let maxEvidence = 4
  while (args.length > 0) {
    const arg = args.shift()
    if (arg === '--root') {
      root = args.shift()
      if (!root) { usage(io); return 2 }
    } else if (arg === '--json') json = true
    else if (arg === '--summary') summary = true
    else if (arg === '--strict') strict = true
    else if (arg === '--verify-doc') {
      verifyFile = args.shift()
      if (!verifyFile) { usage(io); return 2 }
    } else if (arg === '--catalogue') {
      catalogueFile = args.shift()
      if (!catalogueFile) { usage(io); return 2 }
    } else if (arg === '--max-evidence') {
      maxEvidence = Number(args.shift())
      if (!Number.isInteger(maxEvidence) || maxEvidence < 1) { usage(io); return 2 }
    } else if (arg === '-h' || arg === '--help') { usage(io); return 0 }
    else { usage(io); return 2 }
  }
  const resolvedRoot = path.resolve(root ?? findRepoRoot(HERE) ?? path.join(HERE, '..', '..', '..', '..'))
  let vendors = VENDORS
  if (catalogueFile) {
    try {
      vendors = JSON.parse(fs.readFileSync(catalogueFile, 'utf8'))
    } catch (error) {
      io.err(`scan-egress: cannot read the catalogue ${catalogueFile}: ${error.message}\n`)
      return 2
    }
  }
  const result = scan({ root: resolvedRoot, vendors, maxEvidence })
  if (json && summary) { usage(io); return 2 }
  io.out(json ? renderJson(result) : summary ? renderSummary(result) : renderText(result))

  let failed = false
  if (strict) {
    const problems = strictProblems(result)
    for (const problem of problems) io.err(`scan-egress: strict: ${problem}\n`)
    if (problems.length > 0) failed = true
  }
  if (verifyFile) {
    let markdown
    try {
      markdown = fs.readFileSync(verifyFile, 'utf8')
    } catch (error) {
      io.err(`scan-egress: cannot read ${verifyFile}: ${error.message}\n`)
      return 2
    }
    const problems = verifyDocument(result, markdown, { knownIds: vendors.map((vendor) => vendor.id) })
    for (const problem of problems) io.err(`scan-egress: verify-doc: ${problem}\n`)
    if (problems.length > 0) failed = true
  }
  return failed ? 1 : 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2))
}
