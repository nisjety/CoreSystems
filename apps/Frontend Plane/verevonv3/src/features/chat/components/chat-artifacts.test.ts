import { describe, expect, it } from 'vitest'
import {
  artifactDownloadName,
  artifactFileMeta,
  artifactLanguage,
  artifactRenderKind,
  artifactRenderKindLabel,
  artifactVersionAt,
  artifactVersions,
  dataUriByteSize,
  findArtifactById,
  friendlyMimeLabel,
  latestArtifactVersion,
  mergeArtifactVersion,
  mimeFileExtension,
  parseDataUri,
} from './chat-artifacts'
import { normalizeArtifact, upsertArtifact } from './chat-normalizers'
import { highlightCode } from './chat-code-highlight'
import type { ChatArtifact } from './chat-types'

function artifact(overrides: Partial<ChatArtifact> = {}): ChatArtifact {
  return {
    content: 'print("hei")',
    id: 'artifact-1',
    kind: 'code',
    title: 'solve.py',
    version: 1,
    ...overrides,
  }
}

/**
 * `noUncheckedIndexedAccess` is on, and every upsert assertion below is about a
 * list that must hold exactly one artifact — assert that, then work with a
 * defined value.
 */
function onlyArtifact(list: ChatArtifact[]): ChatArtifact {
  expect(list).toHaveLength(1)
  const [first] = list
  if (!first) throw new Error('expected exactly one artifact')
  return first
}

function base64(value: string): string {
  return btoa(value)
}

const XLSX_MIME = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'
const DOCX_MIME = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'

describe('upsertArtifact version history', () => {
  it('retains incoming snapshot history, including when its current revision is omitted', () => {
    const snapshot = artifact({ content: 'third', version: 3, history: [
      { content: 'first', title: 'first.py', version: 1 },
      { content: 'second', title: 'second.py', version: 2 },
    ] })
    expect(artifactVersions(mergeArtifactVersion(undefined, snapshot)).map(entry => entry.content))
      .toEqual(['first', 'second', 'third'])
    const merged = mergeArtifactVersion(artifact({ content: 'fourth', version: 4 }), snapshot)
    expect(artifactVersions(merged).map(entry => entry.content)).toEqual(['first', 'second', 'third', 'fourth'])
    expect(merged.version).toBe(4)
  })

  it('appends a version for a repeated id instead of duplicating the entry', () => {
    let list = upsertArtifact([], artifact({ content: 'v1', version: 1 }))
    list = upsertArtifact(list, artifact({ content: 'v2', version: 2 }))
    list = upsertArtifact(list, artifact({ content: 'v3', version: 3 }))

    const merged = onlyArtifact(list)
    expect(artifactVersions(merged).map((entry) => entry.version)).toEqual([1, 2, 3])
    // The top-level fields always mirror the newest revision so existing
    // readers (message chips, inline previews) need no change.
    expect(merged.content).toBe('v3')
    expect(merged.version).toBe(3)
  })

  it('keeps a different id as its own list entry', () => {
    let list = upsertArtifact([], artifact({ id: 'a', version: 1 }))
    list = upsertArtifact(list, artifact({ id: 'b', version: 1 }))
    expect(list.map((item) => item.id)).toEqual(['a', 'b'])
  })

  it('replaces a re-emitted revision rather than recording it twice', () => {
    let list = upsertArtifact([], artifact({ content: 'partial', version: 2 }))
    list = upsertArtifact(list, artifact({ content: 'complete', version: 2 }))

    const merged = onlyArtifact(list)
    expect(artifactVersions(merged)).toHaveLength(1)
    expect(merged.content).toBe('complete')
  })

  it('records an out-of-order older revision without regressing the current one', () => {
    let list = upsertArtifact([], artifact({ content: 'v3', version: 3 }))
    list = upsertArtifact(list, artifact({ content: 'v2', version: 2 }))

    const merged = onlyArtifact(list)
    expect(artifactVersions(merged).map((entry) => entry.version)).toEqual([2, 3])
    expect(merged.content).toBe('v3')
    expect(merged.version).toBe(3)
  })

  it('carries an earlier turn\'s revisions when the model rewrites it later', () => {
    // The same artifact id can reappear several turns later; the earlier turn's
    // revisions must survive the move onto the new turn.
    const earlier = mergeArtifactVersion(undefined, artifact({ content: 'v1', version: 1 }))
    const list = upsertArtifact([], artifact({ content: 'v2', version: 2 }), earlier)

    expect(artifactVersions(onlyArtifact(list)).map((entry) => entry.content)).toEqual(['v1', 'v2'])
  })
})

describe('version navigation', () => {
  const versioned = (): ChatArtifact => {
    let list = upsertArtifact([], artifact({ content: 'first', title: 'a.py', version: 1 }))
    list = upsertArtifact(list, artifact({ content: 'second', title: 'b.py', version: 2 }))
    list = upsertArtifact(list, artifact({ content: 'third', title: 'c.py', version: 3 }))
    return onlyArtifact(list)
  }

  it('returns the content of each requested version', () => {
    expect(artifactVersionAt(versioned(), 1).content).toBe('first')
    expect(artifactVersionAt(versioned(), 2).content).toBe('second')
    expect(artifactVersionAt(versioned(), 3).content).toBe('third')
    // The title travels with the revision too.
    expect(artifactVersionAt(versioned(), 1).title).toBe('a.py')
  })

  it('defaults to the newest version when no version is selected', () => {
    expect(artifactVersionAt(versioned(), null).content).toBe('third')
    expect(latestArtifactVersion(versioned()).version).toBe(3)
  })

  it('falls back to the newest version for an unknown version number', () => {
    expect(artifactVersionAt(versioned(), 99).content).toBe('third')
  })

  it('treats a history-less artifact (older thread snapshot) as one revision', () => {
    const restored = artifact({ content: 'only', history: undefined, version: 0 })
    expect(artifactVersions(restored)).toEqual([{ content: 'only', title: 'solve.py', version: 0 }])
    expect(artifactVersionAt(restored, null).content).toBe('only')
  })

  it('ignores malformed history entries restored from a snapshot', () => {
    const restored = {
      ...artifact({ content: 'current' }),
      history: [{ nope: true } as unknown as never],
    }
    expect(artifactVersions(restored)).toEqual([{ content: 'current', title: 'solve.py', version: 1 }])
  })
})

describe('artifactRenderKind', () => {
  it('maps the backend kinds to their renderer', () => {
    expect(artifactRenderKind(artifact({ kind: 'code' }))).toBe('code')
    expect(artifactRenderKind(artifact({ kind: 'document', content: '# Tittel' }))).toBe('document')
    expect(artifactRenderKind(artifact({ kind: 'html', content: '<html></html>' }))).toBe('html')
    expect(artifactRenderKind(artifact({ kind: 'spreadsheet', content: `data:${XLSX_MIME};base64,UEs=` }))).toBe('binary')
    expect(artifactRenderKind(artifact({ kind: 'file', content: 'data:application/pdf;base64,JVBER' }))).toBe('binary')
    expect(artifactRenderKind(artifact({ kind: 'image', content: 'data:image/png;base64,iVBOR' }))).toBe('image')
  })

  it('treats a language name used as the kind as code', () => {
    expect(artifactRenderKind(artifact({ kind: 'python' }))).toBe('code')
    expect(artifactRenderKind(artifact({ kind: 'sql', content: 'select 1' }))).toBe('code')
    expect(artifactRenderKind(artifact({ kind: 'json', content: '{}' }))).toBe('code')
  })

  it('keeps the pre-existing markdown kinds on the document renderer', () => {
    for (const kind of ['markdown', 'md', 'doc', 'report', 'prose', 'text']) {
      expect(artifactRenderKind(artifact({ kind, content: 'tekst' }))).toBe('document')
    }
  })

  it('falls back to the content when the kind is unknown', () => {
    expect(artifactRenderKind(artifact({ kind: 'wat', content: 'data:image/png;base64,iVBOR' }))).toBe('image')
    expect(artifactRenderKind(artifact({ kind: 'wat', content: 'data:application/zip;base64,UEs=' }))).toBe('binary')
    expect(artifactRenderKind(artifact({ kind: 'wat', content: 'bare tekst' }))).toBe('text')
    expect(artifactRenderKind(artifact({ kind: '', content: 'https://cdn.test/a.png' }))).toBe('image')
  })
})

describe('artifactLanguage', () => {
  it('reads the language from the filename in the title', () => {
    expect(artifactLanguage(artifact({ title: 'solve.py' }))).toBe('python')
    expect(artifactLanguage(artifact({ title: 'server.ts' }))).toBe('typescript')
    expect(artifactLanguage(artifact({ title: 'query.sql' }))).toBe('sql')
  })

  it('falls back to the kind when the title has no extension', () => {
    expect(artifactLanguage(artifact({ kind: 'rust', title: 'Løsning' }))).toBe('rust')
    expect(artifactLanguage(artifact({ kind: 'rs', title: 'Løsning' }))).toBe('rust')
    expect(artifactLanguage(artifact({ kind: 'code', title: 'Løsning' }))).toBe('')
  })
})

describe('parseDataUri', () => {
  it('extracts the mime type and byte size of a base64 payload', () => {
    // "Hei Verevon" is 11 bytes; its base64 is 16 chars with padding.
    // (The expectation said 10 until this line was corrected: the payload was
    // "Hei Velion" — exactly 10 bytes — and the Velion→Verevon rename grew it
    // by one without updating the count, so the implementation was right and
    // the test was wrong.)
    const uri = `data:text/plain;base64,${base64('Hei Verevon')}`
    const parsed = parseDataUri(uri)
    expect(parsed?.mime).toBe('text/plain')
    expect(parsed?.isBase64).toBe(true)
    expect(parsed?.bytes).toBe(11)
  })

  it('extracts an office mime type with dots and dashes intact', () => {
    const parsed = parseDataUri(`data:${XLSX_MIME};base64,UEsDBBQA`)
    expect(parsed?.mime).toBe(XLSX_MIME)
  })

  it('measures a percent-encoded payload in UTF-8 bytes', () => {
    // "æ" is two bytes in UTF-8.
    const parsed = parseDataUri('data:text/plain;charset=utf-8,%C3%A6')
    expect(parsed?.isBase64).toBe(false)
    expect(parsed?.bytes).toBe(2)
  })

  it('defaults the mime type to text/plain when the header omits it', () => {
    expect(parseDataUri('data:,hei')?.mime).toBe('text/plain')
  })

  it('degrades gracefully on malformed input instead of throwing', () => {
    // No comma: the header/payload separator is missing.
    expect(parseDataUri('data:application/pdf;base64')).toBeNull()
    expect(parseDataUri('https://example.com/a.pdf')).toBeNull()
    expect(parseDataUri('')).toBeNull()
    expect(parseDataUri('data')).toBeNull()
    expect(dataUriByteSize('not a data uri at all')).toBe(0)
    // A truncated percent-escape would throw inside decodeURIComponent.
    expect(() => dataUriByteSize('data:text/plain,%E0%A4%A')).not.toThrow()
    expect(dataUriByteSize('data:text/plain,%E0%A4%A')).toBeGreaterThan(0)
  })
})

describe('friendlyMimeLabel', () => {
  it('labels the office formats the backend generates', () => {
    expect(friendlyMimeLabel(XLSX_MIME)).toBe('Excel-regneark (XLSX)')
    expect(friendlyMimeLabel(DOCX_MIME)).toBe('Word-dokument (DOCX)')
    expect(friendlyMimeLabel('application/pdf')).toBe('PDF-dokument')
  })

  it('ignores parameters and casing', () => {
    expect(friendlyMimeLabel('APPLICATION/PDF; charset=binary')).toBe('PDF-dokument')
  })

  it('derives a label for unmapped media types and reports unknown ones', () => {
    expect(friendlyMimeLabel('image/webp')).toBe('WEBP-bilde')
    expect(friendlyMimeLabel('application/x-tar')).toBe('application/x-tar')
    expect(friendlyMimeLabel(undefined)).toBe('Ukjent filtype')
    expect(friendlyMimeLabel('')).toBe('Ukjent filtype')
  })

  it('maps the same formats to download extensions', () => {
    expect(mimeFileExtension(XLSX_MIME)).toBe('xlsx')
    expect(mimeFileExtension(DOCX_MIME)).toBe('docx')
    expect(mimeFileExtension('application/pdf')).toBe('pdf')
    expect(mimeFileExtension('')).toBeNull()
  })
})

describe('artifactFileMeta', () => {
  it('builds a working download for a generated spreadsheet', () => {
    const sheet = artifact({
      content: `data:${XLSX_MIME};base64,${base64('sheet-bytes')}`,
      kind: 'spreadsheet',
      title: 'Q4-rapport',
    })
    const meta = artifactFileMeta(sheet, 'binary')
    expect(meta?.href).toBe(sheet.content)
    expect(meta?.downloadName).toBe('q4-rapport.xlsx')
    expect(meta?.mimeLabel).toBe('Excel-regneark (XLSX)')
    expect(meta?.bytes).toBe(11)
  })

  it('prefers the paired attachment for the real name, size, and mime', () => {
    const meta = artifactFileMeta(artifact({ kind: 'file', content: 'data:application/pdf;base64,JVBER' }), 'binary', {
      id: 'artifact-1',
      mime: 'application/pdf',
      name: 'tilbud.pdf',
      size: 2048,
      url: 'data:application/pdf;base64,JVBER',
    })
    expect(meta?.downloadName).toBe('tilbud.pdf')
    expect(meta?.bytes).toBe(2048)
  })

  it('synthesizes a text data URI so plain-text kinds are downloadable too', () => {
    const meta = artifactFileMeta(artifact({ content: 'print("hei")' }), 'code')
    expect(meta?.href.startsWith('data:text/plain;charset=utf-8,')).toBe(true)
    expect(meta?.downloadName).toBe('solve.py')
  })

  it('reports nothing downloadable when the content never arrived', () => {
    expect(artifactFileMeta(artifact({ content: '' }), 'binary')).toBeNull()
    expect(artifactFileMeta(artifact({ content: '   ' }), 'binary')).toBeNull()
  })

  it('refuses to fake a binary download from a non-data-URI payload', () => {
    // Handing the literal string over as an .xlsx would deliver a corrupt file.
    expect(artifactFileMeta(artifact({ content: 'Q4.xlsx', kind: 'spreadsheet' }), 'binary')).toBeNull()
  })

  it('resolves bare base64 image bytes the same way the renderer does', () => {
    const meta = artifactFileMeta(artifact({ content: base64('png-bytes'), kind: 'image', title: 'Et bilde' }), 'image')
    expect(meta?.href).toBe(`data:image/png;base64,${base64('png-bytes')}`)
    expect(meta?.mime).toBe('image/png')
    expect(meta?.downloadName).toBe('et-bilde.png')
  })
})

describe('artifactDownloadName', () => {
  it('uses a title that already looks like a filename', () => {
    expect(artifactDownloadName(artifact({ title: 'solve.py' }), 'code')).toBe('solve.py')
  })

  it('slugifies a prose title and adds an extension from the kind', () => {
    expect(artifactDownloadName(artifact({ kind: 'document', title: 'Årsrapport 2026' }), 'document'))
      .toBe('arsrapport-2026.md')
  })
})

describe('normalizeArtifact', () => {
  it('keeps an announced artifact whose content never arrived', () => {
    // The panel must be able to say "kunne ikke lastes" — silently dropping it
    // would leave the user with no trace of an artifact they were told about.
    const normalized = normalizeArtifact({ id: 'a1', kind: 'spreadsheet', title: 'Q4.xlsx', version: 2 })
    expect(normalized).not.toBeNull()
    expect(normalized?.content).toBe('')
    expect(normalized?.version).toBe(2)
  })

  it('drops an event with neither content nor id', () => {
    expect(normalizeArtifact({ kind: 'code' })).toBeNull()
  })
})

describe('findArtifactById', () => {
  it('returns the highest-version carrier across turns', () => {
    const lists = [
      [artifact({ content: 'v1', version: 1 })],
      undefined,
      [artifact({ content: 'v2', version: 2 }), artifact({ id: 'other', version: 9 })],
    ]
    expect(findArtifactById(lists, 'artifact-1')?.content).toBe('v2')
    expect(findArtifactById(lists, 'missing')).toBeUndefined()
  })
})

describe('highlightCode', () => {
  it('numbers lines and preserves every character', () => {
    const source = 'def hei():\n    return "verden"\n'
    const lines = highlightCode(source, 'python')
    expect(lines.map((line) => line.number)).toEqual([1, 2])
    const rebuilt = lines.map((line) => line.tokens.map((token) => token.text).join('')).join('\n')
    expect(rebuilt).toBe('def hei():\n    return "verden"')
  })

  it('tags keywords, strings, and comments', () => {
    const lines = highlightCode('# note\nconst x = "a"', 'javascript')
    const types = lines.flatMap((line) => line.tokens.map((token) => token.type))
    expect(types).toContain('keyword')
    expect(types).toContain('string')
  })

  it('falls back to plain text for an unknown language', () => {
    const lines = highlightCode('const x = 1', 'klingon')
    expect(lines).toEqual([{ number: 1, tokens: [{ text: 'const x = 1', type: 'plain' }] }])
  })

  it('handles empty input without producing a phantom line', () => {
    expect(highlightCode('', 'python')).toEqual([{ number: 1, tokens: [] }])
  })
})

describe('pdf artifacts (definition of finished, point 4)', () => {
  /**
   * A generated PDF used to fall into the binary set and render as a download
   * card, while an ATTACHED pdf previewed in an iframe. Same file, two answers.
   */
  it('previews a pdf artifact whose content the browser can open', () => {
    expect(artifactRenderKind(artifact({ kind: 'pdf', content: 'data:application/pdf;base64,JVBER' }))).toBe('pdf')
    expect(artifactRenderKind(artifact({ kind: 'PDF', content: 'https://example.invalid/rapport.pdf' }))).toBe('pdf')
    expect(artifactRenderKind(artifact({ kind: 'pdf', content: 'blob:http://localhost/9f2c' }))).toBe('pdf')
  })

  /**
   * The conservative half, and the reason the check is not `looksLikeImageContent`'s
   * shape: bare base64 or prose on a `pdf` artifact is not addressable, and
   * guessing would replace a working download with an empty frame.
   */
  it('keeps the download card when a pdf artifact has no openable source', () => {
    expect(artifactRenderKind(artifact({ kind: 'pdf', content: 'JVBERi0xLjQK' }))).toBe('binary')
    expect(artifactRenderKind(artifact({ kind: 'pdf', content: 'Rapporten er vedlagt.' }))).toBe('binary')
    expect(artifactRenderKind(artifact({ kind: 'pdf', content: '' }))).toBe('binary')
    // Not an http(s) URL, and not a pdf data URI.
    expect(artifactRenderKind(artifact({ kind: 'pdf', content: 'file:///C:/rapport.pdf' }))).toBe('binary')
  })

  /**
   * A generic `file` kind carrying pdf bytes stays a download on purpose — that
   * is pinned by the case above in `artifactRenderKind`, and only an explicit
   * `pdf` kind claims the viewer.
   */
  it('does not promote a generic file artifact to the pdf viewer', () => {
    expect(artifactRenderKind(artifact({ kind: 'file', content: 'data:application/pdf;base64,JVBER' }))).toBe('binary')
  })

  it('labels every render kind in both languages', () => {
    // Stand-ins for `i18n.tr`, one per locale. Unused args carry the `_`
    // prefix the lint rule requires.
    const no = (noText: string, _enText: string) => noText
    const en = (_noText: string, enText: string) => enText
    expect(artifactRenderKindLabel('pdf', no)).toBe('PDF')
    expect(artifactRenderKindLabel('binary', no)).toBe('Fil')
    expect(artifactRenderKindLabel('binary', en)).toBe('File')
    expect(artifactRenderKindLabel('html', no)).toBe('Nettside')
    expect(artifactRenderKindLabel('html', en)).toBe('Web page')
  })
})
