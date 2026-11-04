// Tests for gen-egress-disclosure.mjs. Run: node --test scripts/gates/gen-egress-disclosure.test.mjs
//
// The renderer is pure. The file checks run in a throwaway repository root, so nothing here
// depends on the state of the working tree. The committed disclosure is checked by the
// generator's own --check, which the egress-catalogue job in repo-gates.yml runs.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { CATALOGUE_PATH, DISCLOSURE_PATH, main, renderDisclosure } from './gen-egress-disclosure.mjs'

const ENTRY = {
  id: 'resend-mail',
  vendor_id: 'resend',
  plane: 'Control Plane',
  contacting_service: 'auth-core | sign-in mail',
  hosts: ['api.resend.com'],
  data_classes: ['metadata'],
  toggle_env: ['RESEND_API_KEY'],
  zdr: 'unchanged_no_content',
  region: 'TBC',
  notes: 'fixture',
}

function capture() {
  const out = []
  const err = []
  return { io: { out: (text) => out.push(text), err: (text) => err.push(text) }, out, err }
}

function makeRoot(entries) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'egress-disclosure-'))
  fs.mkdirSync(path.dirname(path.join(root, CATALOGUE_PATH)), { recursive: true })
  fs.writeFileSync(path.join(root, CATALOGUE_PATH), JSON.stringify({ schema: 1, entries }))
  return root
}

test('renderDisclosure: one row per entry, sorted by entry id, whatever order the catalogue lists them in', () => {
  const later = { ...ENTRY, id: 'zz-later' }
  const forward = renderDisclosure({ schema: 1, entries: [ENTRY, later] })
  const reverse = renderDisclosure({ schema: 1, entries: [later, ENTRY] })
  assert.equal(forward, reverse)
  const rows = forward.split('\n').filter((line) => line.startsWith('| `'))
  assert.equal(rows.length, 2)
  assert.ok(rows[0].startsWith('| `resend-mail` |'))
  assert.ok(rows[1].startsWith('| `zz-later` |'))
})

test('renderDisclosure: names the vendor from the scan catalogue, escapes pipes, and says what is empty', () => {
  const text = renderDisclosure({ schema: 1, entries: [ENTRY, { ...ENTRY, id: 'no-hosts', hosts: [], toggle_env: [] }] })
  assert.match(
    text,
    /\| `resend-mail` \| Resend \(sign-in and verification email\) \| Control Plane \| auth-core \\\| sign-in mail \| `api\.resend\.com` \| `metadata` \| `RESEND_API_KEY` \| `unchanged_no_content` \| TBC \|/,
  )
  assert.match(text, /\| `no-hosts` \| .* \| none in code \| `metadata` \| none \| `unchanged_no_content` \| TBC \|/)
  assert.match(text, /Entries: 2\. Vendors: 1\./)
  assert.ok(text.endsWith('\n'), 'the file ends with one newline')
  assert.ok(!text.includes('\r'), 'LF line endings only')
})

test('main: writes the disclosure; --check passes on the written file and fails once it is edited or missing', () => {
  const root = makeRoot([ENTRY])
  assert.equal(main(['--check', '--root', root], capture().io), 1, 'a missing file fails --check')
  const written = capture()
  assert.equal(main(['--root', root], written.io), 0)
  assert.match(written.out.join(''), /wrote docs\/compliance\/generated\/egress-disclosure\.md/)
  assert.equal(main(['--check', '--root', root], capture().io), 0)
  fs.appendFileSync(path.join(root, DISCLOSURE_PATH), 'hand edit\n')
  const differs = capture()
  assert.equal(main(['--check', '--root', root], differs.io), 1)
  assert.match(differs.err.join(''), /differs from docs\/compliance\/egress-catalogue\.json/)
})

test('main: an invalid catalogue fails and writes nothing; bad arguments exit 2', () => {
  const root = makeRoot([{ ...ENTRY, vendor_id: 'nobody' }])
  const failed = capture()
  assert.equal(main(['--root', root], failed.io), 1)
  assert.match(failed.err.join(''), /vendor_id "nobody" is not a catalogue vendor/)
  assert.equal(fs.existsSync(path.join(root, DISCLOSURE_PATH)), false, 'nothing is written from an invalid catalogue')
  assert.equal(main(['--bogus'], capture().io), 2)
  assert.equal(main(['--root'], capture().io), 2)
  assert.equal(main(['--root', path.join(root, 'nowhere')], capture().io), 2, 'an unreadable catalogue is an I/O error')
})
