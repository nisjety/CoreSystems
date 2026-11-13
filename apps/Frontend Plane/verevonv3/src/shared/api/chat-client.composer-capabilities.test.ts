import { afterEach, describe, expect, it, vi } from 'vitest'
import { NO_COMPOSER_CAPABILITIES, getComposerCapabilities } from './chat-client'

// W3-EP-2-S7-fe: the composer capabilities are read only as the literal true the gateway answers. Deep research
// over the organisation's knowledge (deep_research_knowledge) is offered only on an explicit true; any failure,
// an older gateway, or a non-boolean reads as nothing offered.

function reply(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('getComposerCapabilities', () => {
  it('reads every field the gateway answers true, under the data envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      reply(200, {
        data: {
          artifact_history: true,
          artifact_editing: true,
          context_refs: true,
          thread_references: true,
          selection_edit: true,
          artifact_promote: true,
          deep_research_knowledge: true,
          wide_research: true,
        },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)
    await expect(getComposerCapabilities()).resolves.toEqual({
      artifactHistory: true,
      artifactEditing: true,
      contextRefs: true,
      threadReferences: true,
      selectionEdit: true,
      artifactPromote: true,
      deepResearchKnowledge: true,
      wideResearch: true,
    })
    expect(String(fetchMock.mock.calls[0]![0])).toMatch(/\/api\/v1\/chat\/composer-capabilities$/)
  })

  it('reads deep_research_knowledge on its own, leaving the others off', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(reply(200, { deep_research_knowledge: true })))
    await expect(getComposerCapabilities()).resolves.toEqual({
      ...NO_COMPOSER_CAPABILITIES,
      deepResearchKnowledge: true,
    })
  })

  it.each([
    ['false', false],
    ['the string "true"', 'true'],
    ['1', 1],
    ['null', null],
  ])('does not offer deep research over knowledge for %s', async (_label, value) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(reply(200, { deep_research_knowledge: value })))
    expect((await getComposerCapabilities()).deepResearchKnowledge).toBe(false)
  })

  it('reads an answer without the field (an older gateway) as not offered', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(reply(200, { context_refs: true })))
    const capabilities = await getComposerCapabilities()
    expect(capabilities.deepResearchKnowledge).toBe(false)
    expect(capabilities.contextRefs).toBe(true)
  })

  it.each([
    ['a 404 (no such route)', () => reply(404, { error: { code: 'not_found', message: 'no' } })],
    ['a 500', () => reply(500, { error: { code: 'internal', message: 'boom' } })],
    ['an array body', () => reply(200, [{ deep_research_knowledge: true }])],
  ])('offers nothing on %s', async (_label, make) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(make()))
    await expect(getComposerCapabilities()).resolves.toEqual(NO_COMPOSER_CAPABILITIES)
  })

  it('offers nothing when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('network')))
    await expect(getComposerCapabilities()).resolves.toEqual(NO_COMPOSER_CAPABILITIES)
  })
})
