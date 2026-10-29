// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { comparisons, context } from '@/shared/api/visibility/__fixtures__/tools'
import { compareCompetitor, loadCompetitors, type Competitors } from '@/shared/api/visibility/tools-client'
import { CompetitorPanel } from './CompetitorPanel'
vi.mock('@/shared/api/visibility/tools-client', () => ({ loadCompetitors: vi.fn(), compareCompetitor: vi.fn() }))
beforeEach(() => { vi.mocked(loadCompetitors).mockReset(); vi.mocked(compareCompetitor).mockReset() })
afterEach(cleanup)
it('lets viewers read cached comparisons without offering write controls', async () => {
  vi.mocked(loadCompetitors).mockResolvedValue(comparisons())
  render(() => <I18nProvider><CompetitorPanel context={context} canWrite={false} /></I18nProvider>)
  await screen.findByRole('heading', { name: /Velg nettstedene|Choose websites/ })
  expect(screen.queryByRole('textbox')).toBeNull()
  expect(compareCompetitor).not.toHaveBeenCalled()
})
it('starts a comparison only on explicit submit, bound to the current website revision', async () => {
  vi.mocked(loadCompetitors).mockResolvedValue(comparisons())
  vi.mocked(compareCompetitor).mockResolvedValue(comparisons())
  render(() => <I18nProvider><CompetitorPanel context={context} canWrite /></I18nProvider>)
  await screen.findByRole('heading', { name: /Velg nettstedene|Choose websites/ })
  fireEvent.input(screen.getByRole('textbox', {name:/Konkurrentens nettadresse|Competitor website URL/}), { target: { value: 'https://competitor.no/' } })
  fireEvent.click(screen.getByRole('button', { name: /Legg til og sammenlign|Add and compare/ }))
  await waitFor(() => expect(compareCompetitor).toHaveBeenCalledWith(context, 'https://competitor.no/', 'refresh', expect.any(AbortSignal), {ownUrl:context.site,targetUrl:'https://competitor.no/'}))
})
it('shows a useful validation error for a non-public target without sending a command', async () => {
  vi.mocked(loadCompetitors).mockResolvedValue(comparisons())
  render(() => <I18nProvider><CompetitorPanel context={context} canWrite /></I18nProvider>)
  await screen.findByRole('heading', { name: /Velg nettstedene|Choose websites/ })
  fireEvent.input(screen.getByRole('textbox', {name:/Konkurrentens nettadresse|Competitor website URL/}), { target: { value: 'https://localhost/' } })
  fireEvent.click(screen.getByRole('button', { name: /Legg til og sammenlign|Add and compare/ }))
  expect(screen.getByRole('alert').textContent).toMatch(/offentlige HTTPS|public HTTPS/)
  expect(compareCompetitor).not.toHaveBeenCalled()
})
it('aborts old-site reads and rejects their late results on a context switch', async () => {
  let resolveOld!: (v: Competitors) => void
  vi.mocked(loadCompetitors).mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve })).mockResolvedValue(comparisons())
  const [current, setCurrent] = createSignal(context)
  render(() => <I18nProvider><CompetitorPanel context={current()} canWrite /></I18nProvider>)
  const signal = vi.mocked(loadCompetitors).mock.calls[0]![1]!
  const next = { ...context, websiteRevision: 8 }
  setCurrent(next)
  expect(signal.aborted).toBe(true)
  resolveOld({ ...comparisons(), entries: [{ ...context, target: 'https://wrong.no/', status: 'queued', requestedAt: '2026-10-28T12:00:00Z', nextRunAt: '2026-10-28T12:00:00Z' }] })
  await waitFor(() => expect(loadCompetitors).toHaveBeenLastCalledWith(next, expect.any(AbortSignal)))
  expect(screen.queryByRole('heading', { name: 'wrong.no' })).toBeNull()
})
