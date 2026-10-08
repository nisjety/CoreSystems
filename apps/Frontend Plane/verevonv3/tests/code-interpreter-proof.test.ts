import { describe, expect, it } from 'vitest'
import {
  CAP_CODES, PROOF_CASES, SANDBOX_LIMIT_MS, answerHasDeadFileLink, dataUriBytes, judgeCase, mockCodeInterpreterTurn,
  oracleInvocation, scrub, summariseTurn, type DeliveryDom, type OracleVerdict, type ProofCase, type TurnObservation,
} from './e2e/code-interpreter-proof'

const PNG_BYTES = Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex')
const file = (name: string, mime: string, bytes = 64) => ({ name, mime, bytes, content_b64: Buffer.alloc(bytes, 7).toString('base64') })
const MOCK_FILES = [
  file('salg-q3.xlsx', 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'),
  file('besokstall.png', 'image/png'),
]

/** Give every SSE frame of a turn its own read, `step` ms apart, starting at `first`. */
function timedChunks(body: string, first = 1000, step = 100) {
  return body.split('\n\n').filter(Boolean).map((frame, index) => ({ at: first + index * step, text: `${frame}\n\n` }))
}

const byKind = (kind: ProofCase['kind']) => PROOF_CASES.find(proofCase => proofCase.kind === kind)!

describe('the proof cases', () => {
  it('are the five outputs the task names, one fresh thread each', () => {
    expect(PROOF_CASES.map(proofCase => proofCase.kind)).toEqual(['xlsx', 'docx', 'pdf', 'pptx', 'png'])
    for (const proofCase of PROOF_CASES) {
      expect(proofCase.fileName.endsWith(proofCase.extension)).toBe(true)
      expect(proofCase.prompt, 'the prompt names its file').toContain(proofCase.fileName)
    }
  })

  it('ask the way a user would: neither the tool nor Python is named, because choosing the tool is the thing proved', () => {
    for (const proofCase of PROOF_CASES) expect(proofCase.prompt).not.toMatch(/code_interpreter|kodeverktøy|python|openpyxl|reportlab|python-docx|pypdf/i)
  })

  it('spell out what the oracle looks for', () => {
    expect(byKind('pdf').prompt).toContain('Rød grøt med fløte er søt og sunn, og blåbær fra Ålesund smaker best.')
    expect(byKind('pdf').prompt).toContain('Sammendrag')
    for (const word of ['Aktivitet', 'Ansvarlig', 'Frist', 'Statusnotat']) expect(byKind('docx').prompt).toContain(word)
    for (const word of ['Omsetning', 'Kostnad', 'Margin']) expect(byKind('xlsx').prompt).toContain(word)
    expect(byKind('pptx').prompt).toContain('Kvartalsrapport')
  })
})

describe('summariseTurn', () => {
  it('reads the tool call, its timing, the delivered files and the done frame from a code_interpreter turn', () => {
    const summary = summariseTurn(timedChunks(mockCodeInterpreterTurn(MOCK_FILES, 'Fil lagret.')), 1000)
    expect(summary.requestId).toBe('h5-dry-run')
    expect(summary.threadId).toBe('H5-DRY-RUN-THREAD')
    expect(summary.tools).toHaveLength(1)
    expect(summary.tools[0]).toMatchObject({ name: 'code_interpreter', status: 'ok', error: false, exitCode: 0, traceback: false })
    expect(summary.tools[0]!.filesDelivered).toEqual(['salg-q3.xlsx', 'besokstall.png'])
    // tool_call is the second frame and tool_result the third: one read apart.
    expect(summary.tools[0]!.resultAfterMs).toBe(100)
    expect(summary.attachments.map(item => item.name)).toEqual(['salg-q3.xlsx', 'besokstall.png'])
    // The gateway's attachment frame names the mime type `type`, not `mime`.
    expect(summary.attachments[0]).toMatchObject({ declaredBytes: 64, decodedBytes: 64, dataUri: true, mime: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' })
    expect(summary.attachments[0]!.sha256).toMatch(/^[0-9a-f]{64}$/)
    expect(summary.done).toBe(true)
    expect(summary.doneModel).toBe('gpt-5.6-terra')
    expect(summary.errorCodes).toEqual([])
    expect(summary.answerChars).toBe('Fil lagret.'.length)
    expect(summary.frameCounts).toMatchObject({ connected: 1, tool_call: 1, tool_result: 1, attachment: 2, artifact: 2, done: 1 })
  })

  it('measures a slow sandbox run as the time between the tool_call frame and its tool_result frame', () => {
    const body = mockCodeInterpreterTurn(MOCK_FILES.slice(0, 1), 'x')
    const frames = body.split('\n\n').filter(Boolean)
    const chunks = frames.map((frame, index) => ({ at: index < 2 ? 1000 + index * 50 : 9000 + index, text: `${frame}\n\n` }))
    expect(summariseTurn(chunks, 1000).tools[0]!.resultAfterMs).toBe(7952)
  })

  it('reads a traceback and a non-zero exit from a result that delivered no file', () => {
    const output = JSON.stringify({ stdout: '', stderr: 'Traceback (most recent call last):\n  File "_verevon_main.py", line 1\nImportError: No module named foo', exit_code: 1, files: [] })
    const text = [
      `event: tool_call\ndata: ${JSON.stringify({ id: 'c1', name: 'code_interpreter', args: {} })}\n\n`,
      `event: tool_result\ndata: ${JSON.stringify({ id: 'c1', status: 'ok', output, error: null })}\n\n`,
    ]
    const summary = summariseTurn(text.map((frame, index) => ({ at: 10 + index, text: frame })), 0)
    expect(summary.tools[0]).toMatchObject({ exitCode: 1, traceback: true, filesDelivered: [], error: false })
  })

  it('reads a result that execution-core rendered behind its provenance header line', () => {
    const output = `[source: org-internal]\n${JSON.stringify({ stdout: '', stderr: 'Traceback (most recent call last):\n  File "x"\nValueError: bad', exit_code: 1, files: [] })}`
    const text = `event: tool_call\ndata: ${JSON.stringify({ id: 'c1', name: 'code_interpreter', args: {} })}\n\nevent: tool_result\ndata: ${JSON.stringify({ id: 'c1', status: 'ok', output, error: null })}\n\n`
    expect(summariseTurn([{ at: 1, text }], 0).tools[0]).toMatchObject({ exitCode: 1, traceback: true })
  })

  it('records the error codes of a turn the subscription stopped, and the BFF transport-loss shape', () => {
    const text = `event: error\ndata: ${JSON.stringify({ code: 'subscription_usage_limit', message: 'x' })}\n\nevent: error\ndata: ${JSON.stringify({ error: { code: 'upstream_unavailable' } })}\n\n`
    const summary = summariseTurn([{ at: 1, text }], 0)
    expect(summary.errorCodes).toEqual(['subscription_usage_limit', 'upstream_unavailable'])
    expect(summary.done).toBe(false)
  })

  it('does not count a tool result as an error because it carries stderr', () => {
    const output = JSON.stringify({ stdout: 'ok', stderr: 'a warning', exit_code: 0, files: [] })
    const text = `event: tool_call\ndata: ${JSON.stringify({ id: 'c1', name: 'code_interpreter', args: {} })}\n\nevent: tool_result\ndata: ${JSON.stringify({ id: 'c1', status: 'ok', output, error: null })}\n\n`
    expect(summariseTurn([{ at: 1, text }], 0).tools[0]).toMatchObject({ error: false, exitCode: 0, traceback: false })
  })
})

describe('dataUriBytes', () => {
  it('decodes a base64 data URI and refuses anything else', () => {
    expect(dataUriBytes(`data:image/png;base64,${PNG_BYTES.toString('base64')}`)).toEqual(PNG_BYTES)
    expect(dataUriBytes('https://example.test/a.png')).toBeNull()
    expect(dataUriBytes('data:text/plain,hello')).toBeNull()
  })
})

// ---------------------------------------------------------------------------

function observation(kind: ProofCase['kind'], overrides: Partial<TurnObservation> = {}): TurnObservation {
  const proofCase = byKind(kind)
  const mime = kind === 'png' ? 'image/png' : kind === 'pdf' ? 'application/pdf' : 'application/octet-stream'
  const files = [file(proofCase.fileName, mime)]
  const summary = summariseTurn(timedChunks(mockCodeInterpreterTurn(files, 'Fil lagret.')), 1000)
  const dom: DeliveryDom = kind === 'png'
    ? { cards: [], previews: [{ src: 'data:image/png;base64,AA==', loaded: true, width: 800, download: 'besokstall.png' }] }
    : { cards: [{ name: proofCase.fileName, href: `data:${mime};base64,AA==`, label: 'x' }], previews: [] }
  const verdicts: OracleVerdict[] = [{ file: `01-${proofCase.fileName}`, kind, ok: true, problems: [], facts: {} }]
  return { summary, dom, verdicts, answer: 'Fil lagret: ' + proofCase.fileName + ', tilgjengelig for nedlasting nedenfor.', ...overrides }
}

describe('judgeCase', () => {
  it('passes a turn that called the tool, delivered the file as a card or preview and was read back clean', () => {
    for (const proofCase of PROOF_CASES) expect(judgeCase(proofCase, observation(proofCase.kind)), proofCase.kind).toEqual([])
  })

  it('says so when the tool decision never called code_interpreter, and what it called instead', () => {
    const base = observation('xlsx')
    const summary = { ...base.summary, tools: [{ ...base.summary.tools[0]!, name: 'create_artifact' }] }
    const problems = judgeCase(byKind('xlsx'), { ...base, summary })
    expect(problems.join('\n')).toContain('never called code_interpreter (tools called: create_artifact)')
  })

  it('fails a call that exited non-zero, or delivered a different kind of file', () => {
    const base = observation('docx')
    const failed = { ...base.summary, tools: [{ ...base.summary.tools[0]!, exitCode: 1 }] }
    expect(judgeCase(byKind('docx'), { ...base, summary: failed }).join('\n')).toContain('no code_interpreter call exited 0 and delivered a .docx file')
    const wrongFile = { ...base.summary, tools: [{ ...base.summary.tools[0]!, filesDelivered: ['notes.csv'] }] }
    expect(judgeCase(byKind('docx'), { ...base, summary: wrongFile }).join('\n')).toContain('delivered a .docx file')
  })

  it('fails a sandbox run at or past the 30 s limit', () => {
    const base = observation('pdf')
    const slow = { ...base.summary, tools: [{ ...base.summary.tools[0]!, resultAfterMs: SANDBOX_LIMIT_MS }] }
    expect(judgeCase(byKind('pdf'), { ...base, summary: slow }).join('\n')).toContain('past the 30000 ms sandbox limit')
    const quick = { ...base.summary, tools: [{ ...base.summary.tools[0]!, resultAfterMs: SANDBOX_LIMIT_MS - 1 }] }
    expect(judgeCase(byKind('pdf'), { ...base, summary: quick })).toEqual([])
  })

  it('treats a usage limit as the subscription stopping the turn, never as something to retry elsewhere', () => {
    for (const code of CAP_CODES) {
      const base = observation('xlsx')
      const problems = judgeCase(byKind('xlsx'), { ...base, summary: { ...base.summary, errorCodes: [code], done: false } })
      expect(problems[0]).toContain(`the subscription stopped the turn (${code})`)
      expect(problems[0]).toContain('do not switch model')
    }
  })

  it('fails a stream without done, or one that reports another model', () => {
    const base = observation('xlsx')
    expect(judgeCase(byKind('xlsx'), { ...base, summary: { ...base.summary, done: false } }).join('\n')).toContain('without a done frame')
    expect(judgeCase(byKind('xlsx'), { ...base, summary: { ...base.summary, doneModel: 'gpt-5.5' } }).join('\n')).toContain('not gpt-5.6-terra')
  })

  it('fails an attachment whose bytes do not match what the frame declared, or that is not a data URI', () => {
    const base = observation('pptx')
    const lying = { ...base.summary, attachments: [{ ...base.summary.attachments[0]!, declaredBytes: 999 }] }
    expect(judgeCase(byKind('pptx'), { ...base, summary: lying }).join('\n')).toContain('declared 999 bytes but carries 64')
    const remote = { ...base.summary, attachments: [{ ...base.summary.attachments[0]!, dataUri: false, decodedBytes: 0 }] }
    expect(judgeCase(byKind('pptx'), { ...base, summary: remote }).join('\n')).toContain('did not arrive as a non-empty data URI')
  })

  it('needs a download card for a document, carrying the data URI', () => {
    const base = observation('docx')
    expect(judgeCase(byKind('docx'), { ...base, dom: { cards: [], previews: [] } }).join('\n')).toContain('no download card for a .docx file')
    const link = { cards: [{ name: 'statusnotat.docx', href: 'https://x.test/a.docx', label: '' }], previews: [] }
    expect(judgeCase(byKind('docx'), { ...base, dom: link }).join('\n')).toContain('does not carry the data URI')
  })

  it('needs an inline preview for the PNG that actually loaded, and no file card for it', () => {
    const base = observation('png')
    expect(judgeCase(byKind('png'), { ...base, dom: { cards: [], previews: [] } }).join('\n')).toContain('did not appear as an inline preview')
    const unloaded = { cards: [], previews: [{ src: 'data:', loaded: false, width: 0, download: '' }] }
    expect(judgeCase(byKind('png'), { ...base, dom: unloaded }).join('\n')).toContain('did not load')
    const both = { cards: [{ name: 'besokstall.png', href: 'data:image/png;base64,AA==', label: '' }], previews: base.dom.previews }
    expect(judgeCase(byKind('png'), { ...base, dom: both }).join('\n')).toContain('also shown as a file card')
  })

  it('carries the oracle\'s verdict: a file the library rejects fails the case with its reasons', () => {
    const base = observation('xlsx')
    const verdicts: OracleVerdict[] = [{ file: '01-salg-q3.xlsx', kind: 'xlsx', ok: false, problems: ['no native chart part', 'no SUM formula'], facts: {} }]
    const problems = judgeCase(byKind('xlsx'), { ...base, verdicts })
    expect(problems).toContain('01-salg-q3.xlsx: no native chart part')
    expect(problems).toContain('01-salg-q3.xlsx: no SUM formula')
    expect(judgeCase(byKind('xlsx'), { ...base, verdicts: [] }).join('\n')).toContain('the oracle was given no .xlsx file')
  })

  it('judges the deck by its own .pptx verdict, not by the extra PNG the program also wrote', () => {
    const base = observation('pptx')
    const verdicts: OracleVerdict[] = [
      { file: '01-kvartalsrapport.pptx', kind: 'pptx', ok: true, problems: [], facts: {} },
      { file: '02-omsetning.png', kind: 'png', ok: false, problems: ['picture looks blank'], facts: {} },
    ]
    expect(judgeCase(byKind('pptx'), { ...base, verdicts })).toEqual([])
  })

  it('fails an answer that links to the file', () => {
    const base = observation('pdf')
    expect(judgeCase(byKind('pdf'), { ...base, answer: 'Her er [sammendrag.pdf](sandbox:/mnt/data/sammendrag.pdf)' }).join('\n')).toContain('links to the file')
  })
})

describe('answerHasDeadFileLink', () => {
  it('catches sandbox paths and markdown links to a generated file, and nothing else', () => {
    expect(answerHasDeadFileLink('Last ned sandbox:/mnt/data/a.xlsx')).toBe(true)
    expect(answerHasDeadFileLink('[a.xlsx](https://example.test/files/a.xlsx)')).toBe(true)
    expect(answerHasDeadFileLink('[a.xlsx](a.xlsx)')).toBe(true)
    expect(answerHasDeadFileLink('Fil lagret: salg-q3.xlsx (6 KB), tilgjengelig for nedlasting nedenfor.')).toBe(false)
    expect(answerHasDeadFileLink('Se [kilden](https://example.test/artikkel) for bakgrunn.')).toBe(false)
  })
})

describe('oracleInvocation', () => {
  it('runs python in a throwaway container from the stack image, offline, with both mounts read-only', () => {
    const { command, args } = oracleInvocation('check', 'C:/proof/xlsx', {})
    expect(command).toBe('docker')
    expect(args.slice(0, 4)).toEqual(['run', '--rm', '--network', 'none'])
    expect(args.filter(arg => arg.endsWith(':ro'))).toHaveLength(2)
    expect(args).toContain('--entrypoint')
    expect(args[args.indexOf('--entrypoint') + 1]).toBe('python3')
    expect(args).toContain('model-plane-execution-core')
    expect(args.slice(-3)).toEqual(['/h5/oracle/h5_oracle.py', 'check', '/h5/files'].slice(0, 3))
  })

  it('mounts no files for reference and self-test', () => {
    for (const command of ['reference', 'self-test'] as const) {
      const { args } = oracleInvocation(command, null, {})
      expect(args.filter(arg => arg.endsWith(':ro'))).toHaveLength(1)
      expect(args.at(-1)).toBe(command)
    }
  })

  it('uses a local interpreter when H5_ORACLE_PYTHON is set, and another image when H5_ORACLE_IMAGE is', () => {
    const local = oracleInvocation('check', 'C:/proof/png', { H5_ORACLE_PYTHON: 'C:/py/python.exe' })
    expect(local.command).toBe('C:/py/python.exe')
    expect(local.args[0]).toMatch(/h5_oracle\.py$/)
    expect(local.args.slice(1)).toEqual(['check', expect.stringMatching(/proof[\\/]png$/)])
    expect(oracleInvocation('reference', null, { H5_ORACLE_IMAGE: 'my/image:1' }).args).toContain('my/image:1')
  })
})

describe('scrub', () => {
  it('keeps identifier-shaped strings and drops prose, so evidence never carries an answer', () => {
    expect(scrub({ file: 'salg-q3.xlsx', mime: 'image/png', note: 'Dette er et svar med mellomrom', n: 3, ok: true, list: ['a_b', 'a b'] }))
      .toEqual({ file: 'salg-q3.xlsx', mime: 'image/png', note: '[omitted]', n: 3, ok: true, list: ['a_b', '[omitted]'] })
  })
})
