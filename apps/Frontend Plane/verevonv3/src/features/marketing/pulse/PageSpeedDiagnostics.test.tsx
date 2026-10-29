// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { PageSpeedDiagnostics } from './PageSpeedDiagnostics.tsx'

describe('PageSpeed technical diagnostics', () => {
  afterEach(() => { cleanup(); window.localStorage.clear() })
  it('shows exactly eleven owned labels with truthful mixed status counts and review filtering', () => {
    render(() => <I18nProvider><PageSpeedDiagnostics checks={[
      { id: 'document-title', status: 'passed', score: 1 }, { id: 'canonical', status: 'failed', score: 0 },
      { id: 'structured-data', status: 'manual', score: null }, { id: 'robots-txt', status: 'error', score: null },
      { id: 'hreflang', status: 'not_applicable', score: null }, { id: 'meta-description', status: 'informative', score: null },
    ]} /></I18nProvider>)
    expect(screen.getAllByRole('listitem')).toHaveLength(11)
    expect(screen.getByText(/Bestått: 1|Passed: 1/)).toBeTruthy()
    expect(screen.getByText(/Trenger vurdering: 3|Needs review: 3/)).toBeTruthy()
    expect(screen.getByText(/Ikke rapportert: 5|Not reported: 5/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Trenger vurdering \(3\)|Needs review \(3\)/ }))
    expect(screen.getAllByRole('listitem')).toHaveLength(3)
    expect(screen.queryByText(/Sidetittel|Document title/)).toBeNull()
    expect(screen.getByRole('button', { name: /Trenger vurdering \(3\)|Needs review \(3\)/ }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByText(/Manuell vurdering|Manual review/)).toBeTruthy()
    expect(screen.getByText(/Feil ved kontroll|Check error/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Alle \(11\)|All \(11\)/ }))
    expect(screen.getAllByRole('listitem')).toHaveLength(11)
    expect(screen.getByText(/omdirigering|redirect/i)).toBeTruthy()
  })
  it('keeps older cached reports unavailable rather than inferring passes from category scores', () => {
    render(() => <I18nProvider><PageSpeedDiagnostics /></I18nProvider>)
    expect(screen.getAllByRole('listitem')).toHaveLength(11)
    expect(screen.getByText(/Bestått: 0|Passed: 0/)).toBeTruthy()
    expect(screen.getByText(/Ikke rapportert: 11|Not reported: 11/)).toBeTruthy()
    for (const row of screen.getAllByRole('listitem')) expect(within(row).getByText(/Ikke rapportert|Not reported/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Trenger vurdering \(0\)|Needs review \(0\)/ }))
    expect(screen.queryAllByRole('listitem')).toHaveLength(0)
    expect(screen.getByRole('status').textContent).toMatch(/ingen rapporterte|no reported/i)
  })
})
