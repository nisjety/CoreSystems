#!/usr/bin/env node
/**
 * Captures the two stills the site shows while the interactive demo loads, fails to load, or sits
 * on a phone card: poster-desktop.webp (1240 x 696 CSS px at 2x) and poster-phone.webp (390 x 844
 * at 2x). They are screenshots of the built demo's dashboard at rest, so regenerate them whenever the
 * demo's first screen changes, then run `pnpm demo:build` again (the posters are build inputs).
 *
 *   node scripts/capture-demo-poster.mjs [--url http://127.0.0.1:5311/product-demo/]
 *        [--out src/demo/product-demo/public]
 *
 * Serve the published demo folder first, for example with any static server over apps/verevon-web/public.
 * Needs Playwright's Chromium and ffmpeg on the PATH (the same tools the product films use).
 */
import { chromium } from '@playwright/test'
import { spawnSync } from 'node:child_process'
import { mkdir, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const spaRoot = path.resolve(here, '..')
const args = new Map()
for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index].replace(/^--/, ''), process.argv[index + 1])
const demoUrl = args.get('url') ?? 'http://127.0.0.1:5311/product-demo/'
const outDir = path.resolve(args.get('out') ?? path.join(spaRoot, 'src/demo/product-demo/public'))

const POSTERS = [
  { file: 'poster-desktop.webp', width: 1240, height: 696 },
  { file: 'poster-phone.webp', width: 390, height: 844 },
]

await mkdir(outDir, { recursive: true })
const scratch = path.join(os.tmpdir(), 'verevon-demo-poster')
await mkdir(scratch, { recursive: true })
const browser = await chromium.launch({ headless: true })
try {
  for (const poster of POSTERS) {
    const context = await browser.newContext({
      viewport: { width: poster.width, height: poster.height },
      deviceScaleFactor: 2,
      locale: 'nb-NO',
      reducedMotion: 'reduce',
      colorScheme: 'light',
    })
    const page = await context.newPage()
    await page.goto(demoUrl, { waitUntil: 'domcontentloaded' })
    await page.waitForSelector('text=God ', { timeout: 30000 })
    await page.waitForLoadState('networkidle')
    await page.waitForTimeout(1800)
    const png = path.join(scratch, poster.file.replace(/\.webp$/, '.png'))
    await page.screenshot({ path: png })
    await context.close()
    const target = path.join(outDir, poster.file)
    const result = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', png, '-c:v', 'libwebp', '-quality', '84', '-compression_level', '6', target], { stdio: 'inherit' })
    if (result.status !== 0) throw new Error(`ffmpeg failed for ${poster.file}`)
    console.log(`demo:poster: ${target}`)
  }
} finally {
  await browser.close()
  await rm(scratch, { recursive: true, force: true })
}
