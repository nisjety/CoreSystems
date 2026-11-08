import { describe, expect, it } from 'vitest'
import { isGeneratedFile, normalizeGeneratedFile, normalizeGeneratedFileCheck } from './chat-normalizers'
import type { GeneratedFile } from './chat-types'

// The gateway attaches the structural check of a generated Office or PDF file to
// the `attachment` frame. The SPA keeps the verdict and its findings, and reads
// no level the gateway did not send. The check only reads a file's structure, so
// nothing the SPA shows for it may say the file was verified in Office.

const DOCX = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'

describe('normalizeGeneratedFile: the check on the attachment frame', () => {
  it('keeps a well-formed check with its level and findings', () => {
    const file = normalizeGeneratedFile({
      id: 'call-1-file-1',
      name: 'deck.docx',
      mime: DOCX,
      size: 4096,
      url: 'data:application/octet-stream;base64,AA',
      check: {
        name: 'deck.docx',
        kind: 'docx',
        level: 'failed',
        checks: [{ id: 'container', level: 'failed', message: 'the file is not a readable zip package' }],
        stats: {},
      },
    })
    expect(file?.check).toEqual({
      level: 'failed',
      checks: [{ id: 'container', level: 'failed', message: 'the file is not a readable zip package' }],
    })
  })

  it('carries no check key when the gateway sent none, so an older frame reads as it always did', () => {
    const file = normalizeGeneratedFile({ name: 'deck.docx', mime: DOCX, size: 1, url: 'data:x' })
    expect(file).not.toBeNull()
    expect(file).not.toHaveProperty('check')
  })

  it('drops a check whose level the SPA does not read and keeps the file itself', () => {
    const file = normalizeGeneratedFile({
      name: 'deck.docx',
      mime: DOCX,
      url: 'data:x',
      check: { level: 'verified', checks: [] },
    })
    expect(file).not.toBeNull()
    expect(file).not.toHaveProperty('check')
  })

  it('drops only the findings that are malformed', () => {
    expect(
      normalizeGeneratedFileCheck({
        level: 'warnings',
        checks: [
          { id: 'rels', level: 'warnings', message: 'a relationship points nowhere' },
          { id: 'bad', level: 'great', message: 'an unknown level' },
          { id: 'no-message', level: 'failed' },
          'not an object',
        ],
      }),
    ).toEqual({
      level: 'warnings',
      checks: [{ id: 'rels', level: 'warnings', message: 'a relationship points nowhere' }],
    })
  })

  it('reads no check from a value that is not a check', () => {
    expect(normalizeGeneratedFileCheck(undefined)).toBeUndefined()
    expect(normalizeGeneratedFileCheck(null)).toBeUndefined()
    expect(normalizeGeneratedFileCheck('failed')).toBeUndefined()
    expect(normalizeGeneratedFileCheck({ level: 'passed' })).toBeUndefined()
  })
})

describe('isGeneratedFile: a persisted file with or without its check', () => {
  const base = { id: 'call-1-file-1', name: 'deck.docx', mime: DOCX, size: 4096, url: 'data:x' }

  it('accepts a persisted file that carries a check, so a reloaded thread keeps its chip', () => {
    const persisted: GeneratedFile = { ...base, check: { level: 'passed', checks: [] } }
    expect(isGeneratedFile(persisted)).toBe(true)
  })

  it('accepts a persisted file from before the check existed', () => {
    expect(isGeneratedFile(base)).toBe(true)
  })
})
