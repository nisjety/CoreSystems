import { describe, expect, it } from 'vitest'
import { actionRegistry } from './action-registry'
import { isModelExecutableAction } from './model-eligibility'

/**
 * Characterisation of the eight model-eligible action ids (WS24-S0). Eligibility
 * is an execution claim, so the test pins the exact set: adding an id is a
 * deliberate edit here, with the evidence in model-eligibility.ts.
 *
 * The second group records which ids have a Model Plane executor. Checked by
 * source search on 2026-10-24: only `tickets.create` is named by a Model Plane
 * service (execution-core and model-gateway). The other seven are listed by the
 * Frontend allowlist with no executor behind them. When an executor lands for
 * one of them, move it between the groups in the same change.
 */
const WITH_MODEL_PLANE_EXECUTOR = ['tickets.create']

const WITHOUT_MODEL_PLANE_EXECUTOR = [
  'chat.save_thread_snapshot',
  'chat.submit_feedback',
  'inbox.follow_conversation',
  'inbox.review_ai_action',
  'inbox.set_csat_preference',
  'org.acknowledge_deletion',
  'org.mark_exported',
]

describe('model eligibility characterisation', () => {
  it('admits exactly the eight pinned action ids, all of them registered', () => {
    const registered = actionRegistry.map((action) => action.id)
    const eligible = registered.filter((id) => isModelExecutableAction(id)).sort()

    expect(eligible).toEqual(
      [...WITH_MODEL_PLANE_EXECUTOR, ...WITHOUT_MODEL_PLANE_EXECUTOR].sort(),
    )
    expect(eligible).toHaveLength(8)
  })

  it('records one eligible id with a Model Plane executor and seven without', () => {
    expect(WITH_MODEL_PLANE_EXECUTOR.every((id) => isModelExecutableAction(id))).toBe(true)
    expect(WITHOUT_MODEL_PLANE_EXECUTOR.every((id) => isModelExecutableAction(id))).toBe(true)
    expect(WITH_MODEL_PLANE_EXECUTOR).toHaveLength(1)
    expect(WITHOUT_MODEL_PLANE_EXECUTOR).toHaveLength(7)
  })

  it('does not admit a read-shaped action merely because it is registered', () => {
    expect(isModelExecutableAction('brreg.lookup_organization')).toBe(false)
    expect(isModelExecutableAction('shipping.get_quotes')).toBe(false)
  })

  it('never admits the sidebar layout or the direct message actions to the model (WS16-S8, WS12-S3)', () => {
    for (const id of ['spaces.update_sidebar', 'spaces.open_dm', 'spaces.hide_space']) {
      expect(actionRegistry.map((action) => action.id), id).toContain(id)
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })

  it('never admits the Space post or entitlement actions to the model (WS01-S5, WS03-S4)', () => {
    for (const id of [
      'spaces.create_post',
      'spaces.reply_in_thread',
      'spaces.set_effect_entitlements',
      'spaces.apply_effect_preset',
    ]) {
      expect(actionRegistry.map((action) => action.id), id).toContain(id)
      expect(isModelExecutableAction(id), id).toBe(false)
    }
  })
})
