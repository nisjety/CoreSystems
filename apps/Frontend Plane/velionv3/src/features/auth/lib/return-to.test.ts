import { describe, expect, it } from 'vitest'
import { safeReturnTo } from './return-to'

describe('safeReturnTo', () => {
  it('accepts an internal invitation path with its query and fragment', () => {
    expect(safeReturnTo('/accept-invitation/inv_123?source=email#confirm')).toBe(
      '/accept-invitation/inv_123?source=email#confirm',
    )
  })

  it.each([
    'https://attacker.example/invite',
    '//attacker.example/invite',
    '/\\attacker.example/invite',
    'javascript:alert(1)',
    '',
  ])('rejects unsafe return target %s', (target) => {
    expect(safeReturnTo(target)).toBeNull()
  })
})
