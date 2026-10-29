// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { StudioDemoShell } from './StudioDemoShell'

function renderDemo(path = '/studio/canvas') {
  window.history.pushState(null, '', path)
  return render(() => (
    <I18nProvider>
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/*all" component={StudioDemoShell} />
      </Router>
    </I18nProvider>
  ))
}

describe('StudioDemoShell', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('renders the product navbar and Studio sidebar without contacting a backend', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    renderDemo()

    await waitFor(() => {
      expect(screen.getAllByRole('banner').length).toBeGreaterThanOrEqual(2)
      expect(screen.getByLabelText(/primærnavigasjon|primary navigation/i)).toBeTruthy()
    })

    expect(screen.getAllByRole('link', { name: 'Canvas' }).length).toBeGreaterThanOrEqual(2)
    expect((screen.getByRole('button', { name: /kunnskapssøk|knowledge search/i }) as HTMLButtonElement).disabled).toBe(true)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('keeps Studio navigation local when changing the visible Studio section', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    renderDemo()
    fireEvent.click(screen.getByRole('link', { name: /kampanjeplanlegger|campaign planner/i }))

    await waitFor(() => {
      expect(window.location.pathname).toBe('/studio/campaigns')
    })

    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('opens the read-only Marketing Pulse from the Marketing sidebar', async () => {
    const fetchMock = vi.fn().mockRejectedValue(new Error('backend unavailable'))
    vi.stubGlobal('fetch', fetchMock)

    renderDemo()
    fireEvent.click(screen.getByRole('link', { name: /markedsføring|marketing/i }))

    await waitFor(() => {
      expect(window.location.pathname).toBe('/studio/pulse')
      expect(screen.getByRole('heading', { level: 1, name: /pulse|puls/i })).toBeTruthy()
    })

    expect(screen.getAllByRole('link', { name: /pulse|puls/i })).toHaveLength(2)
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/marketing/websites', expect.objectContaining({ method: 'GET', credentials: 'include' }))
  })

  it('normalizes the demo root to the Studio canvas route', async () => {
    renderDemo('/')

    await waitFor(() => {
      expect(window.location.pathname).toBe('/studio/canvas')
    })
  })
})
