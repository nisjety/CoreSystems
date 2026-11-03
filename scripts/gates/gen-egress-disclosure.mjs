#!/usr/bin/env node
// gen-egress-disclosure.mjs -- write the egress disclosure from the declared catalogue.
//
// docs/compliance/egress-catalogue.json says, for each outbound destination, which process
// contacts it, what data class leaves, the switch that turns it off and what ZDR does. This
// script renders that as a markdown table, so the table is never edited by hand and a
// destination cannot appear in the disclosure without a catalogue entry. The scan check
// (scan-egress.mjs --strict --declared) makes sure each entry has evidence in the code.
//
// Usage
//   node scripts/gates/gen-egress-disclosure.mjs           write docs/compliance/generated/egress-disclosure.md
//   node scripts/gates/gen-egress-disclosure.mjs --check   exit 1 when that file differs from the output
//   --root DIR   the repository root (tests use it; the default is the root above this script)
//
// The disclosure is generated and is not published: the trust page does not copy it before counsel
// rules on the wording (EP-9 Q-T1, WS01-S14). Output is deterministic: rows sorted by entry id, LF
// line endings, no timestamps.
//
// Exit status: 0 clean or written, 1 the file differs or the catalogue is invalid, 2 usage or I/O error.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { VENDORS, declaredShapeProblems } from '../../docs/compliance/evidence/tools/scan-egress.mjs'

const HERE = path.dirname(fileURLToPath(import.meta.url))

export const CATALOGUE_PATH = 'docs/compliance/egress-catalogue.json'
export const DISCLOSURE_PATH = 'docs/compliance/generated/egress-disclosure.md'

const HEADER = [
  '# Egress disclosure (generated; do not edit)',
  '',
  `Generated from \`${CATALOGUE_PATH}\` by \`scripts/gates/gen-egress-disclosure.mjs\`. Edit the catalogue and run the generator; CI runs it with \`--check\` and fails when this file differs.`,
  '',
  'An engineering reading of the code and its configuration, not a contract fact and not reviewed by counsel. Region and DPA stay `TBC` until a contract or a deployment record says otherwise. Not published on the trust page before counsel rules on the wording (EP-9 Q-T1).',
  '',
  "`zdr`: `refused` (a ZDR run never reaches the destination); `no_retention_attested` (a ZDR run may reach it on the recipient's attested no-retention terms); `unchanged_no_content` (no conversation or document content leaves, so ZDR changes nothing); `not_gated` (content can leave on a ZDR run and no gate or attestation was found: an open finding).",
  '',
  'Data classes: `prompt` (conversation or generation input); `query_terms` (search or lookup text); `document` (document, mailbox or workspace content); `metadata` (identifiers, addresses, account and billing fields); `none` (no customer data leaves).',
  '',
]

const TABLE_HEAD = [
  '| Entry | Vendor | Plane | Contacting service | Hosts | Data classes | Switch | ZDR | Region |',
  '|---|---|---|---|---|---|---|---|---|',
]

const compareStrings = (a, b) => (a < b ? -1 : a > b ? 1 : 0)

/** One markdown cell: a pipe would end the cell early, so it is escaped; a line break would end the row. */
function cell(text) {
  return String(text).replace(/\|/g, '\\|').replace(/\r?\n/g, ' ')
}

function codes(items) {
  return items.map((item) => `\`${cell(item)}\``).join(', ')
}

/** The disclosure for a declared catalogue: one row per entry, sorted by entry id. */
export function renderDisclosure(declared, vendors = VENDORS) {
  const names = new Map(vendors.map((vendor) => [vendor.id, vendor.name]))
  const entries = [...declared.entries].sort((a, b) => compareStrings(a.id, b.id))
  const lines = [...HEADER, ...TABLE_HEAD]
  for (const entry of entries) {
    const row = [
      codes([entry.id]),
      cell(names.get(entry.vendor_id) ?? entry.vendor_id),
      cell(entry.plane),
      cell(entry.contacting_service),
      entry.hosts.length > 0 ? codes(entry.hosts) : 'none in code',
      codes(entry.data_classes),
      entry.toggle_env.length > 0 ? codes(entry.toggle_env) : 'none',
      codes([entry.zdr]),
      cell(entry.region),
    ]
    lines.push(`| ${row.join(' | ')} |`)
  }
  const vendorCount = new Set(entries.map((entry) => entry.vendor_id)).size
  lines.push('', `Entries: ${entries.length}. Vendors: ${vendorCount}.`, '')
  return lines.join('\n')
}

const defaultIo = {
  out: (text) => process.stdout.write(text),
  err: (text) => process.stderr.write(text),
}

function usage(io) {
  io.err('usage: node scripts/gates/gen-egress-disclosure.mjs [--check] [--root DIR]\n')
}

export function main(argv, io = defaultIo) {
  const args = argv.slice()
  let check = false
  let root = null
  while (args.length > 0) {
    const arg = args.shift()
    if (arg === '--check') check = true
    else if (arg === '--root') {
      root = args.shift()
      if (!root) {
        usage(io)
        return 2
      }
    } else if (arg === '-h' || arg === '--help') {
      usage(io)
      return 0
    } else {
      usage(io)
      return 2
    }
  }

  const base = path.resolve(root ?? path.join(HERE, '..', '..'))
  let declared
  try {
    declared = JSON.parse(fs.readFileSync(path.join(base, CATALOGUE_PATH), 'utf8'))
  } catch (error) {
    io.err(`gen-egress-disclosure: cannot read ${CATALOGUE_PATH}: ${error.message}\n`)
    return 2
  }
  // an invalid catalogue is never rendered: the disclosure would state what the catalogue got wrong
  const problems = declaredShapeProblems(declared)
  if (problems.length > 0) {
    for (const problem of problems) io.err(`gen-egress-disclosure: ${problem}\n`)
    return 1
  }

  const expected = renderDisclosure(declared)
  const target = path.join(base, DISCLOSURE_PATH)
  if (!check) {
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, expected)
    io.out(`wrote ${DISCLOSURE_PATH}\n`)
    return 0
  }

  let committed
  try {
    committed = fs.readFileSync(target, 'utf8').replace(/\r\n/g, '\n')
  } catch (error) {
    io.err(`gen-egress-disclosure: ${DISCLOSURE_PATH} cannot be read (${error.code ?? error.message}); run node scripts/gates/gen-egress-disclosure.mjs\n`)
    return 1
  }
  if (committed !== expected) {
    io.err(`gen-egress-disclosure: ${DISCLOSURE_PATH} differs from ${CATALOGUE_PATH}; run node scripts/gates/gen-egress-disclosure.mjs and commit the result\n`)
    return 1
  }
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2))
}
