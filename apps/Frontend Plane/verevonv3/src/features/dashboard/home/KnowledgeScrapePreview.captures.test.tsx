// @vitest-environment jsdom
import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ScrapeCapturesSection } from './KnowledgeScrapePreview'

// WS13.7: the "Skjermbilde / PDF" section renders only when the preview carries a capture.
describe('ScrapeCapturesSection (WS13.7)', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('shows the screenshot from the artifact route and a PDF download link', () => {
    const { container } = render(() => (
      <ScrapeCapturesSection
        title="Eksempel"
        captures={{
          screenshotUrl: '/api/v1/knowledge/scrape-artifacts/art_01',
          pdfUrl: '/api/v1/knowledge/scrape-artifacts/art_02',
        }}
      />
    ))
    const img = container.querySelector('img.knowledge-scrape-captures__img') as HTMLImageElement
    expect(img.getAttribute('src')).toBe('/api/v1/knowledge/scrape-artifacts/art_01')
    expect(img.getAttribute('alt')).toBe('Eksempel')
    expect(img.getAttribute('loading')).toBe('lazy')
    const link = container.querySelector('a.knowledge-scrape-captures__pdf') as HTMLAnchorElement
    expect(link.getAttribute('href')).toBe('/api/v1/knowledge/scrape-artifacts/art_02')
    expect(link.hasAttribute('download')).toBe(true)
  })

  it('renders nothing without a capture', () => {
    const { container } = render(() => <ScrapeCapturesSection title="Eksempel" />)
    expect(container.querySelector('section.knowledge-scrape-captures')).toBeNull()
  })

  it('shows the inline ZDR screenshot through a blob URL and revokes it on unmount', () => {
    const created = 'blob:captures-1'
    const revokeObjectURL = vi.fn()
    const urlStatics = URL as unknown as {
      createObjectURL?: (value: Blob) => string
      revokeObjectURL?: (value: string) => void
    }
    urlStatics.createObjectURL = vi.fn(() => created)
    urlStatics.revokeObjectURL = revokeObjectURL

    const { container, unmount } = render(() => (
      <ScrapeCapturesSection
        title="Eksempel"
        captures={{ screenshotInline: { contentType: 'image/png', dataBase64: btoa('png') } }}
      />
    ))
    expect(container.querySelector('img')?.getAttribute('src')).toBe(created)
    unmount()
    expect(revokeObjectURL).toHaveBeenCalledWith(created)
  })

  it('shows the Norwegian error sentence when the image cannot be loaded', async () => {
    const { container, getByText } = render(() => (
      <ScrapeCapturesSection
        title="Eksempel"
        captures={{ screenshotUrl: '/api/v1/knowledge/scrape-artifacts/art_01' }}
      />
    ))
    const img = container.querySelector('img') as HTMLImageElement
    fireEvent.error(img)
    await waitFor(() => expect(getByText('Bildet kunne ikke lastes.')).toBeTruthy())
  })
})
