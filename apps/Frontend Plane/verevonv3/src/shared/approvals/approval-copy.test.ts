import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  APPROVAL_CAUSE_CODES,
  approvalFallbackLine,
  approvalHeadline,
  approvalKindLabel,
  approvalReason,
  browserApprovalStatusLabel,
  browserRiskLabel,
  causeCodeOf,
  type ApprovalCause,
  type ApprovalCopyI18n,
  type BrowserApprovalStatus,
} from './approval-copy'

const no: ApprovalCopyI18n = { tr: (norwegian) => norwegian }
const en: ApprovalCopyI18n = { tr: (_norwegian, english) => english }

function cause(code: ApprovalCause['code'], extra: Partial<ApprovalCause> = {}): ApprovalCause {
  return { code, scopeOptions: ['once'], stepUpRequired: false, ...extra }
}

// The vocabulary is the server's: the contract file lists every code the gateway can state, and
// this test reads it, so a code added there fails here until it has words in both languages.
const here = dirname(fileURLToPath(import.meta.url))
const contractPath = resolve(here, '../../../../../Model Plane/api/contracts/approval-causes.json')
const contract = JSON.parse(readFileSync(contractPath, 'utf8')) as {
  unknown_degrades_to: string
  codes: { code: string }[]
}

// The kinds are the proto's: a kind added to `ApprovalKind` without words would headline every card
// of that kind with the generic line, so the enum is read the way the contract is.
const protoPath = resolve(here, '../../../../../Model Plane/proto/model_plane/v1/orchestration.proto')
const kindEnum = readFileSync(protoPath, 'utf8').split('enum ApprovalKind {')[1]?.split('}')[0] ?? ''
const protoKinds = [...kindEnum.matchAll(/(APPROVAL_KIND_[A-Z_]+)\s*=\s*\d+;/g)].map((match) => match[1])

describe('the cause vocabulary', () => {
  it('is exactly the set the Model Plane contract lists, in its order', () => {
    expect([...APPROVAL_CAUSE_CODES]).toEqual(contract.codes.map((entry) => entry.code))
  })

  it('reads a code the server states as itself and anything else as unclassified_gate', () => {
    for (const entry of contract.codes) expect(causeCodeOf(entry.code)).toBe(entry.code)
    expect(contract.unknown_degrades_to).toBe('unclassified_gate')
    for (const raw of ['a_code_from_a_newer_server', '', ' provider_write', 'PROVIDER_WRITE', undefined, null, 4]) {
      expect(causeCodeOf(raw)).toBe('unclassified_gate')
    }
  })
})

describe('the copy for every cause code', () => {
  it('words each code in Norwegian and in English, both different and never the wire token', () => {
    for (const code of APPROVAL_CAUSE_CODES) {
      const approval = { cause: cause(code) }
      for (const read of [approvalHeadline, approvalReason] as const) {
        const norwegian = read(approval, no)
        const english = read(approval, en)
        expect(norwegian, `${code} nb`).toBeTruthy()
        expect(english, `${code} en`).toBeTruthy()
        expect(norwegian, `${code} nb differs from en`).not.toBe(english)
        for (const text of [norwegian, english]) {
          expect(text, code).not.toMatch(/APPROVAL_KIND|_/)
        }
      }
    }
  })

  it('gives every code its own headline', () => {
    const headlines = APPROVAL_CAUSE_CODES.filter((code) => code !== 'browser_action_risk').map((code) =>
      approvalHeadline({ cause: cause(code) }, en),
    )
    expect(new Set(headlines).size).toBe(headlines.length)
  })

  it('names the browser risk category in the headline of a browser gate', () => {
    const gated = { cause: cause('browser_action_risk', { riskCategory: 'checkout' }) }
    expect(approvalHeadline(gated, en)).toBe('Checkout or payment')
    expect(approvalHeadline(gated, no)).toBe('Utsjekk/betaling')
    // No category stated: a generic browser line, not the empty label.
    expect(approvalHeadline({ cause: cause('browser_action_risk') }, en)).toBe('A browser action')
  })
})

describe('an approval with no cause', () => {
  it('says that something waits and never shows its kind', () => {
    const approval = { detail: undefined, cause: undefined }
    expect(approvalHeadline(approval, en)).toBe('The agent is waiting for approval before the next step')
    expect(approvalHeadline(approval, no)).toBe('Agenten venter på godkjenning før neste steg')
    expect(approvalReason(approval, en)).toBeUndefined()
    expect(approvalFallbackLine(en)).toBe('The agent is waiting for approval before it continues.')
    expect(approvalFallbackLine(no)).toBe('Agenten venter på godkjenning før den fortsetter.')
  })

  it('still reads a detail the server wrote, trimmed, and prefers the cause when both exist', () => {
    expect(approvalReason({ detail: '  Delete 3 files  ' }, en)).toBe('Delete 3 files')
    expect(approvalReason({ detail: '   ' }, en)).toBeUndefined()
    expect(approvalReason({ detail: 'Delete 3 files', cause: cause('provider_write') }, en)).toBe(
      'The agent wants to change something in a connected service. It may be hard to undo.',
    )
  })
})

describe('the browser gate labels', () => {
  it('labels every risk category the planner and the backstop classifier state, in both languages', () => {
    const categories = [
      'checkout',
      'cross_domain_navigation',
      'destructive',
      'login',
      'persistent_cookie_use',
      'posting_form',
    ]
    const generic = browserRiskLabel('something_new', en)
    const labels = new Set<string>()
    for (const category of categories) {
      const english = browserRiskLabel(category, en)
      const norwegian = browserRiskLabel(category, no)
      expect(english).not.toBe(generic)
      expect(english).not.toBe(norwegian)
      labels.add(english)
    }
    expect(labels.size).toBe(categories.length)
  })

  it('falls back to a generic label for a category it has no words for, even an inherited name', () => {
    expect(browserRiskLabel('something_new', en)).toBe('A risky action')
    expect(browserRiskLabel('something_new', no)).toBe('Risikofylt handling')
    for (const odd of [undefined, '', 'constructor', 'toString', '__proto__']) {
      expect(browserRiskLabel(odd, en)).toBe('A risky action')
    }
  })

  it('labels every status of a browser approval in both languages', () => {
    const statuses: BrowserApprovalStatus[] = ['pending', 'granted', 'denied', 'timed_out']
    const english = statuses.map((status) => browserApprovalStatusLabel(status, en))
    expect(english).toEqual(['Waiting for approval', 'Approved', 'Denied', 'Timed out'])
    expect(statuses.map((status) => browserApprovalStatusLabel(status, no))).toEqual([
      'Venter på godkjenning',
      'Godkjent',
      'Avslått',
      'Tidsavbrutt',
    ])
  })
})

describe('the approval kind a proof or an event states', () => {
  it('words each kind in the proto, whatever casing or prefix the producer used', () => {
    for (const kind of ['APPROVAL_KIND_TOOL_CALL', 'tool_call', 'Tool-Call', 'tool']) {
      expect(approvalKindLabel(kind, en), kind).toBe('Tool call')
      expect(approvalKindLabel(kind, no), kind).toBe('Verktøykall')
    }
    expect(approvalKindLabel('APPROVAL_KIND_PLAN', en)).toBe('Plan')
    expect(approvalKindLabel('APPROVAL_KIND_PERMISSION', en)).toBe('Permission')
    expect(approvalKindLabel('APPROVAL_KIND_DESTRUCTIVE', en)).toBe('A destructive action')
    expect(approvalKindLabel('APPROVAL_KIND_COST', en)).toBe('Cost')
  })

  it('never shows a token it has no words for', () => {
    for (const kind of [undefined, '', 'APPROVAL_KIND_UNSPECIFIED', 'APPROVAL_KIND_FROM_A_NEWER_SERVER', 'constructor']) {
      expect(approvalKindLabel(kind, en)).toBe('Action')
      expect(approvalKindLabel(kind, no)).toBe('Handling')
    }
  })

  it('words every kind the proto declares, so no card falls back to the generic line', () => {
    expect(protoKinds).toContain('APPROVAL_KIND_TOOL_CALL')
    for (const kind of protoKinds.filter((name) => name !== 'APPROVAL_KIND_UNSPECIFIED')) {
      expect(approvalKindLabel(kind, en), kind).not.toBe('Action')
      expect(approvalKindLabel(kind, no), kind).not.toBe('Handling')
    }
  })
})
