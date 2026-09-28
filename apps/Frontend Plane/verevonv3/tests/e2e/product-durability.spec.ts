import { createHash } from 'node:crypto'
import { expect, test } from '@playwright/test'
import { composer, openFresh, transcript, latestArtifacts } from './product-acceptance-helpers'
import { qualifyingThreads, QUALIFYING_STREAK_LENGTH } from './product-qualification-threads'
import { writeGateEvidence } from './gate-evidence'

/** The four product journeys the Q16 qualification series runs (RUN-LOG.md, PRODUCT_RECORDING_READINESS_2026-09-24.md §7). */
const EXPECTED_SCENARIO_COUNT = 4

/**
 * Durability gate (see evidence/PRODUCT_RECORDING_READINESS_2026-09-24.md
 * §7 and RECORDING-WORKFLOW.md step 3): reopening one of the four
 * qualification threads in a FRESH browser context must show the same
 * artifacts/versions/receipts/turns the server actually has — with no
 * duplicate messages or effects, and no console/page errors.
 *
 * Every test here only reopens an existing thread: it never submits a
 * message, so it makes zero model calls. Playwright already gives each
 * `test()` its own isolated browser context (no shared storage between
 * tests), so simply not seeding a transcript before navigating is a real
 * fresh reopen.
 *
 * Restart bracketing is the COORDINATOR's job, not this spec's: run this
 * file once before a controlled container restart and once after, each time
 * into a distinct `GATE_EVIDENCE_DIR`, and diff the two evidence sets — a
 * byte-identical artifact/version/receipt/turn set on both sides is the
 * durability pass.
 */
const threads = qualifyingThreads()

test.describe('Durability: qualifying threads reopen intact in a fresh context', () => {
  test.skip(threads.length === 0, 'No qualifying capture threads found under Q16_CAPTURE_ROOT (or the default private folder); nothing to reopen.')

  // A partial qualification streak is itself a gate failure, not an absence
  // of work to check — this must FAIL, never silently skip or pass on
  // however many threads happen to exist, if a scenario is missing entirely
  // or its streak is shorter than the manifest gate requires.
  test('the qualification data itself is complete: every scenario has a full qualifying streak', () => {
    const countByScenario = new Map<string, number>()
    for (const thread of threads) countByScenario.set(thread.scenario, (countByScenario.get(thread.scenario) ?? 0) + 1)
    expect(countByScenario.size, `expected ${EXPECTED_SCENARIO_COUNT} scenarios with a qualifying streak, found: ${[...countByScenario.keys()].sort().join(', ') || '(none)'}`).toBe(EXPECTED_SCENARIO_COUNT)
    for (const [scenario, count] of countByScenario) {
      expect(count, `${scenario} must contribute exactly ${QUALIFYING_STREAK_LENGTH} qualifying threads (a full streak), got ${count}`).toBe(QUALIFYING_STREAK_LENGTH)
    }
  })

  for (const thread of threads) {
    test(`${thread.scenario} / ${thread.threadId} reopens with matching artifacts, receipt and turns`, async ({ page }, info) => {
      const errors: string[] = []
      page.on('pageerror', (error) => errors.push(error.message))
      page.on('console', (entry) => { if (entry.type() === 'error') errors.push(entry.text()) })

      await openFresh(page, `/chat?thread_id=${encodeURIComponent(thread.threadId)}`)
      await expect.poll(async () => (await transcript(page))?.threadId, {
        timeout: 20_000,
        message: 'deep-linked thread must hydrate into the local transcript',
      }).toBe(thread.threadId)

      const local = await transcript(page)
      const localArtifacts = latestArtifacts(local)

      const persisted = await page.request.get(`/api/v1/chat/threads/${encodeURIComponent(thread.threadId)}/messages`)
      expect(persisted.ok(), 'durable thread must still be readable from the server').toBe(true)
      const payload = await persisted.json()
      const messages = (payload.data ?? payload).messages as Array<{
        role: string
        artifacts?: Array<{ id: string; version: number; title: string; content: string }>
        resultReceipt?: { artifactId?: string; version?: number; scope?: string }
      }>

      // Artifacts: the same id/version/title/content set the client shows,
      // taking each artifact's highest version across the server's messages.
      const serverArtifacts = new Map<string, { id: string; version: number; title: string; content: string }>()
      for (const message of messages) for (const artifact of message.artifacts ?? []) {
        if (artifact.version >= (serverArtifacts.get(artifact.id)?.version ?? 0)) serverArtifacts.set(artifact.id, artifact)
      }
      expect(localArtifacts.length, 'artifact count must match the server after a fresh reopen').toBe(serverArtifacts.size)
      for (const artifact of localArtifacts) {
        const server = serverArtifacts.get(artifact.id)
        expect(server, `artifact ${artifact.id} shown locally but missing from the server`).toBeTruthy()
        expect(artifact, `artifact ${artifact.id} must match the server exactly`).toMatchObject({
          id: server!.id, version: server!.version, title: server!.title, content: server!.content,
        })
      }

      // Receipt: the last assistant message's own receipt must point at an
      // artifact/version that is actually the one the server (and hence the
      // reopened client) shows.
      const lastAssistant = messages.filter((m) => m.role === 'assistant').at(-1)
      const receipt = lastAssistant?.resultReceipt
      expect(receipt, 'a qualifying (accepted) thread must have a result receipt on its final answer').toBeTruthy()
      if (receipt?.artifactId) {
        const shown = serverArtifacts.get(receipt.artifactId)
        expect(shown, 'receipt must reference an artifact actually present in the thread').toBeTruthy()
        expect(receipt.version, 'receipt version must match the artifact it claims to describe').toBe(shown!.version)
      }

      // Turns: roles must line up 1:1 with the server (no duplicate or
      // dropped messages introduced by the reopen).
      expect(local?.turns.map((t) => t.role), 'turn roles after reopen must match the server 1:1').toEqual(messages.map((m) => m.role))

      expect(page.locator('vite-error-overlay')).toHaveCount(0)
      expect(errors, 'reopening a durable thread must raise no console/page errors').toEqual([])

      const summary = {
        scenario: thread.scenario,
        threadId: thread.threadId,
        capturedBuild: thread.build,
        checkedAt: new Date().toISOString(),
        artifacts: [...serverArtifacts.values()]
          .sort((a, b) => a.id.localeCompare(b.id))
          .map((a) => ({ id: a.id, version: a.version, title: a.title, contentHash: createHash('sha256').update(a.content).digest('hex') })),
        receipt: receipt ?? null,
        turnCount: local?.turns.length ?? 0,
        turnRoles: local?.turns.map((t) => t.role) ?? [],
      }
      const evidencePath = writeGateEvidence('GATE_EVIDENCE_DIR', `durability-${thread.scenario}-${thread.threadId}`, summary)
      await info.attach('durability-summary', { body: JSON.stringify(summary, null, 2), contentType: 'application/json' })
      await info.attach('durability-evidence-path', { body: evidencePath, contentType: 'text/plain' })
      await expect(composer(page)).toBeVisible()
    })
  }
})
