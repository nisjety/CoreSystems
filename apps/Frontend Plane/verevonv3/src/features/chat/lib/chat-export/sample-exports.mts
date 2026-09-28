#!/usr/bin/env -S node --experimental-strip-types
/**
 * Builds a realistic Norwegian sample answer through the three pure OOXML
 * builders (`buildDocx`/`buildXlsx`/`buildPptx`) and writes the result to a
 * directory, so a human (or an orchestrator) can open the files in real
 * Word/Excel/PowerPoint and confirm they're not just well-formed XML but
 * actually look right.
 *
 * Deliberately hand-builds the `MarkdownBlock[]` fixture below instead of
 * running the real markdown parser (`parseMarkdownBlocks` in
 * `chat-media-markdown.tsx`): that file is a `.tsx` module using solid-js
 * JSX, which has no meaning to a plain `node --experimental-strip-types` run
 * — there is no JSX transform here, only TypeScript type-stripping. The
 * parser already has its own tests (`chat-markdown.test.ts`); this script's
 * job is to exercise the OOXML BUILDERS against realistic input, not to
 * re-verify the markdown grammar.
 *
 * Run with:
 *   node --experimental-strip-types sample-exports.mts <outdir>
 */

import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { MarkdownBlock } from '../../components/chat-types.ts'
import { buildDocx } from './docx.ts'
import { buildXlsx } from './xlsx.ts'
import { buildPptx } from './pptx.ts'
import type { ExportMeta } from './types.ts'

const outDir = process.argv[2]
if (!outDir) {
  console.error('Usage: node --experimental-strip-types sample-exports.mts <outdir>')
  process.exit(1)
}

const blocks: MarkdownBlock[] = [
  { kind: 'heading', level: 1, text: 'Kvartalsrapport for Q3 2026' },
  {
    kind: 'paragraph',
    text:
      'Dette er et **sammendrag** av kvartalet, med tall hentet fra ' +
      '[regnskapssystemet](https://regnskap.example.com/q3-2026). ' +
      'Se også `beregnMarginal` under for detaljer om marginberegningen.',
  },
  { kind: 'heading', level: 2, text: 'Nøkkeltall' },
  {
    kind: 'table',
    align: [null, 'right', 'right', 'right'],
    header: ['Produkt', 'Solgt (stk)', 'Omsetning (kr)', 'Andel av total'],
    rows: [
      ['Kaffe', '1 234', '1 234,50', '45,2 %'],
      ['Te', '567', '12,5', '12,5 %'],
      ['Sjokolade', '89', '8 901,00', '8,9 %'],
      ['Diverse', '12.5', '1.234', '3,1 %'],
    ],
  },
  { kind: 'heading', level: 2, text: 'Handlingspunkter' },
  {
    kind: 'list',
    ordered: false,
    items: [
      { depth: 0, ordered: false, text: 'Følg opp leverandører' },
      { depth: 1, ordered: false, text: 'Bekreft pris for Q4' },
      { depth: 1, ordered: false, text: 'Fornye avtale innen 1. oktober' },
      { depth: 0, ordered: false, text: 'Oppdater prognosen', checked: false },
      { depth: 0, ordered: false, text: 'Send rapport til styret', checked: true },
    ],
  },
  { kind: 'heading', level: 2, text: 'Marginberegning' },
  {
    kind: 'code',
    lang: 'ts',
    closed: true,
    text: 'function beregnMarginal(pris: number): number {\n\treturn pris * 0.18\n}',
  },
  {
    kind: 'quote',
    lines: [{ depth: 1, text: '«Vi ligger foran budsjett for tredje kvartal på rad.»' }],
  },
  { kind: 'paragraph', text: 'Dette bekreftes av ekstern revisjon [1].' },
  { kind: 'heading', level: 3, text: 'Forbehold' },
  { kind: 'paragraph', text: 'Tallene er foreløpige og kan justeres etter den endelige revisjonen.' },
]

const meta: ExportMeta = {
  title: 'Kvartalsrapport for Q3 2026',
  createdAt: '2026-09-28T09:00:00Z',
  locale: 'no',
  sources: [{ title: 'Ekstern revisjonsrapport 2026', url: 'https://revisjon.example.com/q3-2026' }],
}

mkdirSync(outDir, { recursive: true })

const targets: Array<{ file: string; bytes: Uint8Array }> = [
  { file: 'sample-kvartalsrapport.docx', bytes: buildDocx(blocks, meta) },
  { file: 'sample-kvartalsrapport.xlsx', bytes: buildXlsx(blocks, meta) },
  { file: 'sample-kvartalsrapport.pptx', bytes: buildPptx(blocks, meta) },
]

for (const target of targets) {
  const outPath = resolve(outDir, target.file)
  writeFileSync(outPath, target.bytes)
  console.log(`wrote ${outPath} (${target.bytes.byteLength} bytes)`)
}
