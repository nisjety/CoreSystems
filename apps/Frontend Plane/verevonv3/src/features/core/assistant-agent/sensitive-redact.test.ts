// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { redactSensitive, REDACTION_PLACEHOLDER } from './sensitive-redact'

describe('redactSensitive', () => {
  it('masks bearer tokens, API keys and passwords', () => {
    expect(redactSensitive('Bearer abcdef1234567890')).toContain(REDACTION_PLACEHOLDER)
    expect(redactSensitive('key sk-proj-abcdefghijklmnop here')).toContain(REDACTION_PLACEHOLDER)
    expect(redactSensitive('password=hunter2')).toBe(REDACTION_PLACEHOLDER)
    expect(redactSensitive('jane@example.com')).toBe(REDACTION_PLACEHOLDER)
  })

  it('leaves ordinary task text alone', () => {
    expect(redactSensitive('open the knowledge page')).toBe('open the knowledge page')
  })
})
