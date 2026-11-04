// EP-11 S4: the editor reads capability-core's skill lint findings, and only those.

import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import { findingsFor, skillLintMessage, skillLintRefusal, skillLintWarnings } from './skill-lint'

const tr = (no: string) => no

describe('skill lint findings', () => {
  it('reads a refusal only from a skill_lint_failed error', () => {
    const findings = [{ code: 'name_too_long', field: 'name', severity: 'error' }]
    expect(skillLintRefusal(new ApiError('x', 422, 'skill_lint_failed', { details: { findings } }))).toEqual(findings)
    expect(skillLintRefusal(new ApiError('x', 422, 'invalid_request', { details: { findings } }))).toEqual([])
    expect(skillLintRefusal(new Error('x'))).toEqual([])
  })

  it('drops entries that are not findings', () => {
    const body = {
      id: 'skill_1',
      warnings: [
        { code: 'transcript_shape', field: 'content', severity: 'warn' },
        { code: 'x', field: 'content', severity: 'fatal' },
        'content_empty',
        null,
      ],
    }
    expect(skillLintWarnings(body)).toEqual([{ code: 'transcript_shape', field: 'content', severity: 'warn' }])
    expect(skillLintWarnings({ id: 'skill_1' })).toEqual([])
  })

  it('groups findings by field and words every code', () => {
    const findings = skillLintWarnings({
      warnings: [
        { code: 'description_missing', field: 'description', severity: 'warn' },
        { code: 'content_exceeds_budget', field: 'content', severity: 'warn' },
      ],
    })
    expect(findingsFor(findings, 'content').map((finding) => finding.code)).toEqual(['content_exceeds_budget'])
    for (const code of [
      'name_empty',
      'name_too_long',
      'name_charset',
      'description_missing',
      'description_too_long',
      'content_empty',
      'content_too_long',
      'content_exceeds_budget',
      'transcript_shape',
      'trigger_keyword_count',
      'trigger_keyword_too_long',
      'tool_restriction_invalid',
      'frontmatter_name_mismatch',
    ]) {
      expect(skillLintMessage(code, tr)).not.toBe(skillLintMessage('unknown_rule', tr))
    }
  })
})
