import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

test('canonical frontend build includes the local Quarry SDK without the entire Ingestion source', () => {
  const compose = readFileSync(new URL('../docker-compose.yml', import.meta.url), 'utf8')
  const dockerfile = readFileSync(new URL('../Dockerfile', import.meta.url), 'utf8')
  assert.match(compose, /quarry-sdk:\s*\.\.\/\.\.\/Ingestion Plane\/Quarry-v2\/sdks\/typescript/)
  assert.match(dockerfile, /COPY --from=quarry-sdk/)
  assert.match(dockerfile, /tsc --ignoreDeprecations 6\.0/)
  assert.match(dockerfile, /COREPACK_ENABLE_PROJECT_SPEC=0/)
  assert.doesNotMatch(dockerfile, /--no-frozen-lockfile/)
})
