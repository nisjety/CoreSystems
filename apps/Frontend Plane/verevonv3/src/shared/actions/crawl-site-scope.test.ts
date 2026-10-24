import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

// WS14.S6: the Model-visible scope subset of knowledge.crawl_site. The schema must never
// name an external-links or robots key (the gateway drops them too; see quarry.rs Surface::Model).
// The check reads the schema block of action-registry.ts, so a future key is caught here.

const here = dirname(fileURLToPath(import.meta.url))
const registrySource = readFileSync(resolve(here, 'action-registry.ts'), 'utf8')

function crawlSiteSchemaCode(source: string): string {
  const start = source.indexOf('const crawlSiteInput = z.object({')
  expect(start).toBeGreaterThan(-1)
  const end = source.indexOf('})', start)
  expect(end).toBeGreaterThan(start)
  // Comment lines are documentation, not schema keys: they are not checked.
  return source
    .slice(start, end + 2)
    .split('\n')
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n')
}

describe('knowledge.crawl_site Model-visible scope', () => {
  const code = crawlSiteSchemaCode(registrySource)

  it('has no key that mentions robots or external links', () => {
    const keys = [...code.matchAll(/^\s{2}(\w+):/gm)].map((match) => match[1])
    expect(keys.length).toBeGreaterThan(0)
    for (const key of keys) {
      expect(key.toLowerCase()).not.toMatch(/robot|external/)
    }
    expect(code.toLowerCase()).not.toMatch(/robots|external/)
  })

  it('exposes the scope fields the plan allows the Model to set', () => {
    for (const key of ['maxDepth', 'includePaths', 'excludePaths', 'allowSubdomains']) {
      expect(code).toContain(`${key}:`)
    }
  })

  it('caps the patterns the same way the edge does', () => {
    expect(code).toContain('.max(512)')
    expect(code).toContain('.max(100)')
    expect(code).toContain('.max(10)')
  })
})
