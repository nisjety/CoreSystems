#!/usr/bin/env node
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { gzipSync } from 'node:zlib'
import path from 'node:path'

const root = process.cwd()
const routes = ['/chat', '/inbox', '/knowledge']
const budgets = new Map([
  ['/chat', 295 * 1024],
  ['/inbox', 284 * 1024],
  ['/knowledge', 271 * 1024],
])
const embedBudgetBytes = 8 * 1024

function walk(dir) {
  if (!existsSync(dir)) return []
  return readdirSync(dir).flatMap((name) => {
    const full = path.join(dir, name)
    const stat = statSync(full)
    return stat.isDirectory() ? walk(full) : [full]
  })
}

function manifestForRoute(route) {
  const appDir = path.join(root, '.next/server/app')
  const target = route === '/' ? 'page_client-reference-manifest.js' : `${route.replace(/^\//, '')}/page_client-reference-manifest.js`
  const file = path.join(appDir, target)
  return existsSync(file) ? file : null
}

let failed = false
for (const route of routes) {
  const manifest = manifestForRoute(route)
  if (!manifest) {
    console.warn(`perf: missing manifest for ${route}`)
    continue
  }

  const gzBytes = gzipSync(readFileSync(manifest)).byteLength
  const budget = budgets.get(route) ?? Number.POSITIVE_INFINITY
  const label = `${Math.round(gzBytes / 1024)} KB gzip`
  if (gzBytes > budget) {
    failed = true
    console.error(`perf: ${route} exceeds audit baseline budget: ${label} > ${Math.round(budget / 1024)} KB`)
  } else {
    console.log(`perf: ${route} ${label}`)
  }
}

const embedPath = path.join(root, 'public/embed.js')
if (existsSync(embedPath)) {
  const gzBytes = gzipSync(readFileSync(embedPath)).byteLength
  if (gzBytes > embedBudgetBytes) {
    failed = true
    console.error(`perf: embed.js exceeds budget: ${gzBytes} B gzip > ${embedBudgetBytes} B`)
  } else {
    console.log(`perf: embed.js ${gzBytes} B gzip`)
  }
}

const staticDir = path.join(root, '.next/static')
const staticBytes = walk(staticDir).reduce((sum, file) => sum + statSync(file).size, 0)
console.log(`perf: .next/static ${Math.round(staticBytes / 1024 / 1024)} MB raw`)

process.exit(failed ? 1 : 0)
