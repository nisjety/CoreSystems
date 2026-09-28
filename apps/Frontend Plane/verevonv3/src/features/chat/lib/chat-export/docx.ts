/**
 * `buildDocx` — a finished answer as valid WordprocessingML (.docx), built
 * from scratch with no dependency beyond this directory's own `zip.ts` and
 * `ooxml.ts`.
 *
 * Kept pure on purpose (no DOM, no Solid): it only reads `MarkdownBlock[]`
 * data and returns bytes, so it runs identically in the browser, under
 * vitest, and under a plain `node --experimental-strip-types` invocation of
 * `sample-exports.mts`. The one import from outside this directory,
 * `MarkdownBlock` et al. from `chat-types.ts`, is `import type` only —
 * `verbatimModuleSyntax` erases it at compile time, so it carries no runtime
 * dependency on anything that file itself imports (solid-js JSX included).
 */

import type {
  MarkdownBlock,
  MarkdownListItem,
  MarkdownTableAlign,
} from '../../components/chat-types'
// Explicit `.ts` extensions below: this module must be importable by a plain
// `node --experimental-strip-types` run (`sample-exports.mts`), and Node's
// ESM resolver requires the extension on a relative specifier — see
// `ooxml.ts`'s header comment for the full reasoning.
import {
  buildAppPropsXml,
  buildCorePropsXml,
  buildPackageRelsXml,
  encodeUtf8,
  escapeXmlAttr,
  escapeXmlText,
  formatExportDate,
  isSafeExportHref,
  leadHeadingText,
  tokenizeInline,
  XML_DECLARATION,
  type InlineRun,
} from './ooxml.ts'
import { buildZip, type ZipEntry } from './zip.ts'
import type { ExportLocale, ExportMeta } from './types.ts'

const W_NS = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
const R_NS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'

/** Every bullet-family list in a document shares this numbering instance — bullets never need to "restart", so one definition is enough for all of them. */
const BULLET_NUM_ID = 1
/** Fallback for a numbered item that isn't the top level of its OWN ordered `MarkdownBlock` (a numbered item nested inside an otherwise-bulleted list, which the parser never assigns a `startNumber`). */
const ORDERED_DEFAULT_NUM_ID = 2
/** Every TOP-LEVEL ordered list gets its own `w:num` starting from this id, so it can restart its OWN `startNumber` (see F-02 in chat-media-markdown.tsx) without disturbing any other ordered list in the same document. */
const FIRST_ORDERED_INSTANCE_NUM_ID = 3

type DocxContext = {
  locale: ExportLocale
  hyperlinkRelIds: Map<string, string>
  nextRelId: number
  nextOrderedNumId: number
  /** `<w:num>` instance definitions accumulated while walking the document body, one per top-level ordered list, emitted into `word/numbering.xml` at the end. */
  orderedNumInstances: string[]
}

function createContext(locale: ExportLocale): DocxContext {
  return {
    locale,
    hyperlinkRelIds: new Map(),
    nextRelId: 3, // rId1 = styles.xml, rId2 = numbering.xml (fixed, see buildDocumentRelsXml)
    nextOrderedNumId: FIRST_ORDERED_INSTANCE_NUM_ID,
    orderedNumInstances: [],
  }
}

function hyperlinkRelId(ctx: DocxContext, url: string): string {
  const existing = ctx.hyperlinkRelIds.get(url)
  if (existing) return existing
  const id = `rId${ctx.nextRelId}`
  ctx.nextRelId += 1
  ctx.hyperlinkRelIds.set(url, id)
  return id
}

function runPropsXml(run: InlineRun): string {
  const parts: string[] = []
  if (run.bold) parts.push('<w:b/>')
  if (run.italic) parts.push('<w:i/>')
  if (run.code) parts.push('<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/>')
  return parts.length ? `<w:rPr>${parts.join('')}</w:rPr>` : ''
}

/**
 * One run becomes one `<w:r>`, except a link run, which is wrapped in a real
 * `<w:hyperlink>` (see `buildDocumentRelsXml` for the matching external
 * relationship this refers to by `r:id`). A run never carries both `href`
 * and bold/italic/code — `tokenizeInline`'s alternation only ever assigns one
 * category per token — so there is no formatting to preserve across the two
 * branches here.
 */
function runXml(ctx: DocxContext, run: InlineRun): string {
  if (!run.href) {
    return `<w:r>${runPropsXml(run)}<w:t xml:space="preserve">${escapeXmlText(run.text)}</w:t></w:r>`
  }
  const rId = hyperlinkRelId(ctx, run.href)
  return (
    `<w:hyperlink r:id="${rId}" w:history="1">` +
    `<w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr>` +
    `<w:t xml:space="preserve">${escapeXmlText(run.text)}</w:t></w:r>` +
    '</w:hyperlink>'
  )
}

function runsXml(ctx: DocxContext, runs: readonly InlineRun[]): string {
  return runs.map((run) => runXml(ctx, run)).join('')
}

/** A run list forced bold — used for the details `<summary>` line, which reads as a heading even though it's ordinary paragraph markdown underneath. */
function boldRuns(runs: readonly InlineRun[]): InlineRun[] {
  return runs.map((run) => ({ ...run, bold: true }))
}

function paragraphXml(ctx: DocxContext, text: string, pStyle?: string, extraPPr = ''): string {
  const inner = `${pStyle ? `<w:pStyle w:val="${pStyle}"/>` : ''}${extraPPr}`
  const pPr = inner ? `<w:pPr>${inner}</w:pPr>` : ''
  return `<w:p>${pPr}${runsXml(ctx, tokenizeInline(text))}</w:p>`
}

/**
 * Code content is verbatim: no markdown tokenizing at all (a `*` in a code
 * block must stay a literal asterisk, never become italics), a tab becomes a
 * real `<w:tab/>` run — the literal tab CHARACTER inside `<w:t>` is exactly
 * the kind of whitespace Word is free to collapse — and every text run
 * carries `xml:space="preserve"` so leading/trailing spaces used for
 * indentation survive.
 */
function codeLineRunsXml(line: string): string {
  const segments = line.split('\t')
  const parts: string[] = []
  segments.forEach((segment, index) => {
    parts.push(`<w:r><w:t xml:space="preserve">${escapeXmlText(segment)}</w:t></w:r>`)
    if (index < segments.length - 1) parts.push('<w:r><w:tab/></w:r>')
  })
  return parts.join('')
}

function codeLikeParagraphs(text: string): string {
  const lines = text.length ? text.split('\n') : ['']
  return lines
    .map((line) => `<w:p><w:pPr><w:pStyle w:val="Code"/></w:pPr>${codeLineRunsXml(line)}</w:p>`)
    .join('')
}

function taskPrefix(item: MarkdownListItem): string {
  if (item.checked === undefined) return ''
  return item.checked ? '☑ ' : '☐ ' // BALLOT BOX WITH CHECK / BALLOT BOX
}

function renderList(ctx: DocxContext, block: Extract<MarkdownBlock, { kind: 'list' }>): string {
  let orderedBlockNumId: number | null = null
  if (block.ordered) {
    orderedBlockNumId = ctx.nextOrderedNumId
    ctx.nextOrderedNumId += 1
    ctx.orderedNumInstances.push(
      `<w:num w:numId="${orderedBlockNumId}"><w:abstractNumId w:val="1"/>` +
        `<w:lvlOverride w:ilvl="0"><w:startOverride w:val="${block.startNumber ?? 1}"/></w:lvlOverride>` +
        '</w:num>',
    )
  }
  return block.items
    .map((item) => {
      const ilvl = Math.min(8, Math.max(0, item.depth))
      const numId = item.ordered ? (orderedBlockNumId ?? ORDERED_DEFAULT_NUM_ID) : BULLET_NUM_ID
      const numPr = `<w:numPr><w:ilvl w:val="${ilvl}"/><w:numId w:val="${numId}"/></w:numPr>`
      const runs = tokenizeInline(`${taskPrefix(item)}${item.text}`)
      return `<w:p><w:pPr>${numPr}</w:pPr>${runsXml(ctx, runs)}</w:p>`
    })
    .join('')
}

function jcForAlign(align: MarkdownTableAlign | undefined): string | null {
  if (align === 'center' || align === 'right' || align === 'left') return align
  return null
}

/** Total usable width inside the A4 body this document lays out (see `buildSectPr`): 11906 twips page minus 1440 twips of margin on each side. */
const TABLE_TOTAL_WIDTH_TWIPS = 9026

function renderTableRow(ctx: DocxContext, cells: readonly string[], align: readonly MarkdownTableAlign[], header: boolean): string {
  const trPr = header ? '<w:trPr><w:tblHeader/></w:trPr>' : ''
  const tcs = cells
    .map((cell) => {
      const runs = tokenizeInline(cell)
      return header ? boldRuns(runs) : runs
    })
    .map((runs, index) => {
      const jc = jcForAlign(align[index])
      const pPr = jc ? `<w:pPr><w:jc w:val="${jc}"/></w:pPr>` : ''
      return `<w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr><w:p>${pPr}${runsXml(ctx, runs)}</w:p></w:tc>`
    })
    .join('')
  return `<w:tr>${trPr}${tcs}</w:tr>`
}

function renderTable(ctx: DocxContext, block: Extract<MarkdownBlock, { kind: 'table' }>): string {
  const columnCount = Math.max(1, block.header.length)
  const columnWidth = Math.floor(TABLE_TOTAL_WIDTH_TWIPS / columnCount)
  const grid = `<w:tblGrid>${block.header.map(() => `<w:gridCol w:w="${columnWidth}"/>`).join('')}</w:tblGrid>`
  const tblPr = '<w:tblPr><w:tblStyle w:val="VerevonTable"/><w:tblW w:w="0" w:type="auto"/><w:tblLook w:val="04A0"/></w:tblPr>'
  const headerRow = renderTableRow(ctx, block.header, block.align, true)
  const bodyRows = block.rows.map((row) => renderTableRow(ctx, row, block.align, false)).join('')
  return `<w:tbl>${tblPr}${grid}${headerRow}${bodyRows}</w:tbl>`
}

function renderDetails(ctx: DocxContext, block: Extract<MarkdownBlock, { kind: 'details' }>): string {
  const summaryP = `<w:p>${runsXml(ctx, boldRuns(tokenizeInline(block.summary)))}</w:p>`
  return summaryP + block.blocks.map((inner) => blockXml(ctx, inner)).join('')
}

function blockXml(ctx: DocxContext, block: MarkdownBlock): string {
  switch (block.kind) {
    case 'heading':
      return paragraphXml(ctx, block.text, `Heading${block.level}`)
    case 'paragraph':
      return block.text ? paragraphXml(ctx, block.text) : '<w:p/>'
    case 'hr':
      return '<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="D1D5DB"/></w:pBdr></w:pPr></w:p>'
    case 'quote':
      return block.lines
        .map((line) => paragraphXml(ctx, line.text, 'Quote', `<w:ind w:left="${288 * Math.max(1, line.depth)}"/>`))
        .join('')
    case 'list':
      return renderList(ctx, block)
    case 'table':
      return renderTable(ctx, block)
    case 'code':
      return codeLikeParagraphs(block.text)
    case 'math':
      return codeLikeParagraphs(block.text)
    case 'details':
      return renderDetails(ctx, block)
    /* c8 ignore next 2 -- exhaustiveness guard: a new MarkdownBlock kind fails the build, not this line at runtime. */
    default:
      return ((_never: never) => '')(block)
  }
}

function buildSourcesSection(ctx: DocxContext, meta: ExportMeta): string {
  const sources = meta.sources ?? []
  if (sources.length === 0) return ''
  const heading = ctx.locale === 'en' ? 'Sources' : 'Kilder'
  const headingP = paragraphXml(ctx, heading, 'Heading2')
  const items = sources
    .map((source, index) => {
      const label = `${index + 1}. ${source.title}`
      const url = source.url?.trim()
      if (!url) return `<w:p>${runsXml(ctx, tokenizeInline(label))}</w:p>`
      // A source url with a scheme outside the export's own allowlist (see
      // `isSafeExportHref`) is still shown — a reader benefits from seeing
      // where a citation claims to point — just never as a clickable
      // external-relationship hyperlink.
      const runs: InlineRun[] = isSafeExportHref(url)
        ? [{ text: `${label} — ` }, { text: url, href: url }]
        : [{ text: `${label} — ${url}` }]
      return `<w:p>${runsXml(ctx, runs)}</w:p>`
    })
    .join('')
  return headingP + items
}

function buildFooterParagraph(locale: ExportLocale, createdAt: string | undefined): string {
  const date = formatExportDate(createdAt, locale)
  const text = locale === 'en' ? `Exported from Verevon ${date}` : `Eksportert fra Verevon ${date}`
  return (
    '<w:p><w:pPr><w:spacing w:before="360"/></w:pPr>' +
    `<w:r><w:rPr><w:i/><w:sz w:val="16"/><w:color w:val="9CA3AF"/></w:rPr>` +
    `<w:t xml:space="preserve">${escapeXmlText(text)}</w:t></w:r></w:p>`
  )
}

/** A4, 1-inch margins — the same body width `TABLE_TOTAL_WIDTH_TWIPS` assumes. */
const SECT_PR =
  '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>' +
  '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/>' +
  '</w:sectPr>'

function buildDocumentXml(blocks: readonly MarkdownBlock[], meta: ExportMeta, ctx: DocxContext): string {
  const titleP = `<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr>${runsXml(ctx, tokenizeInline(meta.title))}</w:p>`
  // An answer that opens with its own heading usually has that heading as
  // its title (index.ts); keeping it in the body as well prints the same
  // line twice, once as Title and once as Heading, straight under each other.
  const body = leadHeadingText(blocks) === meta.title.replace(/\s+/g, ' ').trim() ? blocks.slice(1) : blocks
  const bodyXml = body.map((block) => blockXml(ctx, block)).join('')
  const sourcesXml = buildSourcesSection(ctx, meta)
  const footerXml = buildFooterParagraph(ctx.locale, meta.createdAt)
  return (
    XML_DECLARATION +
    `<w:document xmlns:w="${W_NS}" xmlns:r="${R_NS}">` +
    `<w:body>${titleP}${bodyXml}${sourcesXml}${footerXml}${SECT_PR}</w:body>` +
    '</w:document>'
  )
}

function buildStylesXml(): string {
  return (
    XML_DECLARATION +
    `<w:styles xmlns:w="${W_NS}">` +
    '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:lang w:val="nb-NO"/></w:rPr></w:rPrDefault></w:docDefaults>' +
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:style>' +
    '<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:spacing w:after="240"/></w:pPr><w:rPr><w:b/><w:sz w:val="44"/><w:color w:val="1F2937"/></w:rPr></w:style>' +
    '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:keepNext/><w:spacing w:before="360" w:after="160"/><w:outlineLvl w:val="0"/></w:pPr>' +
    '<w:rPr><w:b/><w:sz w:val="32"/><w:color w:val="1F2937"/></w:rPr></w:style>' +
    '<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:keepNext/><w:spacing w:before="280" w:after="140"/><w:outlineLvl w:val="1"/></w:pPr>' +
    '<w:rPr><w:b/><w:sz w:val="27"/><w:color w:val="1F2937"/></w:rPr></w:style>' +
    '<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:keepNext/><w:spacing w:before="220" w:after="120"/><w:outlineLvl w:val="2"/></w:pPr>' +
    '<w:rPr><w:b/><w:sz w:val="24"/><w:color w:val="1F2937"/></w:rPr></w:style>' +
    '<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:pBdr><w:left w:val="single" w:sz="12" w:space="8" w:color="9CA3AF"/></w:pBdr><w:ind w:left="288"/></w:pPr>' +
    '<w:rPr><w:i/><w:color w:val="4B5563"/></w:rPr></w:style>' +
    '<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:qFormat/>' +
    '<w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F3F4F6"/><w:spacing w:after="0"/></w:pPr>' +
    '<w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="20"/></w:rPr></w:style>' +
    '<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/>' +
    '<w:rPr><w:color w:val="2B5CD9"/><w:u w:val="single"/></w:rPr></w:style>' +
    '<w:style w:type="table" w:styleId="VerevonTable"><w:name w:val="Verevon Table"/><w:tblPr><w:tblBorders>' +
    '<w:top w:val="single" w:sz="4" w:color="D1D5DB"/><w:left w:val="single" w:sz="4" w:color="D1D5DB"/>' +
    '<w:bottom w:val="single" w:sz="4" w:color="D1D5DB"/><w:right w:val="single" w:sz="4" w:color="D1D5DB"/>' +
    '<w:insideH w:val="single" w:sz="4" w:color="D1D5DB"/><w:insideV w:val="single" w:sz="4" w:color="D1D5DB"/>' +
    '</w:tblBorders></w:tblPr></w:style>' +
    '</w:styles>'
  )
}

function abstractNumLevels(format: 'bullet' | 'decimal'): string {
  const levels: string[] = []
  for (let level = 0; level < 9; level += 1) {
    const indent = 360 * (level + 1)
    const lvlText = format === 'bullet' ? '•' : `%${level + 1}.`
    levels.push(
      `<w:lvl w:ilvl="${level}"><w:start w:val="1"/><w:numFmt w:val="${format}"/>` +
        `<w:lvlText w:val="${lvlText}"/><w:lvlJc w:val="left"/>` +
        `<w:pPr><w:ind w:left="${indent}" w:hanging="360"/></w:pPr></w:lvl>`,
    )
  }
  return levels.join('')
}

function buildNumberingXml(orderedNumInstances: readonly string[]): string {
  const bulletAbstract = `<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>${abstractNumLevels('bullet')}</w:abstractNum>`
  const decimalAbstract = `<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>${abstractNumLevels('decimal')}</w:abstractNum>`
  const staticInstances =
    '<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>' +
    '<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>'
  return (
    XML_DECLARATION +
    `<w:numbering xmlns:w="${W_NS}">` +
    bulletAbstract +
    decimalAbstract +
    staticInstances +
    orderedNumInstances.join('') +
    '</w:numbering>'
  )
}

function buildDocumentRelsXml(hyperlinkRelIds: ReadonlyMap<string, string>): string {
  const base =
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>' +
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>'
  const hyperlinks = [...hyperlinkRelIds.entries()]
    .map(
      ([url, id]) =>
        `<Relationship Id="${id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="${escapeXmlAttr(url)}" TargetMode="External"/>`,
    )
    .join('')
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    base +
    hyperlinks +
    '</Relationships>'
  )
}

function buildContentTypesXml(): string {
  return (
    XML_DECLARATION +
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
    '<Default Extension="xml" ContentType="application/xml"/>' +
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>' +
    '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>' +
    '<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>' +
    '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>' +
    '<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>' +
    '</Types>'
  )
}

/** Builds a complete, valid WordprocessingML (.docx) package for one finished chat answer. */
export function buildDocx(blocks: readonly MarkdownBlock[], meta: ExportMeta): Uint8Array {
  const ctx = createContext(meta.locale ?? 'no')
  const documentXml = buildDocumentXml(blocks, meta, ctx)
  const numberingXml = buildNumberingXml(ctx.orderedNumInstances)

  const entries: ZipEntry[] = [
    { name: '[Content_Types].xml', data: encodeUtf8(buildContentTypesXml()) },
    { name: '_rels/.rels', data: encodeUtf8(buildPackageRelsXml('word/document.xml')) },
    { name: 'word/document.xml', data: encodeUtf8(documentXml) },
    { name: 'word/styles.xml', data: encodeUtf8(buildStylesXml()) },
    { name: 'word/numbering.xml', data: encodeUtf8(numberingXml) },
    { name: 'word/_rels/document.xml.rels', data: encodeUtf8(buildDocumentRelsXml(ctx.hyperlinkRelIds)) },
    { name: 'docProps/core.xml', data: encodeUtf8(buildCorePropsXml(meta.title, meta.createdAt)) },
    { name: 'docProps/app.xml', data: encodeUtf8(buildAppPropsXml()) },
  ]
  return buildZip(entries)
}
