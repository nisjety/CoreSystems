// @vitest-environment jsdom

import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { ArtifactsPanel } from './ChatArtifactPanel'
import { mergeArtifactVersion } from './chat-artifacts'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'

function turn(overrides: Partial<ChatTurn> = {}): ChatTurn {
  return {
    attachments: [],
    content: 'Lag et dokument',
    createdAt: '2026-07-30T10:00:00.000Z',
    id: 'turn-1',
    role: 'assistant',
    streaming: false,
    tools: [],
    ...overrides,
  }
}

function item(artifact: ChatArtifact, file?: ArtifactPanelItem['file']): ArtifactPanelItem {
  return { artifact, file, turn: turn({ artifacts: [artifact] }) }
}

function versioned(revisions: Array<{ content: string; title: string; version: number }>): ChatArtifact {
  return revisions.reduce<ChatArtifact | undefined>(
    (carried, revision) => mergeArtifactVersion(carried, {
      content: revision.content,
      id: 'artifact-1',
      kind: 'document',
      title: revision.title,
      version: revision.version,
    }),
    undefined,
  ) as ChatArtifact
}

describe('ArtifactsPanel', () => {
  it('shows the honest empty state when the conversation has no artifacts', () => {
    render(() => <ArtifactsPanel items={[]} />)
    expect(screen.getByText('Ingen artefakter ennå')).toBeTruthy()
  })

  it('renders a code artifact with a line-number gutter', () => {
    const code: ChatArtifact = {
      content: 'def hei():\n    return 1\n',
      id: 'artifact-1',
      kind: 'code',
      title: 'solve.py',
      version: 1,
    }
    const { container } = render(() => <ArtifactsPanel items={[item(code)]} />)

    const gutters = container.querySelectorAll('.verevon-chat-artifact-code__gutter')
    expect([...gutters].map((node) => node.textContent)).toEqual(['1', '2'])
    // The language recovered from the title drives the highlighter.
    expect(container.querySelector('.verevon-code-keyword')?.textContent).toBe('def')
  })

  it('previews HTML in an iframe sandboxed WITHOUT allow-same-origin', () => {
    // Security requirement: model-generated HTML is untrusted. `allow-scripts`
    // together with `allow-same-origin` would let the page drop its own sandbox
    // and reach this app's cookies and same-origin /api BFF, so the pair must
    // never appear together.
    const page: ChatArtifact = {
      content: '<html><body><script>parent.document.title = "pwned"</script></body></html>',
      id: 'artifact-1',
      kind: 'html',
      title: 'landing.html',
      version: 1,
    }
    const { container } = render(() => <ArtifactsPanel items={[item(page)]} />)

    const frame = container.querySelector('iframe')
    expect(frame).toBeTruthy()
    expect(frame?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(frame?.getAttribute('sandbox')).not.toContain('allow-same-origin')
    expect(frame?.getAttribute('srcdoc')).toBe(page.content)
    expect(frame?.getAttribute('referrerpolicy')).toBe('no-referrer')
  })

  it('toggles between the HTML preview and its source', () => {
    const page: ChatArtifact = {
      content: '<h1>Hei</h1>',
      id: 'artifact-1',
      kind: 'html',
      title: 'index.html',
      version: 1,
    }
    const { container } = render(() => <ArtifactsPanel items={[item(page)]} />)

    screen.getByText('Vis kode').click()
    expect(container.querySelector('iframe')).toBeNull()
    expect(container.querySelector('.verevon-chat-artifact-code')).toBeTruthy()

    screen.getByText('Vis forhåndsvisning').click()
    expect(container.querySelector('iframe')).toBeTruthy()
  })

  it('opens on the newest revision and steps back through the history', () => {
    const doc = versioned([
      { content: 'Første utkast', title: 'Rapport v1', version: 1 },
      { content: 'Andre utkast', title: 'Rapport v2', version: 2 },
      { content: 'Tredje utkast', title: 'Rapport v3', version: 3 },
    ])
    render(() => <ArtifactsPanel items={[item(doc)]} />)

    expect(screen.getByText('v3 · 3/3')).toBeTruthy()
    expect(screen.getByText('Tredje utkast')).toBeTruthy()

    screen.getByLabelText('Forrige versjon').click()
    expect(screen.getByText('v2 · 2/3')).toBeTruthy()
    expect(screen.getByText('Andre utkast')).toBeTruthy()

    screen.getByLabelText('Neste versjon').click()
    expect(screen.getByText('v3 · 3/3')).toBeTruthy()
  })

  it('offers a working download for a generated spreadsheet', () => {
    const href = `data:application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;base64,${btoa('sheet')}`
    const sheet: ChatArtifact = {
      content: href,
      id: 'artifact-1',
      kind: 'spreadsheet',
      title: 'Q4-tall',
      version: 1,
    }
    render(() => <ArtifactsPanel items={[item(sheet)]} />)

    const links = screen.getAllByText('Last ned').map((node) => node.closest('a'))
    expect(links.length).toBeGreaterThan(0)
    for (const link of links) {
      expect(link?.getAttribute('href')).toBe(href)
      expect(link?.getAttribute('download')).toBe('q4-tall.xlsx')
    }
    expect(screen.getAllByText(/Excel-regneark \(XLSX\)/).length).toBeGreaterThan(0)
  })

  it('says an announced artifact could not be loaded instead of showing an empty box', () => {
    const announced: ChatArtifact = {
      content: '',
      id: 'artifact-1',
      kind: 'spreadsheet',
      title: 'Q4.xlsx',
      version: 1,
    }
    render(() => <ArtifactsPanel items={[item(announced)]} />)

    expect(screen.getByText('Artefaktet kunne ikke lastes')).toBeTruthy()
    // Nothing to download, so no download control is offered at all.
    expect(screen.queryByText('Last ned')).toBeNull()
  })

  it('copies the shown revision to the clipboard', async () => {
    const writeText = vi.fn(async () => undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    const code: ChatArtifact = {
      content: 'SELECT 1',
      id: 'artifact-1',
      kind: 'sql',
      title: 'query.sql',
      version: 1,
    }
    render(() => <ArtifactsPanel items={[item(code)]} />)

    screen.getByText('Kopier').click()
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('SELECT 1'))
    await vi.waitFor(() => expect(screen.getByText('Kopiert')).toBeTruthy())
    vi.unstubAllGlobals()
  })

  it('reports a refused clipboard write instead of claiming success', async () => {
    // A denied permission or insecure context must not render as "Kopiert".
    const writeText = vi.fn(async () => {
      throw new Error('NotAllowedError')
    })
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    const doc: ChatArtifact = {
      content: '# Notat',
      id: 'artifact-1',
      kind: 'document',
      title: 'Notat',
      version: 1,
    }
    render(() => <ArtifactsPanel items={[item(doc)]} />)

    screen.getByText('Kopier').click()
    await vi.waitFor(() => expect(screen.getByText('Kunne ikke kopiere')).toBeTruthy())
    expect(screen.queryByText('Kopiert')).toBeNull()
    vi.unstubAllGlobals()
  })

  it('switches the viewer when another artifact is selected from the list', () => {
    const code: ChatArtifact = {
      content: 'SELECT 1',
      id: 'artifact-code',
      kind: 'sql',
      title: 'query.sql',
      version: 1,
    }
    const doc: ChatArtifact = {
      content: '# Notat',
      id: 'artifact-doc',
      kind: 'document',
      title: 'Notat',
      version: 1,
    }
    const { container } = render(() => <ArtifactsPanel items={[item(code), item(doc)]} />)

    // The newest artifact is selected by default.
    expect(container.querySelector('.verevon-chat-artifact-document')).toBeTruthy()

    screen.getByText('query.sql').click()
    expect(container.querySelector('.verevon-chat-artifact-code')).toBeTruthy()
    expect(container.querySelector('.verevon-chat-artifact-document')).toBeNull()
  })
})
