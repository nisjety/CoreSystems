/**
 * The right panel opens for work, not for bookkeeping.
 *
 * VEREVON_CHAT_DESIGN.md section 3.1: "the right panel is never opened by the
 * product, only by the work." Every lifecycle event (connect, compose, model
 * selected, grounding summary, reasoning, usage, memory recall, verification)
 * also becomes a task step, so an auto-open rule keyed on "any step exists"
 * fired on a bare factual answer -- the live audit of 2026-09-02 watched the
 * Work panel open for "What is the capital of Norway?" (plan item 18).
 *
 * `available` (may the tab be offered) and `claimsFocus` (may it interrupt) are
 * now two predicates, and `isWorkStep` classifies steps by the id conventions
 * their producers already use.
 */

import { describe, expect, it } from 'vitest'
import {
  chatSurfaceClaimsFocus,
  isChatSurfaceAvailable,
  type ChatSurfaceAvailability,
} from './chat-surfaces'
import { isWorkStep, normalizeStep } from '../components/chat-normalizers'

const calm: ChatSurfaceAvailability = {
  sourceCount: 0,
  hasGrounding: false,
  artifactCount: 0,
  attachmentCount: 0,
  stepCount: 0,
  hasRun: false,
  workStepCount: 0,
  toolCallCount: 0,
}

describe('isWorkStep', () => {
  const turn = 'turn_01'
  const lifecycle = ['connect', 'answer', 'model', 'grounding', 'reasoning', 'usage', 'memory', 'verification']

  it('treats every lifecycle step as bookkeeping', () => {
    for (const kind of lifecycle) {
      expect(isWorkStep({ id: `${turn}:${kind}` }), kind).toBe(false)
    }
  })

  it("treats the composer's requested-capability placeholders as bookkeeping", () => {
    // These share the `:tool-` prefix with real calls but carry a composer tool id.
    for (const tool of ['search', 'research', 'image', 'reason']) {
      expect(isWorkStep({ id: `${turn}:tool-${tool}` }), tool).toBe(false)
    }
  })

  it('treats orchestration steps, provider actions and real tool calls as work', () => {
    expect(isWorkStep({ id: `${turn}:event-deep-research-subquery-2` })).toBe(true)
    expect(isWorkStep({ id: `${turn}:event-plan-step-7` })).toBe(true)
    expect(isWorkStep({ id: `${turn}:action-send_email` })).toBe(true)
    expect(isWorkStep({ id: `${turn}:tool-call_01HZX9` })).toBe(true)
  })

  it('fails calm on shapes it does not recognise', () => {
    expect(isWorkStep({ id: 'no-marker-at-all' })).toBe(false)
    expect(isWorkStep({ id: `${turn}:something-new` })).toBe(false)
  })

  it('keeps source review visible before any artifact or tool is published', () => {
    const review = normalizeStep({ id: 'call-1:source-check', title: 'Kontrollerer utkastet mot kildene', status: 'running' }, turn)!
    expect(isWorkStep(review)).toBe(true)
    const state = { ...calm, stepCount: 1, workStepCount: [review].filter(isWorkStep).length }
    expect(isChatSurfaceAvailable('steps', state)).toBe(true)
    expect(chatSurfaceClaimsFocus('steps', state)).toBe(true)
    expect(isWorkStep({ id: `${turn}:source-check` })).toBe(false)
  })
})

describe('claimsFocus versus available', () => {
  it('does not even offer Work for a bookkeeping-only turn', () => {
    // A plain answer produces about six lifecycle steps and nothing else. The
    // tab used to be offered on that count alone: measured 2026-09-04, a
    // one-question thread advertised "Arbeid 4" over connect/compose/model/
    // usage rows (audit item 27). UX spec section 4: only evidence-backed
    // destinations appear.
    const state = { ...calm, stepCount: 6 }
    expect(isChatSurfaceAvailable('steps', state)).toBe(false)
    expect(chatSurfaceClaimsFocus('steps', state)).toBe(false)
  })

  it('offers Work once there is work, or a durable run to inspect', () => {
    expect(isChatSurfaceAvailable('steps', { ...calm, stepCount: 7, toolCallCount: 1 })).toBe(true)
    expect(isChatSurfaceAvailable('steps', { ...calm, stepCount: 7, workStepCount: 1 })).toBe(true)
    expect(isChatSurfaceAvailable('steps', { ...calm, hasRun: true })).toBe(true)
  })

  it('opens Work on the first real tool call, work step, or durable run', () => {
    expect(chatSurfaceClaimsFocus('steps', { ...calm, stepCount: 7, toolCallCount: 1 })).toBe(true)
    expect(chatSurfaceClaimsFocus('steps', { ...calm, stepCount: 7, workStepCount: 1 })).toBe(true)
    expect(chatSurfaceClaimsFocus('steps', { ...calm, hasRun: true })).toBe(true)
  })

  it('opens Output for an artifact, never for the user\'s own attachments', () => {
    expect(isChatSurfaceAvailable('artifacts', { ...calm, attachmentCount: 2 })).toBe(true)
    expect(chatSurfaceClaimsFocus('artifacts', { ...calm, attachmentCount: 2 })).toBe(false)
    expect(chatSurfaceClaimsFocus('artifacts', { ...calm, artifactCount: 1 })).toBe(true)
  })

  it('opens Sources for a citation that was read, not for a bare grounding summary', () => {
    expect(isChatSurfaceAvailable('sources', { ...calm, hasGrounding: true })).toBe(true)
    expect(chatSurfaceClaimsFocus('sources', { ...calm, hasGrounding: true })).toBe(false)
    expect(
      chatSurfaceClaimsFocus('sources', { ...calm, sourceCount: 1, readSourceCount: 1 }),
    ).toBe(true)
  })

  it('offers Sources for a lead it never read, but does not open the panel for one', () => {
    // Deep research can list two dozen candidates and read three. A lead is
    // worth listing; it is not worth pulling the user out of the answer for,
    // and the panel popping open on one reads as "here is your evidence" when
    // nothing was fetched.
    const leadsOnly = { ...calm, sourceCount: 24, readSourceCount: 0 }
    expect(isChatSurfaceAvailable('sources', leadsOnly)).toBe(true)
    expect(chatSurfaceClaimsFocus('sources', leadsOnly)).toBe(false)
  })

  it('treats an omitted readSourceCount as nothing read rather than guessing', () => {
    // Same rule as workStepCount: a caller that cannot compute the narrower
    // number gets the conservative answer, never a fallback to the wider one.
    expect(chatSurfaceClaimsFocus('sources', { ...calm, sourceCount: 5 })).toBe(false)
  })

  it('never lets Trace or Chat claim focus', () => {
    expect(chatSurfaceClaimsFocus('trace', { ...calm, hasRun: true })).toBe(false)
    expect(chatSurfaceClaimsFocus('chat', calm)).toBe(false)
  })

  it('tolerates callers that do not supply the optional counts', () => {
    const legacy: ChatSurfaceAvailability = { sourceCount: 0, hasGrounding: false, artifactCount: 0, attachmentCount: 0, stepCount: 6, hasRun: false }
    expect(chatSurfaceClaimsFocus('steps', legacy)).toBe(false)
  })
})

describe('ChatPage wiring', () => {
  it('gates auto-open on claimsFocus, not on availability', async () => {
    const { readFileSync } = await import('node:fs')
    const { resolve } = await import('node:path')
    const page = readFileSync(resolve(process.cwd(), 'src/features/chat/components/ChatPage.tsx'), 'utf8')
    expect(page).toContain('!autoOpenedSurfaces.has(surface) && chatSurfaceClaimsFocus(surface, availability)')
    expect(page).not.toContain('!autoOpenedSurfaces.has(surface) && isChatSurfaceAvailable(surface, availability)')
    // One derivation feeds the stale-tab guard, the auto-open rule, the header
    // dropdown and the canvas tab strip (audit item 27): four inline copies of
    // `filter(isWorkStep)` is how they drift apart.
    expect(page).toContain('const workStepCount = createMemo(() => state.taskSteps.filter(isWorkStep).length)')
    expect(page).toContain('workStepCount: workStepCount()')
    expect(page).toContain('workStepCount={workStepCount()}')
  })
})
