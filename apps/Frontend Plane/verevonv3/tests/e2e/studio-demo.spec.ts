import { expect, test } from '@playwright/test'

test('keeps Studio editing local, unauthenticated, and backend-free', async ({ page, baseURL }) => {
  const demoOrigin = new URL(baseURL ?? 'http://127.0.0.1:5174').origin
  const apiRequests: string[] = []
  const externalRequests: string[] = []

  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.origin !== demoOrigin) externalRequests.push(request.url())
    if (url.pathname === '/api' || url.pathname.startsWith('/api/')) apiRequests.push(request.url())
  })

  await page.goto('/')

  await expect(page).toHaveTitle('Verevon Studio Demo')
  await expect(page).toHaveURL(/\/studio\/canvas$/)
  await expect(page.locator('.core-navbar')).toBeVisible()
  await expect(page.getByLabel(/primærnavigasjon|primary navigation/i)).toBeVisible()
  await expect(page.getByText(/Lokal Studio-demo · ingen konto eller API/)).toBeVisible()
  await expect(page.getByRole('button', { name: 'Lagre' })).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Send til utkast' })).toBeDisabled()

  await page.getByRole('button', { name: 'Tekst', exact: true }).click()
  await expect(page.getByRole('textbox', { name: 'Tittel' })).toHaveValue('Text note 1')
  expect(apiRequests).toEqual([])
  expect(externalRequests).toEqual([])

  const apiResponse = await page.goto('/api/v1/studio/projects')
  expect(apiResponse?.status()).toBe(404)
})

test('uses the current shell navigation while retaining the Studio-only local boundary', async ({ page, baseURL }) => {
  const demoOrigin = new URL(baseURL ?? 'http://127.0.0.1:5174').origin
  const apiRequests: string[] = []

  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.origin === demoOrigin && (url.pathname === '/api' || url.pathname.startsWith('/api/'))) {
      apiRequests.push(request.url())
    }
  })

  await page.goto('/studio/canvas')
  await expect(page.locator('.core-navbar')).toBeVisible()
  await expect(page.getByRole('button', { name: /kunnskapssøk|knowledge search/i })).toBeDisabled()

  await page.getByRole('link', { name: /kampanjeplanlegger|campaign planner/i }).click()
  await expect(page).toHaveURL(/\/studio\/campaigns$/)
  await expect(page.getByRole('heading', { name: /kampanjeplanlegger|campaign planner/i })).toBeVisible()
  expect(apiRequests).toEqual([])
})

test('shows a read-only Marketing Pulse without inventing unavailable analytics', async ({ page, baseURL }) => {
  const demoOrigin = new URL(baseURL ?? 'http://127.0.0.1:5174').origin
  const apiRequests: string[] = []
  const externalRequests: string[] = []

  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.origin !== demoOrigin) externalRequests.push(request.url())
    if (url.origin === demoOrigin && (url.pathname === '/api' || url.pathname.startsWith('/api/'))) {
      apiRequests.push(request.url())
    }
  })

  await page.goto('/studio/canvas')
  const pulseResponse = page.waitForResponse((response) => new URL(response.url()).pathname === '/api/v1/marketing/pulse')
  await page.getByRole('link', { name: /markedsføring|marketing/i }).click()
  const apiResponse = await pulseResponse

  await expect(page).toHaveURL(/\/studio\/pulse$/)
  await expect(page.getByRole('heading', { name: /pulse|puls/i })).toBeVisible()
  if (apiResponse.ok()) {
    const payload = await apiResponse.json() as { data?: { status?: string; totals?: { clicks?: number } | null } }
    const clicks = payload.data?.status === 'connected' ? payload.data.totals?.clicks : null
    await expect(page.getByTestId('pulse-primary-value')).toHaveText(
      typeof clicks === 'number' ? new Intl.NumberFormat('nb-NO').format(clicks) : '—',
    )
  } else {
    await expect(page.getByTestId('pulse-primary-value')).toHaveText('—')
  }
  await expect(page.getByTestId('pulse-activity-chart')).toBeVisible()
  await expect(page.getByTestId('pulse-timeline-scrubber')).toBeVisible()
  await expect(page.getByTestId('pulse-project-momentum-card')).toBeVisible()
  await expect(page.getByTestId('pulse-summary-card')).toHaveCount(3)
  await expect(page.getByTestId('pulse-momentum-value')).toHaveText('—')
  await expect(page.getByTestId('pulse-backlog-count')).toHaveText('0')
  await expect(page.getByTestId('pulse-workspace-view-switch')).toBeVisible()
  await expect(page.getByTestId('pulse-backlog-empty-row')).toBeVisible()
  await expect(page.getByTestId('pulse-log-count')).toHaveText('0')
  await expect(page.locator('.marketing-pulse__breakdown')).toBeVisible()
  await expect(page.locator('.marketing-pulse__breakdown .marketing-pulse__ledger')).toHaveCount(2)
  // The isolated design demo returns 404; the opt-in local bridge shows the
  // backend status. Neither state may invent metrics or ask for a local code.
  await expect(page.getByLabel(/local access code|lokal tilgangskode/i)).toHaveCount(0)
  expect(apiRequests).toHaveLength(1)
  expect(new URL(apiRequests[0] ?? '').pathname).toBe('/api/v1/marketing/pulse')
  expect(externalRequests).toEqual([])
})

test('keeps the complete Pulse work surface reachable below the fixed shell chrome', async ({ page }) => {
  await page.goto('/studio/pulse')

  const pulseSurface = page.getByTestId('marketing-pulse')
  await expect(pulseSurface).toBeVisible()

  const scrollRange = await pulseSurface.evaluate((element) => element.scrollHeight - element.clientHeight)
  expect(scrollRange).toBeGreaterThan(0)

  await pulseSurface.evaluate((element) => { element.scrollTop = element.scrollHeight })
  await expect.poll(() => pulseSurface.evaluate((element) => element.scrollTop)).toBeGreaterThan(0)
  await expect(page.getByText('Meta Ads')).toBeInViewport()
})

test('adds and changes a website without borrowing another site’s metrics', async ({ page, baseURL }) => {
  const demoOrigin = new URL(baseURL ?? 'http://127.0.0.1:5174').origin
  const externalRequests: string[] = []
  page.on('request', (request) => {
    if (new URL(request.url()).origin !== demoOrigin) externalRequests.push(request.url())
  })

  await page.goto('/studio/pulse')
  await page.getByRole('button', { name: /nettsider|websites/i }).click()
  await expect(page.getByLabel(/nettadresse|website url/i)).toBeVisible()
  await page.getByLabel(/nettadresse|website url/i).fill('example.com')
  await page.getByRole('button', { name: /legg til og hent data|add and fetch data/i }).click()
  await expect(page.locator('.marketing-pulse__masthead strong')).toHaveText('example.com')
  await expect(page.getByTestId('pulse-primary-value')).toHaveText('—')
  await page.reload()
  await expect(page.locator('.marketing-pulse__masthead strong')).toHaveText('example.com')
  expect(externalRequests).toEqual([])
})
