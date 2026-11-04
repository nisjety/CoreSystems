// @vitest-environment jsdom

import { webcrypto } from 'node:crypto'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import * as chatClient from '@/shared/api/chat-client'
import { quoteSha256 } from './ArtifactSelection'
import { ArtifactsPanel } from './ChatArtifactPanel'
import { mergeArtifactVersion } from './chat-artifacts'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, getComposerCapabilities: vi.fn() }
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.mocked(chatClient.getComposerCapabilities).mockReset()
})

function turn(artifact: ChatArtifact): ChatTurn {
  return {
    attachments: [],
    content: 'Lag et utkast',
    createdAt: '2026-11-04T10:00:00.000Z',
    id: 'turn-1',
    role: 'assistant',
    streaming: false,
    tools: [],
    artifacts: [artifact],
  }
}

function item(artifact: ChatArtifact): ArtifactPanelItem {
  return { artifact, turn: turn(artifact) }
}

function artifactOf(kind: string, id: string, title: string, content: string): ChatArtifact {
  return mergeArtifactVersion(undefined, { id, kind, title, content, version: 1 }) as ChatArtifact
}

/** The text nodes under `root`, in document order. */
function textNodes(root: Element): Text[] {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const nodes: Text[] = []
  for (let node = walker.nextNode(); node; node = walker.nextNode()) nodes.push(node as Text)
  return nodes
}

/** Makes the page report one selection from `anchor` to `focus`, as a drag across the text would. */
function selectOnPage(anchorNode: Node, anchorOffset: number, focusNode: Node, focusOffset: number): void {
  const selection = {
    isCollapsed: false,
    anchorNode,
    anchorOffset,
    focusNode,
    focusOffset,
  } as unknown as Selection
  // The document moves its selection more than once in the document test: reuse the spy the first call made.
  if (vi.isMockFunction(document.getSelection)) {
    vi.mocked(document.getSelection).mockReturnValue(selection)
  } else {
    vi.spyOn(document, 'getSelection').mockReturnValue(selection)
  }
}

function bodyOf(container: HTMLElement): HTMLElement {
  return container.querySelector('.verevon-chat-artifact-view__body') as HTMLElement
}

const CODE = 'const a = 1\nconst b = 2'

describe('ArtifactsPanel selection edit (EP-3 S6)', () => {
  it('offers no ask without the gateway capability', async () => {
    vi.mocked(chatClient.getComposerCapabilities).mockResolvedValue({
      ...chatClient.NO_COMPOSER_CAPABILITIES,
      selectionEdit: false,
    })
    const onAsk = vi.fn()
    const { container } = render(() => (
      <ArtifactsPanel items={[item(artifactOf('code', 'snippet', 'snippet.ts', CODE))]} onAskAboutSelection={onAsk} />
    ))
    await waitFor(() => expect(chatClient.getComposerCapabilities).toHaveBeenCalled())
    const second = textNodes(container.querySelectorAll('.verevon-chat-artifact-code__text')[1] as HTMLElement)
    selectOnPage(second[0] as Text, 0, second[second.length - 1] as Text, (second[second.length - 1] as Text).data.length)
    fireEvent.mouseUp(bodyOf(container))
    expect(screen.queryByRole('button', { name: 'Be om endring' })).toBeNull()
    expect(onAsk).not.toHaveBeenCalled()
  })

  it('sends_the_range_and_its_hash_as_one_turn', async () => {
    // jsdom does not provide SubtleCrypto; the panel and the expectation both use Node's implementation.
    vi.stubGlobal('crypto', webcrypto)
    vi.mocked(chatClient.getComposerCapabilities).mockResolvedValue({
      ...chatClient.NO_COMPOSER_CAPABILITIES,
      selectionEdit: true,
    })
    const onAsk = vi.fn()
    const { container } = render(() => (
      <ArtifactsPanel items={[item(artifactOf('code', 'snippet', 'snippet.ts', CODE))]} onAskAboutSelection={onAsk} />
    ))
    // The second line, from its first token to its last: characters 12 to 23 of the artifact.
    const second = textNodes(container.querySelectorAll('.verevon-chat-artifact-code__text')[1] as HTMLElement)
    const last = second[second.length - 1] as Text
    selectOnPage(second[0] as Text, 0, last, last.data.length)
    await waitFor(() => {
      fireEvent.mouseUp(bodyOf(container))
      expect(screen.getByRole('button', { name: 'Be om endring' })).toBeTruthy()
    })
    fireEvent.input(screen.getByRole('textbox', { name: 'Hva skal endres i det valgte?' }), {
      target: { value: 'Gjør dette formelt' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Be om endring' }))
    await waitFor(() => expect(onAsk).toHaveBeenCalledTimes(1))
    expect(onAsk).toHaveBeenCalledWith(expect.objectContaining({
      artifactId: 'snippet',
      version: 1,
      start: 12,
      end: 23,
      quoteSha256: await quoteSha256('const b = 2'),
      instruction: 'Gjør dette formelt',
    }))
  })

  it('asks_about_a_selection_in_the_document_source_and_not_in_the_rendered_text', async () => {
    vi.stubGlobal('crypto', webcrypto)
    vi.mocked(chatClient.getComposerCapabilities).mockResolvedValue({
      ...chatClient.NO_COMPOSER_CAPABILITIES,
      selectionEdit: true,
    })
    const content = '# Tittel\n\nEt **fet** ord.'
    const onAsk = vi.fn()
    const { container } = render(() => (
      <ArtifactsPanel items={[item(artifactOf('document', 'brief', 'Brief', content))]} onAskAboutSelection={onAsk} />
    ))
    // The rendered Markdown offers no selection: its text is not the artifact's text.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Vis kilde' })).toBeTruthy())
    const rendered = textNodes(container.querySelector('.verevon-chat-artifact-document') as HTMLElement)
    const word = rendered.find((node) => node.data === 'fet') as Text
    selectOnPage(word, 0, word, 3)
    fireEvent.mouseUp(bodyOf(container))
    expect(screen.queryByRole('button', { name: 'Be om endring' })).toBeNull()

    // The source view shows the Markdown exactly, so "fet" is characters 15 to 18 of the artifact.
    fireEvent.click(screen.getByRole('button', { name: 'Vis kilde' }))
    const source = textNodes(container.querySelector('pre') as HTMLElement)[0] as Text
    expect(source.data).toBe(content)
    selectOnPage(source, 15, source, 18)
    await waitFor(() => {
      fireEvent.mouseUp(bodyOf(container))
      expect(screen.getByRole('button', { name: 'Be om endring' })).toBeTruthy()
    })
    fireEvent.input(screen.getByRole('textbox', { name: 'Hva skal endres i det valgte?' }), {
      target: { value: 'Gjør dette tydeligere' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Be om endring' }))
    await waitFor(() => expect(onAsk).toHaveBeenCalledTimes(1))
    expect(onAsk).toHaveBeenCalledWith(expect.objectContaining({
      artifactId: 'brief',
      version: 1,
      start: 15,
      end: 18,
      quoteSha256: await quoteSha256('fet'),
    }))
  })
})
