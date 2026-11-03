import { expect, test, type Page, type Route } from '@playwright/test'
import { composer, openFresh } from './product-acceptance-helpers'

/**
 * WS18-S4, Projects in the chat sidebar, in the browser. It needs a build that carries
 * VITE_CHAT_PROJECTS_ENABLED=true (the build half of GATEWAY_CHAT_PROJECTS_ENABLED) and
 * is opted in with E2E_CHAT_PROJECTS_ENABLED=1, so a default build keeps the section absent.
 *
 * The Space routes are answered by page.route, so the listing, the roster and the membership
 * are fixed. No send completes: the first-send ask must appear, a cancel must keep the draft,
 * and the chat stream must never be called.
 */
test.skip(process.env.E2E_CHAT_PROJECTS_ENABLED !== '1', 'needs a build with VITE_CHAT_PROJECTS_ENABLED=true')

const launchRoom = { space_ref: 'space_e2e_launch', name: 'Launch room', kind: 'room', lifecycle: 'active', is_organization_room: false }
const organization = { space_ref: 'space_e2e_org', name: 'Organisasjon', kind: 'room', lifecycle: 'active', is_organization_room: true }

const envelope = (data: unknown) => ({ data })

async function mockSpaceRoutes(page: Page, role: string) {
  await page.route('**/api/v1/spaces', (route: Route) => route.fulfill({ json: envelope({ spaces: [launchRoom, organization] }) }))
  await page.route('**/api/v1/spaces/*/membership', (route: Route) => {
    const spaceRef = decodeURIComponent(new URL(route.request().url()).pathname.split('/')[4] ?? '')
    return route.fulfill({
      json: envelope({ space_ref: spaceRef, org_id: 'org_e2e', subject_id: 'user_e2e', kind: 'shared', role, revisions: {} }),
    })
  })
  await page.route('**/api/v1/spaces/*/context', (route: Route) => route.fulfill({
    json: envelope({
      space: launchRoom,
      membership: { space_ref: launchRoom.space_ref, org_id: 'org_e2e', subject_id: 'user_e2e', kind: 'shared', role, revisions: {} },
    }),
  }))
  await page.route('**/api/v1/spaces/*/roster', (route: Route) => route.fulfill({
    json: envelope({
      members: [
        { subject_type: 'user', subject_id: 'user_e2e', role, revision: 1, display_name: 'Ada' },
        { subject_type: 'user', subject_id: 'user_b', role: 'viewer', revision: 1, display_name: 'Bo' },
      ],
    }),
  }))
  await page.route('**/api/v1/spaces/*/threads', (route: Route) => route.fulfill({
    json: envelope({ space: launchRoom, membership: { space_ref: launchRoom.space_ref, role }, threads: [] }),
  }))
}

test('projects sit above the chat list and leave out the organization channel', async ({ page }) => {
  await mockSpaceRoutes(page, 'editor')
  await openFresh(page, '/chat', { mockedInference: true })

  const section = page.getByRole('region', { name: /Prosjekter|Projects/ })
  await expect(section.getByRole('button', { name: /^Launch room/ })).toBeVisible()
  await expect(section.getByRole('button', { name: /Organisasjon/ })).toHaveCount(0)
})

test('a viewer reads a room but cannot start a project chat in it', async ({ page }) => {
  await mockSpaceRoutes(page, 'viewer')
  await openFresh(page, '/chat', { mockedInference: true })

  const section = page.getByRole('region', { name: /Prosjekter|Projects/ })
  await expect(section.getByRole('button', { name: /Ny samtale i Launch room|New chat in Launch room/ })).toHaveCount(0)
})

test('the first send to a shared project asks who can see it, and a cancel keeps the draft', async ({ page }) => {
  await mockSpaceRoutes(page, 'editor')
  const streamCalls: string[] = []
  page.on('request', (request) => {
    if (request.method() === 'POST' && request.url().includes('/chat/stream')) streamCalls.push(request.url())
  })
  await openFresh(page, '/chat', { mockedInference: true })

  const section = page.getByRole('region', { name: /Prosjekter|Projects/ })
  await section.getByRole('button', { name: /^Ny samtale i Launch room$|^New chat in Launch room$/ }).click()
  await expect(page.locator('.verevon-chat-project-chip')).toContainText('Launch room')

  await composer(page).fill('Hva er status for lanseringen?')
  await composer(page).press('Enter')

  const ask = page.getByRole('alertdialog')
  await expect(ask).toContainText(/2 medlemmer|2 members/)
  await ask.getByRole('button', { name: /^Avbryt$|^Cancel$/ }).click()

  await expect(composer(page)).toHaveValue('Hva er status for lanseringen?')
  expect(streamCalls).toEqual([])
})
