// @vitest-environment jsdom

import { createRouter, memoryHistory, useSearchParams } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { I18nProvider } from '@/shared/i18n'

// The Home tab lives in ?tab= (DashboardHome), so the sidebar's address changes
// whenever the person switches tab. That must not reset the sidebar's own state.

function Harness(props: { route: '/dashboard' | '/studio' }) {
  const [, setSearchParams] = useSearchParams()
  const setParams = (params: Record<string, string | undefined>) => () => setSearchParams(params)
  return (
    <>
      <CoreSidebar activeRoute={props.route} expanded onExpandedChange={vi.fn()} onOpenSearch={vi.fn()} />
      <button type="button" onClick={setParams({ tab: 'web' })}>tab: web</button>
      <button type="button" onClick={setParams({ tab: 'extract' })}>tab: extract</button>
      <button type="button" onClick={setParams({ tab: undefined })}>tab: chat</button>
      <button type="button" onClick={setParams({ overview: 'knowledge' })}>overview: knowledge</button>
    </>
  )
}

function renderSidebar(path: string, route: '/dashboard' | '/studio' = '/dashboard') {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <Harness route={route} /> }],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

const filterBox = () => screen.getByRole('textbox', { name: 'Filtrer sidefeltseksjon' }) as HTMLInputElement
const panelTab = (name: string) => screen.getByRole('button', { name })
const isActive = (name: string) => panelTab(name).className.includes('verevon-sidebar-tab-active')
const switchTab = async (name: 'web' | 'extract' | 'chat') => {
  fireEvent.click(screen.getByRole('button', { name: `tab: ${name}` }))
  flush()
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

afterEach(cleanup)

describe('Sidebar on Home when the tab in the address changes', () => {
  it('keeps what was typed in the section filter and the selected panel tab', async () => {
    renderSidebar('/dashboard')
    fireEvent.click(panelTab('Delt med meg'))
    flush()
    fireEvent.input(filterBox(), { target: { value: 'kunnskap' } })
    flush()
    expect(filterBox().value).toBe('kunnskap')
    expect(isActive('Delt med meg')).toBe(true)

    for (const name of ['web', 'extract', 'chat'] as const) {
      await switchTab(name)
      expect(filterBox().value).toBe('kunnskap')
      expect(isActive('Delt med meg')).toBe(true)
      expect(isActive('Min konto')).toBe(false)
    }
  })

  it('still starts over when the overview section moves to the other group', async () => {
    renderSidebar('/dashboard')
    fireEvent.click(panelTab('Min konto'))
    flush()
    fireEvent.input(filterBox(), { target: { value: 'hjem' } })
    flush()

    fireEvent.click(screen.getByRole('button', { name: 'overview: knowledge' }))
    flush()

    await waitFor(() => expect(filterBox().value).toBe(''))
    expect(isActive('Delt med meg')).toBe(true)
  })

  it('keeps a collapsed group collapsed when only another query parameter changes', async () => {
    renderSidebar('/studio/canvas', '/studio')
    const group = () => screen.getByRole('button', { name: /^(Slå sammen|Utvid) Opprett$/ })
    expect(group().getAttribute('aria-expanded')).toBe('true')
    fireEvent.click(group())
    flush()
    expect(group().getAttribute('aria-expanded')).toBe('false')

    await switchTab('web')

    expect(group().getAttribute('aria-expanded')).toBe('false')
  })
})
