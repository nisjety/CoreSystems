import { describe, expect, it } from 'vitest'
import { feedbackReasonLabels } from './feedback-reasons'

describe('feedbackReasonLabels', () => {
  it('offers the same four reasons, in the same order, in each language', () => {
    expect(feedbackReasonLabels({ tr: (noText) => noText })).toEqual([
      'Unøyaktig',
      'Ikke relevant',
      'Ufullstendig',
      'Skadelig',
    ])
    expect(feedbackReasonLabels({ tr: (_noText, enText) => enText })).toEqual([
      'Inaccurate',
      'Not relevant',
      'Incomplete',
      'Harmful',
    ])
  })
})
