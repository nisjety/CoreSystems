import { expect, test } from '@playwright/test'

/**
 * Installable web app shell and Web Push worker (WS16 S6), checked on the served production build. These checks read
 * the files the nginx build serves and the caches the page can see, so they hold whether or not the person is signed
 * in. The worker must revalidate on every load, must not be cached as immutable, and the origin must hold no cache
 * the worker filled.
 */

test('the app shell links the web app manifest and the theme colour', async ({ page }) => {
  await page.goto('/')

  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute('href', '/manifest.webmanifest')
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute('content', '#171717')
})

test('the manifest is served as an application manifest and names the app', async ({ request }) => {
  const response = await request.get('/manifest.webmanifest')

  expect(response.status()).toBe(200)
  expect(response.headers()['content-type']).toContain('application/manifest+json')
  const manifest = await response.json()
  expect(manifest.name).toBe('Verevon')
  expect(manifest.display).toBe('standalone')
  expect(manifest.icons[0].src).toBe('/icons/verevon-mark.svg')
})

test('the push worker is revalidated on every load and never cached as immutable', async ({ request }) => {
  const response = await request.get('/sw.js')

  expect(response.status()).toBe(200)
  expect(response.headers()['content-type']).toContain('javascript')
  expect(response.headers()['cache-control']).toBe('no-cache')
  expect(response.headers()['service-worker-allowed']).toBe('/')
})

test('the content security policy pins workers and manifests to this origin', async ({ request }) => {
  const response = await request.get('/')
  const policy = response.headers()['content-security-policy'] ?? ''

  expect(policy).toContain("worker-src 'self'")
  expect(policy).toContain("manifest-src 'self'")
})

test('the origin holds no cache storage: the worker never caches a page, a response or a conversation', async ({ page }) => {
  await page.goto('/')

  const cacheNames = await page.evaluate(async () => (typeof caches === 'undefined' ? [] : caches.keys()))
  expect(cacheNames).toEqual([])
})
