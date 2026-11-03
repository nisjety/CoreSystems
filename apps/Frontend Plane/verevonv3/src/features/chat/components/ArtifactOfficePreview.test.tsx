// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ArtifactOfficePreview, officePdfRenderer } from './ArtifactOfficePreview'
import type { ChatArtifact, GeneratedFile, GeneratedFilePreview } from './chat-types'

const XLSX = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'

function preview(overrides: Partial<GeneratedFilePreview> = {}): GeneratedFilePreview {
  return {
    mime: 'application/pdf',
    url: 'data:application/pdf;base64,JVBERi0=',
    pages: 3,
    fontsSubstituted: false,
    thumbs: ['data:image/png;base64,iVBORw0KGgo=', 'data:image/png;base64,iVBORw0KGgo='],
    ...overrides,
  }
}

function artifact(overrides: Partial<ChatArtifact> = {}): ChatArtifact {
  return {
    content: `data:${XLSX};base64,QUFB`,
    id: 'call-1-file-1',
    kind: 'spreadsheet',
    title: 'budget.xlsx',
    version: 1,
    ...overrides,
  }
}

function generatedFile(overrides: Partial<GeneratedFile> = {}): GeneratedFile {
  return { id: 'call-1-file-1', name: 'budget.xlsx', mime: XLSX, size: 3, url: `data:${XLSX};base64,QUFB`, ...overrides }
}

describe('ArtifactOfficePreview', () => {
  beforeEach(() => {
    window.localStorage.setItem(localeStorageKey, 'en')
  })

  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('draws the PDF in a frame, with one strip button per thumbnail', () => {
    render(() => (
      <I18nProvider>
        <ArtifactOfficePreview title="budget.xlsx" preview={preview()} />
      </I18nProvider>
    ))
    const frame = screen.getByTitle('Preview of budget.xlsx')
    expect(frame.tagName).toBe('IFRAME')
    expect(frame.getAttribute('src')).toBe('data:application/pdf;base64,JVBERi0=#page=1')
    expect(screen.getByRole('button', { name: 'Page 1' }).getAttribute('aria-current')).toBe('page')
    expect(screen.getAllByRole('button', { name: /^Page \d$/ })).toHaveLength(2)
  })

  it('moves the frame to the page whose thumbnail is picked', () => {
    render(() => (
      <I18nProvider>
        <ArtifactOfficePreview title="budget.xlsx" preview={preview()} />
      </I18nProvider>
    ))
    fireEvent.click(screen.getByRole('button', { name: 'Page 2' }))
    flush()
    expect(screen.getByTitle('Preview of budget.xlsx').getAttribute('src')).toMatch(/#page=2$/)
    expect(screen.getByRole('button', { name: 'Page 2' }).getAttribute('aria-current')).toBe('page')
    expect(screen.getByRole('button', { name: 'Page 1' }).getAttribute('aria-current')).toBeNull()
  })

  it('says that a font was drawn with a stand-in only when one was', () => {
    const { unmount } = render(() => (
      <I18nProvider>
        <ArtifactOfficePreview title="budget.xlsx" preview={preview({ fontsSubstituted: false })} />
      </I18nProvider>
    ))
    expect(screen.queryByText('Fonts substituted in preview')).toBeNull()
    unmount()

    render(() => (
      <I18nProvider>
        <ArtifactOfficePreview title="budget.xlsx" preview={preview({ fontsSubstituted: true })} />
      </I18nProvider>
    ))
    expect(screen.getByRole('note').textContent).toBe('Fonts substituted in preview')
  })

  it('draws no strip when the render made no thumbnails', () => {
    render(() => (
      <I18nProvider>
        <ArtifactOfficePreview title="budget.xlsx" preview={preview({ thumbs: [] })} />
      </I18nProvider>
    ))
    expect(screen.queryByRole('list')).toBeNull()
    expect(screen.getByTitle('Preview of budget.xlsx').tagName).toBe('IFRAME')
  })
})

describe('officePdfRenderer', () => {
  beforeEach(() => {
    window.localStorage.setItem(localeStorageKey, 'en')
  })

  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('claims generated Word, Excel and PowerPoint files and nothing else', () => {
    expect(officePdfRenderer.match(artifact())).toBe(true)
    expect(officePdfRenderer.match(artifact({ content: 'data:application/pdf;base64,AA', kind: 'binary' }))).toBe(false)
    expect(officePdfRenderer.match(artifact({ content: '<p>hei</p>', kind: 'html' }))).toBe(false)
  })

  it('draws the preview of the file the artifact came from', () => {
    const Renderer = officePdfRenderer.component!
    render(() => (
      <I18nProvider>
        <Renderer
          artifact={artifact()}
          file={generatedFile({ preview: preview() })}
          fallback={() => <p>File card</p>}
        />
      </I18nProvider>
    ))
    expect(screen.getByTitle('Preview of budget.xlsx').tagName).toBe('IFRAME')
    expect(screen.queryByText('File card')).toBeNull()
  })

  it('hands the panel its own body when the file has no preview', () => {
    const Renderer = officePdfRenderer.component!
    render(() => (
      <I18nProvider>
        <Renderer artifact={artifact()} file={generatedFile()} fallback={() => <p>File card</p>} />
      </I18nProvider>
    ))
    expect(screen.getByText('File card')).toBeTruthy()
    expect(screen.queryByTitle('Preview of budget.xlsx')).toBeNull()
  })
})
