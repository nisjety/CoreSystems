import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import { ACTION_CATALOG_SCHEMA_VERSION, actionSchemaHash } from '@/shared/actions/catalog-manifest'
import {
  MODEL_ACCESS,
  MODEL_READ_SURFACE,
  type ModelAccess,
  type ModelAuthority,
  type ModelExecutor,
  type ModelExemptReason,
  type ModelSideEffectClass,
} from '@/shared/actions/model-access'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'

/**
 * The agent's admission contract (WS24-S5), generated from the typed side map and the
 * registry. Every plane that admits an agent action reads a byte-identical copy of this
 * file (apps/Model Plane/api/contracts/README.md), so a row changed here changes all of
 * them, or a plane's test fails.
 *
 * Regenerate after a deliberate change to model-access.ts:
 *   VEREVON_UPDATE_GOLDEN=1 pnpm vitest run src/shared/actions/admission-contract.test.ts
 * then refresh the .sha256 pin and re-vendor the copies named in zdr-contract-sync.test.sh.
 */
const here = dirname(fileURLToPath(import.meta.url))
const contractPath = resolve(here, '../api/contracts/agent-action-admission.json')

// Owner-pinned by Control Plane user-core, Capability Core and execution-core. Must match.
const TICKETS_CREATE_OWNER_HASH = 'sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df'

// The eight ids the frontend allowlist admits. Each one has an owner path proven in model-eligibility.ts.
const ELIGIBLE_IDS = [
  'chat.save_thread_snapshot',
  'chat.submit_feedback',
  'inbox.follow_conversation',
  'inbox.review_ai_action',
  'inbox.set_csat_preference',
  'org.acknowledge_deletion',
  'org.mark_exported',
  'tickets.create',
]

export type AdmissionRow = {
  actionId: string
  registry: boolean
  access: ModelAccess
  authority: ModelAuthority
  sideEffectClass: ModelSideEffectClass
  exemptReason: ModelExemptReason | null
  eligible: boolean
  executor: ModelExecutor
  hashBasis: 'catalog_entry' | 'owner_contract' | null
  schemaHash: string | null
  zdr_allowed: false
}

export async function buildAgentActionAdmissionContract() {
  const registryRows: AdmissionRow[] = await Promise.all(
    actionRegistry.map(async (action) => {
      const entry = MODEL_ACCESS[action.id]
      return {
        actionId: action.id,
        registry: true,
        access: entry.access,
        authority: entry.authority,
        sideEffectClass: entry.sideEffectClass,
        exemptReason: entry.access === 'none' ? entry.exemptReason : null,
        eligible: entry.eligible,
        executor: entry.executor,
        hashBasis: entry.hashBasis,
        schemaHash: await actionSchemaHash(action),
        zdr_allowed: false as const,
      }
    }),
  )
  const surfaceRows: AdmissionRow[] = Object.entries(MODEL_READ_SURFACE).map(([actionId, entry]) => ({
    actionId,
    registry: false,
    access: entry.access,
    authority: entry.authority,
    sideEffectClass: entry.sideEffectClass,
    exemptReason: null,
    eligible: false,
    executor: entry.executor,
    hashBasis: null,
    schemaHash: null,
    zdr_allowed: false as const,
  }))
  const rows = [...registryRows, ...surfaceRows].sort((left, right) =>
    left.actionId < right.actionId ? -1 : left.actionId > right.actionId ? 1 : 0,
  )
  return {
    contract: 'agent-action-admission',
    schemaVersion: 1,
    catalogSchemaVersion: ACTION_CATALOG_SCHEMA_VERSION,
    rows,
  }
}

describe('agent action admission contract', () => {
  it('is the file every plane vendors, and changes only on an explicit update', async () => {
    const generated = `${JSON.stringify(await buildAgentActionAdmissionContract(), null, 2)}\n`
    if (process.env.VEREVON_UPDATE_GOLDEN === '1') writeFileSync(contractPath, generated)
    expect(readFileSync(contractPath, 'utf8')).toBe(generated)
  })

  it('admits exactly the eight eligible ids, and tickets.create is the only write', async () => {
    const contract = await buildAgentActionAdmissionContract()
    const eligible = contract.rows.filter((row) => row.eligible).map((row) => row.actionId)
    const registryEligible = actionRegistry.map((action) => action.id).filter((id) => isModelExecutableAction(id))

    expect([...eligible].sort()).toEqual([...ELIGIBLE_IDS].sort())
    expect([...registryEligible].sort()).toEqual([...ELIGIBLE_IDS].sort())
    expect(contract.rows.filter((row) => row.access === 'write_approval').map((row) => row.actionId)).toEqual([
      'tickets.create',
    ])
  })

  it('gives every closed row a reason, and closes every row to ZDR', async () => {
    const contract = await buildAgentActionAdmissionContract()
    for (const row of contract.rows) {
      expect(row.zdr_allowed, row.actionId).toBe(false)
      if (row.access === 'none') expect(row.exemptReason, row.actionId).not.toBeNull()
      else expect(row.exemptReason, row.actionId).toBeNull()
    }
  })

  it('pins tickets.create to the owner contract digest', async () => {
    const contract = await buildAgentActionAdmissionContract()
    expect(contract.rows.find((row) => row.actionId === 'tickets.create')).toMatchObject({
      access: 'write_approval',
      hashBasis: 'owner_contract',
      schemaHash: TICKETS_CREATE_OWNER_HASH,
    })
  })

  it('keeps the seven unexecutable eligible ids visible, with no executor behind them', async () => {
    const contract = await buildAgentActionAdmissionContract()
    const debt = contract.rows.filter((row) => row.eligible && row.access === 'none')

    expect(debt.map((row) => row.actionId)).toHaveLength(7)
    expect(debt.every((row) => Object.keys(row.executor).length === 0)).toBe(true)
  })
})
