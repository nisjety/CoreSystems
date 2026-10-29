// @vitest-environment jsdom
import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, expect, it } from 'vitest'
import { anbudWorkspace, resetAnbudState, anbudFilters } from '@/features/anbud/lib/anbud-state'
import { AnbudExpandedSidebarPanel } from './CoreSidebarAnbudPanel'

beforeEach(() => { resetAnbudState(); anbudWorkspace.setView('pipeline'); window.history.pushState(null, '', '/anbud') })
afterEach(cleanup)

function renderSidebar() {
  render(() => <Router><Route path="/anbud" component={() => <AnbudExpandedSidebarPanel onCollapse={() => undefined} />} /></Router>)
}

it('selects a pipeline stage in the shell', () => {
  renderSidebar()
  expect(screen.getByRole('link', { name: /Til Engasjement|Back to Engagement/ }).getAttribute('href')).toBe('/engagement')
  fireEvent.click(screen.getByRole('button', { name: /Vurderes/ }))
  expect(anbudWorkspace.view()).toBe('pipeline')
  expect(anbudWorkspace.stage()).toBe('review')
  expect(screen.getByRole('button', { name: /Vurderes/ }).getAttribute('aria-current')).toBe('page')
})

it('opens the shared company profile editor and filters sidebar stages', () => {
  renderSidebar()
  fireEvent.click(screen.getByRole('button', { name: 'Rediger profil' }))
  expect(anbudWorkspace.profileOpen()).toBe(true)
  fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Levert' } })
  expect(screen.queryByRole('button', { name: /Innboks/ })).toBeNull()
  expect(screen.getByRole('button', { name: /Levert/ })).toBeTruthy()
})

it('replaces pipeline navigation with tender filters and radar filters', () => {
  renderSidebar()
  anbudWorkspace.setView('all')
  expect(screen.queryByRole('button', { name: /Innboks/ })).toBeNull()
  fireEvent.click(screen.getByRole('radio', { name: 'Aktiv frist' }))
  expect(anbudFilters.filters().status).toBe('active')
  fireEvent.input(screen.getByRole('searchbox', { name: 'Filtrer innkjøper' }), { target: { value: 'Oslo' } })
  expect(anbudFilters.filters().buyer).toBe('Oslo')
  fireEvent.click(screen.getByRole('button', { name: 'Nullstill' }))
  expect(anbudFilters.filters().buyer).toBe('')
  anbudWorkspace.setView('radar')
  expect(screen.queryByRole('radio', { name: 'Aktiv frist' })).toBeNull()
  fireEvent.click(screen.getByRole('radio', { name: 'Nær match' }))
  expect(anbudFilters.radarReach()).toBe('near')
})

it('shows catalogue-wide filter counts rather than the current page size', () => {
  anbudWorkspace.setView('all')
  anbudFilters.setFacets({ status: { all: 149, active: 80, expired: 40 }, deadline: { all: 149, soon: 20 }, noticeType: { all: 149, competition: 100 } })
  renderSidebar()
  const status = screen.getByRole('group', { name: 'STATUS' })
  expect(within(status).getByText('149')).toBeTruthy()
  expect(within(status).getByText('80')).toBeTruthy()
  expect(within(status).getByText('40')).toBeTruthy()
})
