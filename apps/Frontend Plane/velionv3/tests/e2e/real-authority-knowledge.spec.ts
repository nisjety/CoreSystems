import { readFileSync } from 'node:fs'
import { expect, test, type Page } from '@playwright/test'

type AuthorityUser = {
  email: string
  name: string
  orgId: string
  password: string
  userId: string
}

type BrowserFixture = {
  documentId: string
  graphMarker: string
  orgId: string
  title: string
}

const authority = readFixture<{ users: AuthorityUser[] }>('REAL_AUTHORITY_FIXTURE_FILE')
const fixtures = readFixture<{ fixtures: BrowserFixture[] }>('REAL_AUTHORITY_BROWSER_FIXTURE_FILE')

test.beforeAll(() => {
  expect(authority.users).toHaveLength(2)
  expect(fixtures.fixtures).toHaveLength(2)
  expect(authority.users[0].orgId).not.toBe(authority.users[1].orgId)
  expect(fixtures.fixtures[0].orgId).toBe(authority.users[0].orgId)
  expect(fixtures.fixtures[1].orgId).toBe(authority.users[1].orgId)
})

test('real Auth/User session surfaces isolated Knowledge, GraphRAG, and navbar retrieval', async ({ page, baseURL }) => {
  const user = authority.users[0]
  const fixture = fixtures.fixtures[0]
  const otherFixture = fixtures.fixtures[1]
  await authenticate(page, baseURL, user)

  const workspaceResponse = page.waitForResponse((response) =>
    response.url().includes('/api/v1/knowledge/sources') && response.request().method() === 'GET',
  )
  await page.goto('/knowledge')
  expect((await workspaceResponse).status()).toBe(200)
  await expect(page.getByRole('heading', { name: 'Knowledge', exact: true })).toBeVisible()
  await expect(page.getByText(fixture.title).first()).toBeVisible()
  await expect(page.getByText(otherFixture.title)).toHaveCount(0)

  const filter = page.getByRole('textbox', { name: 'Search knowledge base' })
  await filter.fill(fixture.graphMarker)
  await expect(page.getByText(fixture.title).first()).toBeVisible()
  await expect(page.getByText(otherFixture.title)).toHaveCount(0)
  await filter.clear()

  await page.getByRole('button', { name: 'Graph', exact: true }).click()
  await expect(page.getByRole('region', { name: 'RAGGraph relationship map' })).toBeVisible()
  await expect(page.getByRole('button', { name: `Select ${fixture.graphMarker}`, exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: `Select ${otherFixture.graphMarker}`, exact: true })).toHaveCount(0)

  await page.getByRole('button', { name: /^(Open knowledge search|Åpne kunnskapssøk)$/ }).click()
  const navbarInput = page.getByRole('textbox', { name: /^(Search the knowledge base|Søk i kunnskapsbasen)$/ })
  const navbarResponse = page.waitForResponse((response) =>
    response.url().includes('/api/v1/navbar/search') && response.url().includes(encodeURIComponent(fixture.graphMarker)),
  )
  await navbarInput.fill(fixture.graphMarker)
  expect((await navbarResponse).status()).toBe(200)
  const result = page.getByRole('button').filter({ hasText: fixture.title }).first()
  await expect(result).toBeVisible()
  await result.click()

  await expect(page).toHaveURL(new RegExp(`/knowledge\\?source=${escapeRegex(fixture.documentId)}$`))
  const selectedSource = page.getByRole('button', {
    name: new RegExp(`^${escapeRegex(fixture.title)}`),
    pressed: true,
  })
  await expect(selectedSource).toHaveAttribute('aria-pressed', 'true')
})

test('cross-tenant headers and search cannot disclose the other isolated fixture', async ({ page, baseURL }) => {
  const user = authority.users[0]
  const ownFixture = fixtures.fixtures[0]
  const otherFixture = fixtures.fixtures[1]
  await authenticate(page, baseURL, user)

  const spoofed = await page.request.get('/api/v1/knowledge/sources', {
    headers: { 'x-velion-org-id': authority.users[1].orgId },
  })
  expect(spoofed.status()).toBe(200)
  const workspace = await spoofed.json() as { orgId?: string; sources?: Array<{ title?: string }> }
  expect(workspace.orgId).toBe(user.orgId)
  expect(workspace.sources?.some((source) => source.title === ownFixture.title)).toBe(true)
  expect(workspace.sources?.some((source) => source.title === otherFixture.title)).toBe(false)
  expectPayloadIsolated(workspace, otherFixture)

  const search = await page.request.get(
    `/api/v1/navbar/search?q=${encodeURIComponent(otherFixture.graphMarker)}&scope=knowledge`,
    { headers: { 'x-velion-org-id': authority.users[1].orgId } },
  )
  expect(search.status()).toBe(200)
  const payload = await search.json() as { data?: { results?: Array<{ label?: string; excerpt?: string }> }; results?: Array<{ label?: string; excerpt?: string }> }
  const results = payload.data?.results ?? payload.results ?? []
  expect(results.some((result) =>
    result.label?.includes(otherFixture.title) || result.excerpt?.includes(otherFixture.graphMarker),
  )).toBe(false)
  expectPayloadIsolated(payload, otherFixture)
})

async function authenticate(page: Page, baseURL: string | undefined, user: AuthorityUser) {
  const origin = baseURL ?? 'http://localhost'
  const signIn = await page.request.post('/api/v1/auth/sign-in', {
    headers: { 'content-type': 'application/json', origin },
    data: { email: user.email, password: user.password },
  })
  expect(signIn.status(), 'supported gateway sign-in').toBe(200)

  const switchOrg = await page.request.post('/api/v1/orgs/switch-active', {
    headers: { 'content-type': 'application/json', origin },
    data: { organizationId: user.orgId },
  })
  expect(switchOrg.status(), 'supported active organization switch').toBe(200)

  const me = await page.request.get('/api/v1/me')
  expect(me.status(), 'real gateway session').toBe(200)
  const session = await page.request.get('/api/v1/me/session-context')
  expect(session.status(), 'real User Core session context').toBe(200)
  const body = await session.json() as { data?: { orgId?: string; userId?: string }; orgId?: string; userId?: string }
  const context = body.data ?? body
  expect(context.userId).toBe(user.userId)
  expect(context.orgId).toBe(user.orgId)
}

function readFixture<T>(name: string): T {
  const path = process.env[name]
  if (!path) throw new Error(`${name} is required`)
  return JSON.parse(readFileSync(path, 'utf8')) as T
}

function escapeRegex(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function expectPayloadIsolated(payload: unknown, forbidden: BrowserFixture) {
  const serialized = JSON.stringify(payload)
  for (const value of Object.values(forbidden)) {
    expect(serialized).not.toContain(value)
  }
}
