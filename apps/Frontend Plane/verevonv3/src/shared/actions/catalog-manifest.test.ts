import { describe, expect, it } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import {
  ACTION_CATALOG_SCHEMA_VERSION,
  actionCatalogSha256,
  buildActionCatalogManifest,
  canonicalActionCatalogJson,
} from './catalog-manifest'

describe('provisional Action Catalog manifest', () => {
  it('is deterministic, versioned, and sorted independently of source order', () => {
    const forward = buildActionCatalogManifest(actionRegistry)
    const reverse = buildActionCatalogManifest([...actionRegistry].reverse())

    expect(forward.schemaVersion).toBe(ACTION_CATALOG_SCHEMA_VERSION)
    expect(forward).toEqual(reverse)
    expect(forward.actions.map((action) => action.id)).toEqual(
      [...forward.actions.map((action) => action.id)].sort(),
    )
    expect(canonicalActionCatalogJson(actionRegistry)).toBe(canonicalActionCatalogJson([...actionRegistry].reverse()))
  })

  it('marks an action model-eligible only where the owner adapter is live', () => {
    const manifest = buildActionCatalogManifest()
    expect(manifest.actions).not.toHaveLength(0)

    // Each of these is an action whose owner issues a durable, queryable
    // record of the action, collapses duplicate submissions, and denies a
    // forged actor on both sides -- see model-eligibility.ts for the full
    // evidence trail per action. Everything else stays human-only until its
    // own owner path is proven the same way.
    const modelEligible = manifest.actions
      .filter((action) => action.allowedActorTypes.includes('model'))
      .map((action) => action.id)
    expect(modelEligible).toEqual([
      'chat.save_thread_snapshot',
      'chat.submit_feedback',
      'inbox.follow_conversation',
      'inbox.review_ai_action',
      'inbox.set_csat_preference',
      'org.acknowledge_deletion',
      'org.mark_exported',
      'tickets.create',
    ])

    // Every action stays available to a human: eligibility widens the actor
    // set, it never narrows it -- the 'anything AI can do, a human can do'
    // half of the contract.
    expect(manifest.actions.every((action) => action.allowedActorTypes.includes('human'))).toBe(true)
  })

  it('does not confuse legacy dispatch with an owner-issued operation receipt', () => {
    const manifest = buildActionCatalogManifest()
    const ticketCreate = manifest.actions.find((action) => action.id === 'tickets.create')
    const ticketUpdate = manifest.actions.find((action) => action.id === 'tickets.update')

    expect(ticketCreate).toMatchObject({
      executionContract: 'owner_operation_receipt',
      idempotency: 'caller_supplied',
    })
    expect(ticketUpdate).toMatchObject({
      executionContract: 'legacy_direct',
      idempotency: 'not_yet_contractual',
    })
  })

  it('produces a stable SHA-256 digest', async () => {
    await expect(actionCatalogSha256()).resolves.toMatch(/^[a-f0-9]{64}$/)
    await expect(actionCatalogSha256()).resolves.toBe(await actionCatalogSha256())
  })
})
