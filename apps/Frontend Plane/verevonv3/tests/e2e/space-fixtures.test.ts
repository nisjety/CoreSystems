// Unit tests for the Space e2e fixtures (slice M0-T0). Pure logic, no browser, no network:
//   node --test "tests/e2e/space-fixtures.test.ts"   (Node >= 22.18 strips the types)
// The file is named .test.ts on purpose: Playwright only collects *.spec.ts and Vitest
// excludes tests/e2e/**, so neither runner picks it up by accident.
import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it } from 'node:test'
import {
  isVerevonRoutedModel,
  requireMembers,
  requireSpace,
  requireSpaceRef,
  skipUnlessSpace,
  spaceRef,
  spaceTestModel,
  storageStateFor,
} from './space-fixtures.ts'

const ENV_KEYS = [
  'E2E_REQUIRE_SPACE',
  'E2E_SPACE_REF',
  'E2E_SPACE_MODEL',
  'E2E_STORAGE_STATE_A',
  'E2E_STORAGE_STATE_B',
]

/** A recording stand-in for Playwright's `test`. */
function fakeTest() {
  const skips: Array<{ condition: boolean; description?: string }> = []
  return { skips, skip: (condition: boolean, description?: string) => void skips.push({ condition, description }) }
}

describe('space-fixtures', () => {
  const saved: Record<string, string | undefined> = {}
  beforeEach(() => {
    for (const key of ENV_KEYS) {
      saved[key] = process.env[key]
      delete process.env[key]
    }
  })
  afterEach(() => {
    for (const key of ENV_KEYS) {
      if (saved[key] === undefined) delete process.env[key]
      else process.env[key] = saved[key]
    }
  })

  describe('skipUnlessSpace', () => {
    it('does nothing when the fixture is available', () => {
      const t = fakeTest()
      skipUnlessSpace(t, false, 'unused')
      assert.deepEqual(t.skips, [])
    })

    it('skips (does not throw) when the fixture is missing and E2E_REQUIRE_SPACE is unset', () => {
      const t = fakeTest()
      skipUnlessSpace(t, true, 'no space')
      assert.deepEqual(t.skips, [{ condition: true, description: 'no space' }])
    })

    it('skips when E2E_REQUIRE_SPACE is anything other than "1"', () => {
      for (const value of ['0', 'true', '', ' 1']) {
        process.env.E2E_REQUIRE_SPACE = value
        const t = fakeTest()
        skipUnlessSpace(t, true, 'no space')
        assert.equal(t.skips.length, 1, `value ${JSON.stringify(value)}`)
      }
    })

    it('throws instead of skipping under E2E_REQUIRE_SPACE=1, and names the missing fixture', () => {
      process.env.E2E_REQUIRE_SPACE = '1'
      assert.equal(requireSpace(), true)
      const t = fakeTest()
      assert.throws(() => skipUnlessSpace(t, true, 'E2E_SPACE_REF missing'), /E2E_REQUIRE_SPACE=1.*E2E_SPACE_REF missing/)
      assert.deepEqual(t.skips, [], 'a gate run must never register a skip')
    })

    it('does not throw under E2E_REQUIRE_SPACE=1 when the fixture is available', () => {
      process.env.E2E_REQUIRE_SPACE = '1'
      assert.doesNotThrow(() => skipUnlessSpace(fakeTest(), false, 'unused'))
    })
  })

  describe('requireSpaceRef', () => {
    it('skips with a reason when E2E_SPACE_REF is unset or blank', () => {
      for (const value of [undefined, '', '   ']) {
        if (value === undefined) delete process.env.E2E_SPACE_REF
        else process.env.E2E_SPACE_REF = value
        const t = fakeTest()
        assert.equal(requireSpaceRef(t), '')
        assert.equal(t.skips.length, 1)
      }
    })

    it('returns the trimmed ref when set', () => {
      process.env.E2E_SPACE_REF = '  space-123  '
      const t = fakeTest()
      assert.equal(spaceRef(), 'space-123')
      assert.equal(requireSpaceRef(t), 'space-123')
      assert.deepEqual(t.skips, [])
    })

    it('fails under E2E_REQUIRE_SPACE=1 when the ref is missing', () => {
      process.env.E2E_REQUIRE_SPACE = '1'
      assert.throws(() => requireSpaceRef(fakeTest()), /E2E_SPACE_REF/)
    })
  })

  describe('storage states', () => {
    it('storageStateFor is undefined when the env var is unset or the file does not exist', () => {
      assert.equal(storageStateFor('A'), undefined)
      process.env.E2E_STORAGE_STATE_A = 'Z:/definitely/not/here/state-A.json'
      assert.equal(storageStateFor('A'), undefined)
    })

    it('storageStateFor returns the path of an existing file', () => {
      process.env.E2E_STORAGE_STATE_A = import.meta.filename
      assert.equal(storageStateFor('A'), import.meta.filename)
    })

    it('requireMembers lists every missing storage-state variable and fails at a gate', () => {
      process.env.E2E_STORAGE_STATE_A = import.meta.filename
      const t = fakeTest()
      requireMembers(t, 'A', 'B')
      assert.equal(t.skips.length, 1)
      assert.match(t.skips[0]?.description ?? '', /E2E_STORAGE_STATE_B/)
      assert.doesNotMatch(t.skips[0]?.description ?? '', /E2E_STORAGE_STATE_A/)

      process.env.E2E_REQUIRE_SPACE = '1'
      assert.throws(() => requireMembers(fakeTest(), 'A', 'B'), /E2E_STORAGE_STATE_B/)
    })

    it('requireMembers returns the paths when every member is present', () => {
      process.env.E2E_STORAGE_STATE_A = import.meta.filename
      process.env.E2E_STORAGE_STATE_B = import.meta.filename
      const t = fakeTest()
      const states = requireMembers(t, 'A', 'B')
      assert.equal(states.A, import.meta.filename)
      assert.equal(states.B, import.meta.filename)
      assert.deepEqual(t.skips, [])
    })
  })

  describe('spaceTestModel (owner order: never a Verevon-routed model)', () => {
    const verevonRouted = [
      'verevon',
      'verevon-budget',
      'verevon-genius',
      'verevon-balance',
      'verevon-auto',
      'auto',
      'default',
      'Verevon',
      'VEREVON-BUDGET',
      '  verevon-genius  ',
      'verevon/budget',
      'verevon:genius',
      'verevon_budget',
      'Auto',
      'DEFAULT',
    ]
    for (const model of verevonRouted) {
      it(`rejects ${JSON.stringify(model)}`, () => {
        process.env.E2E_SPACE_MODEL = model
        assert.equal(isVerevonRoutedModel(model), true)
        assert.throws(() => spaceTestModel(fakeTest()), /Verevon-routed/)
      })
    }

    it('rejects a Verevon-routed model even when E2E_REQUIRE_SPACE is unset (never a skip)', () => {
      process.env.E2E_SPACE_MODEL = 'verevon-budget'
      const t = fakeTest()
      assert.throws(() => spaceTestModel(t), /non-Verevon/)
      assert.deepEqual(t.skips, [])
    })

    it('accepts explicit non-Verevon models', () => {
      for (const model of ['gpt-4o-mini', 'claude-sonnet-4', 'anthropic/claude-haiku', 'gemini-2.0-flash', 'automation-model', 'defaults-x', 'my-verevon']) {
        assert.equal(isVerevonRoutedModel(model), false, model)
        process.env.E2E_SPACE_MODEL = model
        const t = fakeTest()
        assert.equal(spaceTestModel(t), model)
        assert.deepEqual(t.skips, [])
      }
    })

    it('skips when unset and fails at a gate', () => {
      const t = fakeTest()
      assert.equal(spaceTestModel(t), '')
      assert.equal(t.skips.length, 1)
      process.env.E2E_REQUIRE_SPACE = '1'
      assert.throws(() => spaceTestModel(fakeTest()), /E2E_SPACE_MODEL/)
    })
  })
})
