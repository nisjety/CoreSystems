import { describe, expect, it } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'
import {
  ACTION_CATALOG_SCHEMA_VERSION,
  actionCatalogSha256,
  actionSchemaHash,
  buildActionCatalogManifest,
  canonicalActionCatalogJson,
} from './catalog-manifest'

describe('provisional Action Catalog manifest', () => {
  it('is deterministic, versioned, and sorted independently of source order', async () => {
    const forward = await buildActionCatalogManifest(actionRegistry)
    const reverse = await buildActionCatalogManifest([...actionRegistry].reverse())

    expect(forward.schemaVersion).toBe(ACTION_CATALOG_SCHEMA_VERSION)
    expect(forward).toEqual(reverse)
    expect(forward.actions.map((action) => action.id)).toEqual(
      [...forward.actions.map((action) => action.id)].sort(),
    )
    expect(await canonicalActionCatalogJson(actionRegistry)).toBe(
      await canonicalActionCatalogJson([...actionRegistry].reverse()),
    )
  })

  it('gives every action an access level, a closed reason only where it is closed, and the owner hash for tickets.create', async () => {
    const manifest = await buildActionCatalogManifest()

    expect(manifest.actions.filter((action) => action.modelAccess === 'none').every((action) => action.exemptReason !== null)).toBe(true)
    expect(manifest.actions.filter((action) => action.modelAccess !== 'none').every((action) => action.exemptReason === null)).toBe(true)
    expect(manifest.actions.find((action) => action.id === 'tickets.create')).toMatchObject({
      modelAccess: 'write_approval',
      schemaHash: 'sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df',
    })
  })

  it('marks an action model-eligible only where the owner adapter is live', async () => {
    const manifest = await buildActionCatalogManifest()
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

  it('does not confuse legacy dispatch with an owner-issued operation receipt', async () => {
    const manifest = await buildActionCatalogManifest()
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

  it('lists the memory controls as human-only actions', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of ['memory.set_state', 'memory.reset', 'memory.remember']) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.allowedActorTypes).toEqual(['human'])
    }
  })

  it('produces a stable SHA-256 digest', async () => {
    await expect(actionCatalogSha256()).resolves.toMatch(/^[a-f0-9]{64}$/)
    await expect(actionCatalogSha256()).resolves.toBe(await actionCatalogSha256())
  })

  it('lists the Space post and entitlement actions as human-only, closed to the model (WS01-S5, WS03-S4)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of [
      'spaces.create_post',
      'spaces.reply_in_thread',
      'spaces.set_effect_entitlements',
      'spaces.apply_effect_preset',
    ]) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.allowedActorTypes).toEqual(['human'])
      expect(entry?.modelAccess).toBe('none')
      expect(entry?.exemptReason).toBe('human_identity_or_security')
    }
  })

  it('lists the sidebar and direct message actions as human-only with a closed model access (WS16-S8, WS12-S3)', async () => {
    const manifest = await buildActionCatalogManifest()
    const expected: Record<string, string> = {
      'spaces.update_sidebar': 'ui_state_only',
      'spaces.hide_space': 'ui_state_only',
      'spaces.open_dm': 'human_identity_or_security',
    }
    for (const [id, reason] of Object.entries(expected)) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.allowedActorTypes).toEqual(['human'])
      expect(entry?.modelAccess).toBe('none')
      expect(entry?.exemptReason).toBe(reason)
    }
  })

  it('lists the document share actions as Data Plane owned and closed to the model (WS14-S4)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of ['ownership.share_document', 'ownership.revoke_document_share']) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      // documents-api owns the document and proves ownership, so the owner plane is data.
      expect(entry?.ownerPlane).toBe('data')
      // Negative assertion: the model can never share or un-share a document.
      expect(isModelExecutableAction(id), id).toBe(false)
      expect(entry?.allowedActorTypes).toEqual(['human'])
      expect(entry?.modelAccess).toBe('none')
      expect(entry?.exemptReason).not.toBeNull()
    }
  })

  it('keeps the reaction action human-only (WS12-S8): a reaction is a personal mark that no model may place', async () => {
    const manifest = await buildActionCatalogManifest()
    const entry = manifest.actions.find((action) => action.id === 'spaces.toggle_reaction')
    expect(entry).toBeDefined()
    expect(isModelExecutableAction('spaces.toggle_reaction')).toBe(false)
    expect(entry?.allowedActorTypes).toEqual(['human'])
    expect(entry?.modelAccess).toBe('none')
    expect(entry?.exemptReason).not.toBeNull()
  })

  it('does not let the owner plane move the admission schema hash (WS14-S4 re-point)', async () => {
    // Re-pointing the share actions from Control to the Data Plane changed ownerPlane only.
    // The admission contract hashes id + input + output schemas, so the vendored golden
    // (admission-contract.test.ts) must not move with it.
    for (const id of ['ownership.share_document', 'ownership.revoke_document_share']) {
      const action = actionRegistry.find((candidate) => candidate.id === id)
      expect(action, id).toBeDefined()
      if (!action) continue
      const moved = { ...action, ownerPlane: action.ownerPlane === 'data' ? 'control' : 'data' } as typeof action
      await expect(actionSchemaHash(moved)).resolves.toBe(await actionSchemaHash(action))
    }
  })

  it('lists the audit event read as human-only, and adds no audit export action (WS05-S4)', async () => {
    const manifest = await buildActionCatalogManifest()
    const entry = manifest.actions.find((action) => action.id === 'audit.list_events')
    expect(entry).toBeDefined()
    expect(entry?.allowedActorTypes).toEqual(['human'])
    expect(isModelExecutableAction('audit.list_events')).toBe(false)
    // The plan's export route and its audit.export action were not built; the compliance export is audit.export_log.
    expect(actionRegistry.map((action) => action.id)).not.toContain('audit.export')
  })

  it('lists the organization domain, SSO and SCIM actions as human-only and closed to the model (WS05-S6, WS05-S7, WS05-S8)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of [
      'org.add_domain',
      'org.verify_domain',
      'org.revoke_domain',
      'org.update_sso_policy',
      'org.add_sso_provider',
      'org.remove_sso_provider',
      'org.create_scim_token',
      'org.revoke_scim_token',
    ]) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.allowedActorTypes, id).toEqual(['human'])
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })

  it('lists the Space read-state and notify actions as application actions closed to the model (WS11-S3, WS11-S10)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of ['spaces.mark_all_read', 'spaces.mark_unread', 'spaces.set_notify_level']) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.ownerPlane, id).toBe('application')
      expect(entry?.risk, id).toBe('low')
      expect(entry?.modelAccess, id).toBe('none')
      expect(entry?.allowedActorTypes, id).toEqual(['human'])
    }
    expect(manifest.actions.find((action) => action.id === 'spaces.set_status')).toBeDefined()
  })

  it('keeps the edit and delete message actions out of the model (WS12-S7)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of ['spaces.edit_message', 'spaces.delete_message']) {
      expect(manifest.actions.find((action) => action.id === id), id).toBeDefined()
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })

  it('keeps the agent withdrawal, private panel, memory sharing and memory restore actions human-only (WS17-S5, WS17-S6, WS17-S7, WS17-S7b, W3-DEC-NOTES, W3-FIX-13)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of [
      'agents.create_personal',
      'agents.update_access',
      'agents.set_primary',
      'spaces.withdraw_agent',
      'spaces.open_private_panel',
      'memory.declassify_to_space',
      'memory.retract_from_space',
      'memory.restore',
    ]) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.allowedActorTypes, id).toEqual(['human'])
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })

  it('keeps the canvas save, the canvas restore and the browser run start out of the model (WS19-S4, WS20-S5)', async () => {
    const manifest = await buildActionCatalogManifest()
    for (const id of ['spaces.save_canvas', 'spaces.restore_canvas_version', 'spaces.browser_run.start']) {
      const entry = manifest.actions.find((action) => action.id === id)
      expect(entry, id).toBeDefined()
      expect(entry?.allowedActorTypes, id).toEqual(['human'])
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })
})
