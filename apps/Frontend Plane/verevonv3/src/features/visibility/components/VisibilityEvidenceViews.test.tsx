// @vitest-environment jsdom
import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { afterEach, expect, it } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { analysis } from '@/shared/api/visibility/__fixtures__/tools'
import { GroupsView, LinksView, PagesView } from './VisibilityEvidenceViews'
afterEach(cleanup)

it('shows measured zero separately from unknown page metrics, with working filters', () => {
  render(() => <I18nProvider><PagesView data={analysis()} /></I18nProvider>)
  const rows = screen.getAllByRole('row')
  expect(within(rows[1]!).getAllByRole('cell')[1]!.textContent).toBe('0')
  expect(within(rows[2]!).getAllByRole('cell')[1]!.textContent).toBe('—')
  fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'Unmeasured' } })
  expect(screen.queryByText('Measured homepage')).toBeNull()
  expect(screen.getByText('Unmeasured page')).toBeTruthy()
  fireEvent.click(screen.getByText('Unmeasured page'))
  expect(screen.getByRole('link', { name: /Åpne side|Open page/ }).getAttribute('rel')).toContain('noopener')
})
it('opens explicit SEO group rules and links page findings to their evidence', () => {
  render(() => <I18nProvider><Router><Route path="*all" component={() => <GroupsView data={analysis()} />} /></Router></I18nProvider>)
  expect(screen.getByText('relevant query')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: /Få observerte inngående lenker|Few observed incoming links/ }))
  expect(screen.queryByText('relevant query')).toBeNull()
  expect(screen.getByRole('link', { name: /Se grunnlag|View evidence/ }).getAttribute('href')).toContain('/pages?url=')
  expect(screen.getAllByText(/beviser ikke|does not prove/).length).toBeGreaterThan(0)
})
it('counts unchecked links separately and never presents them as broken', () => {
  render(() => <I18nProvider><Router><Route path="*all" component={() => <LinksView data={analysis()} />} /></Router></I18nProvider>)
  expect(screen.queryByRole('heading', { name: /Bekreftede brutte lenker|Confirmed broken links/ })).toBeNull()
  const row = screen.getAllByRole('row')[2]!
  expect(within(row).getAllByRole('cell')[2]!.textContent).toBe('—')
  fireEvent.change(screen.getByRole('combobox', { name: /Vis sider|Show pages/ }), { target: { value: 'unreached' } })
  expect(within(screen.getByRole('table', { name: /Lenkestruktur|Link structure/ })).getAllByRole('row')).toHaveLength(1)
})
