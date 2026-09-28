/**
 * `buildPptx` — a finished answer as a valid PresentationML (.pptx) deck:
 * a title slide, then a new slide at each level-1/2 heading, with paragraphs
 * and list items as bullets, a level-3 heading as a bold lead-in line, a code
 * block as a monospace box, and a table as a real `a:tbl`.
 *
 * PowerPoint is the STRICT one of the three formats this directory builds:
 * Word and Excel both tolerate a part missing or slightly out of order and
 * quietly repair it; PowerPoint's importer is far more likely to refuse the
 * whole file or show a "we found a problem" dialog. Every part the format
 * requires is written below even where its content is minimal — a complete
 * `theme1.xml` with all three `fmtScheme` list minimums, an actual
 * `slideMaster` -> `slideLayout` -> `slide` relationship chain, and
 * `presProps`/`viewProps`/`tableStyles` — rather than leaving any of them
 * out because a slide doesn't visibly need it.
 *
 * Pure — see `docx.ts`'s header comment for why, and why the one
 * `chat-types.ts` import below is `import type` only.
 */

import type { MarkdownBlock, MarkdownTableAlign } from '../../components/chat-types'
// Explicit `.ts` extensions: see `ooxml.ts`'s header comment — Node's ESM
// resolver needs them for `sample-exports.mts` to run this module directly.
import { buildAppPropsXml, buildCorePropsXml, buildPackageRelsXml, encodeUtf8, escapeXmlText, formatExportDate, plainTextFromRuns, stripInlineMarkdown, tokenizeInline, XML_DECLARATION } from './ooxml.ts'
import { buildZip, type ZipEntry } from './zip.ts'
import type { ExportLocale, ExportMeta } from './types.ts'

const A_NS = 'http://schemas.openxmlformats.org/drawingml/2006/main'
const P_NS = 'http://schemas.openxmlformats.org/presentationml/2006/main'
const R_NS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'

// 16:9 at PowerPoint's standard 914400 EMU/inch (13.333in x 7.5in).
const SLIDE_WIDTH = 12192000
const SLIDE_HEIGHT = 6858000
const MARGIN = 457200 // 0.5in
const CONTENT_WIDTH = SLIDE_WIDTH - MARGIN * 2
const TITLE_TOP = 274638
const TITLE_HEIGHT = 1143000
const CONTENT_TOP = TITLE_TOP + TITLE_HEIGHT + 100000
const CONTENT_HEIGHT = SLIDE_HEIGHT - CONTENT_TOP - MARGIN

/** How many bullets, or how many characters of bullet text, a single slide may hold before a "(forts.)" continuation slide starts — Copilot/Gamma-style decks use roughly this budget per slide before text starts overflowing the box. */
const MAX_BULLETS_PER_SLIDE = 8
const MAX_CHARS_PER_SLIDE = 700

type SlideBullet = { kind: 'bullet'; text: string; level: number }
type SlideBoldLine = { kind: 'bold-paragraph'; text: string }
type SlideCode = { kind: 'code'; text: string }
type SlideTable = { kind: 'table'; align: readonly MarkdownTableAlign[]; header: readonly string[]; rows: readonly (readonly string[])[] }
type SlideContentItem = SlideBullet | SlideBoldLine | SlideCode | SlideTable

type SlidePlan = { title: string; items: SlideContentItem[] }

/**
 * Walks the parsed answer into a flat list of slide plans. A level-1/2
 * heading always starts a fresh slide; everything else accumulates onto the
 * CURRENT slide (creating a leading one, titled generically, if the answer
 * starts with content before its first heading) until the bullet/character
 * budget above is spent, at which point a "(forts.)" continuation slide
 * opens under the same title.
 */
function planSlides(blocks: readonly MarkdownBlock[], meta: ExportMeta): SlidePlan[] {
  const locale: ExportLocale = meta.locale ?? 'no'
  const continuationSuffix = locale === 'en' ? ' (cont.)' : ' (forts.)'
  const leadingTitle = locale === 'en' ? 'Answer' : 'Svar'

  const slides: SlidePlan[] = []
  let current: SlidePlan | null = null
  let bulletCount = 0
  let charCount = 0

  const startSlide = (title: string) => {
    current = { title, items: [] }
    slides.push(current)
    bulletCount = 0
    charCount = 0
  }
  const ensureSlide = () => {
    if (!current) startSlide(leadingTitle)
    return current as SlidePlan
  }
  const addItem = (item: SlideContentItem) => {
    ensureSlide().items.push(item)
  }
  const addBullet = (text: string, level: number) => {
    const slide = ensureSlide()
    // Only split when the CURRENT slide already holds something: a single
    // bullet/paragraph longer than the character budget must still land on
    // the slide its heading just opened, not bounce straight onto an empty
    // slide's "(forts.)" continuation — that would leave the just-started
    // slide with a title and no content at all.
    if (slide.items.length > 0 && (bulletCount >= MAX_BULLETS_PER_SLIDE || charCount + text.length > MAX_CHARS_PER_SLIDE)) {
      const base = slide.title.endsWith(continuationSuffix) ? slide.title : `${slide.title}${continuationSuffix}`
      startSlide(base)
    }
    current!.items.push({ kind: 'bullet', text, level: Math.min(8, Math.max(0, level)) })
    bulletCount += 1
    charCount += text.length
  }

  const visit = (list: readonly MarkdownBlock[]) => {
    for (const block of list) {
      switch (block.kind) {
        case 'heading':
          if (block.level === 3) addItem({ kind: 'bold-paragraph', text: block.text })
          else startSlide(block.text)
          break
        case 'paragraph':
          if (block.text) addBullet(stripInlineMarkdown(block.text), 0)
          break
        case 'list':
          for (const item of block.items) {
            const prefix = item.checked === undefined ? '' : item.checked ? '☑ ' : '☐ '
            addBullet(`${prefix}${stripInlineMarkdown(item.text)}`, item.depth)
          }
          break
        case 'quote':
          for (const line of block.lines) addBullet(stripInlineMarkdown(line.text), 0)
          break
        case 'code':
        case 'math':
          addItem({ kind: 'code', text: block.text })
          break
        case 'table':
          addItem({ kind: 'table', align: block.align, header: block.header, rows: block.rows })
          break
        case 'details':
          if (block.summary) addItem({ kind: 'bold-paragraph', text: block.summary })
          visit(block.blocks)
          break
        case 'hr':
          break
        /* c8 ignore next 2 -- exhaustiveness guard: a new MarkdownBlock kind fails the build, not this line at runtime. */
        default:
          ((_never: never) => undefined)(block)
      }
    }
  }
  visit(blocks)
  return slides
}

/** Groups a slide's items into vertical stripes: a run of text-like items (bullets/code/bold lines) shares one text box, and each table gets its own `p:graphicFrame` — the two are different PresentationML shape kinds and can't share a text body. */
type ContentRun = { kind: 'text'; items: (SlideBullet | SlideBoldLine | SlideCode)[] } | { kind: 'table'; item: SlideTable }

function groupRuns(items: readonly SlideContentItem[]): ContentRun[] {
  const runs: ContentRun[] = []
  for (const item of items) {
    if (item.kind === 'table') {
      runs.push({ kind: 'table', item })
      continue
    }
    const last = runs.at(-1)
    if (last && last.kind === 'text') last.items.push(item)
    else runs.push({ kind: 'text', items: [item] })
  }
  return runs
}

function escapeAndPreserve(text: string): string {
  // PresentationML text runs collapse leading/trailing whitespace the same
  // way HTML does unless `xml:space="preserve"` is set on `<a:t>`.
  return escapeXmlText(text)
}

function shapeNvPr(id: number, name: string): string {
  return `<p:nvSpPr><p:cNvPr id="${id}" name="${escapeXmlText(name)}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>`
}

function shapeXfrm(x: number, y: number, cx: number, cy: number): string {
  return `<a:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></a:xfrm>`
}

function textRunXml(text: string, opts: { bold?: boolean; size?: number; monospace?: boolean } = {}): string {
  const rPr: string[] = [`sz="${opts.size ?? 1800}"`]
  if (opts.bold) rPr.push('b="1"')
  const font = opts.monospace ? '<a:latin typeface="Consolas"/>' : ''
  return `<a:r><a:rPr ${rPr.join(' ')} dirty="0">${font}</a:rPr><a:t>${escapeAndPreserve(text)}</a:t></a:r>`
}

/** A title textbox: one non-bulleted paragraph, sized for a slide heading. */
function titleBoxXml(shapeId: number, title: string): string {
  return (
    `<p:sp>${shapeNvPr(shapeId, 'Title')}` +
    `<p:spPr>${shapeXfrm(MARGIN, TITLE_TOP, CONTENT_WIDTH, TITLE_HEIGHT)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>` +
    '<p:txBody><a:bodyPr wrap="square" anchor="t"/><a:lstStyle/>' +
    `<a:p><a:pPr><a:buNone/></a:pPr>${textRunXml(title, { bold: true, size: 3200 })}</a:p>` +
    '</p:txBody></p:sp>'
  )
}

function bulletParagraphXml(item: SlideBullet | SlideBoldLine | SlideCode): string {
  if (item.kind === 'bold-paragraph') {
    return `<a:p><a:pPr><a:buNone/></a:pPr>${textRunXml(item.text, { bold: true, size: 2000 })}</a:p>`
  }
  if (item.kind === 'code') {
    const lines = item.text.length ? item.text.split('\n') : ['']
    return lines
      .map((line) => `<a:p><a:pPr marL="0" indent="0"><a:buNone/></a:pPr>${textRunXml(line, { size: 1400, monospace: true })}</a:p>`)
      .join('')
  }
  const indent = 228600 * item.level
  return (
    `<a:p><a:pPr marL="${228600 + indent}" indent="-228600" lvl="${item.level}">` +
    `<a:buFont typeface="Arial"/><a:buChar char="•"/></a:pPr>` +
    `${textRunXml(item.text, { size: 1800 })}</a:p>`
  )
}

function textRunShapeXml(shapeId: number, items: readonly (SlideBullet | SlideBoldLine | SlideCode)[], x: number, y: number, cx: number, cy: number): string {
  const paragraphs = items.map((item) => bulletParagraphXml(item)).join('')
  return (
    `<p:sp>${shapeNvPr(shapeId, 'Content')}` +
    `<p:spPr>${shapeXfrm(x, y, cx, cy)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>` +
    `<p:txBody><a:bodyPr wrap="square" anchor="t"><a:normAutofit/></a:bodyPr><a:lstStyle/>${paragraphs}</p:txBody>` +
    '</p:sp>'
  )
}

function tableCellXml(text: string, header: boolean): string {
  // A cell carries the same inline markdown as body text; a model bolds a
  // totals row cell by cell ("**Totalt**", "**360 000 kr**"), which reached
  // the slide as literal asterisks. Strip the syntax, and keep a cell that
  // was bold as a whole bold.
  const runs = tokenizeInline(text)
  const bold = header || (runs.some((run) => run.text.trim()) && runs.every((run) => run.bold || !run.text.trim()))
  const runProps = bold ? { bold: true, size: 1400 } : { size: 1400 }
  return (
    '<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>' +
    `<a:p>${textRunXml(plainTextFromRuns(runs), runProps)}</a:p>` +
    '</a:txBody><a:tcPr/></a:tc>'
  )
}

function tableGraphicFrameXml(shapeId: number, table: SlideTable, x: number, y: number, cx: number, cy: number): string {
  const columnCount = Math.max(1, table.header.length)
  const columnWidth = Math.floor(cx / columnCount)
  const grid = `<a:tblGrid>${table.header.map(() => `<a:gridCol w="${columnWidth}"/>`).join('')}</a:tblGrid>`
  const headerRow = `<a:tr h="370840">${table.header.map((cell) => tableCellXml(cell, true)).join('')}</a:tr>`
  const bodyRows = table.rows
    .map((row) => `<a:tr h="370840">${row.map((cell) => tableCellXml(cell, false)).join('')}</a:tr>`)
    .join('')
  return (
    `<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="${shapeId}" name="Table"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>` +
    `<p:xfrm><a:off x="${x}" y="${y}"/><a:ext cx="${cx}" cy="${cy}"/></p:xfrm>` +
    `<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">` +
    `<a:tbl><a:tblPr firstRow="1" bandRow="1"/>${grid}${headerRow}${bodyRows}</a:tbl>` +
    '</a:graphicData></a:graphic></p:graphicFrame>'
  )
}

function renderSlideXml(plan: SlidePlan): string {
  const runs = groupRuns(plan.items)
  const runCount = Math.max(1, runs.length)
  const runHeight = Math.floor(CONTENT_HEIGHT / runCount)
  const gap = 60000

  let shapeId = 1
  const bodyShapes = runs
    .map((run, index) => {
      shapeId += 1
      const y = CONTENT_TOP + runHeight * index
      const height = Math.max(200000, runHeight - gap)
      if (run.kind === 'table') return tableGraphicFrameXml(shapeId, run.item, MARGIN, y, CONTENT_WIDTH, height)
      return textRunShapeXml(shapeId, run.items, MARGIN, y, CONTENT_WIDTH, height)
    })
    .join('')

  shapeId += 1
  const titleXml = titleBoxXml(shapeId, plan.title)

  return (
    XML_DECLARATION +
    `<p:sld xmlns:a="${A_NS}" xmlns:r="${R_NS}" xmlns:p="${P_NS}">` +
    '<p:cSld><p:spTree>' +
    '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>' +
    '<p:grpSpPr/>' +
    titleXml +
    bodyShapes +
    '</p:spTree></p:cSld>' +
    '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>' +
    '</p:sld>'
  )
}

/** Slide 1: the answer's title, plus the export date as a subtitle line. */
function renderTitleSlideXml(meta: ExportMeta): string {
  const locale: ExportLocale = meta.locale ?? 'no'
  const date = formatExportDate(meta.createdAt, locale)
  return (
    XML_DECLARATION +
    `<p:sld xmlns:a="${A_NS}" xmlns:r="${R_NS}" xmlns:p="${P_NS}">` +
    '<p:cSld><p:spTree>' +
    '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>' +
    '<p:grpSpPr/>' +
    '<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>' +
    `<p:spPr>${shapeXfrm(MARGIN, 2286000, CONTENT_WIDTH, 1143000)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>` +
    '<p:txBody><a:bodyPr wrap="square" anchor="ctr" anchorCtr="1"/><a:lstStyle/>' +
    `<a:p><a:pPr algn="ctr"><a:buNone/></a:pPr>${textRunXml(meta.title, { bold: true, size: 4000 })}</a:p>` +
    '</p:txBody></p:sp>' +
    '<p:sp><p:nvSpPr><p:cNvPr id="3" name="Subtitle"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>' +
    `<p:spPr>${shapeXfrm(MARGIN, 3657600, CONTENT_WIDTH, 685800)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>` +
    '<p:txBody><a:bodyPr wrap="square" anchor="t"/><a:lstStyle/>' +
    `<a:p><a:pPr algn="ctr"><a:buNone/></a:pPr>${textRunXml(date, { size: 1800 })}</a:p>` +
    '</p:txBody></p:sp>' +
    '</p:spTree></p:cSld>' +
    '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>' +
    '</p:sld>'
  )
}

function buildPresentationXml(slideCount: number): string {
  const slideIds = Array.from({ length: slideCount }, (_, index) => `<p:sldId id="${256 + index}" r:id="rIdSlide${index + 1}"/>`).join('')
  return (
    XML_DECLARATION +
    `<p:presentation xmlns:a="${A_NS}" xmlns:r="${R_NS}" xmlns:p="${P_NS}">` +
    '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdMaster1"/></p:sldMasterIdLst>' +
    `<p:sldIdLst>${slideIds}</p:sldIdLst>` +
    `<p:sldSz cx="${SLIDE_WIDTH}" cy="${SLIDE_HEIGHT}" type="screen16x9"/>` +
    '<p:notesSz cx="6858000" cy="9144000"/>' +
    '</p:presentation>'
  )
}

function buildPresentationRelsXml(slideCount: number): string {
  const slideRels = Array.from(
    { length: slideCount },
    (_, index) => `<Relationship Id="rIdSlide${index + 1}" Type="${REL}/slide" Target="slides/slide${index + 1}.xml"/>`,
  ).join('')
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    `<Relationship Id="rIdMaster1" Type="${REL}/slideMaster" Target="slideMasters/slideMaster1.xml"/>` +
    slideRels +
    `<Relationship Id="rIdPresProps" Type="${REL}/presProps" Target="presProps.xml"/>` +
    `<Relationship Id="rIdViewProps" Type="${REL}/viewProps" Target="viewProps.xml"/>` +
    `<Relationship Id="rIdTableStyles" Type="${REL}/tableStyles" Target="tableStyles.xml"/>` +
    '</Relationships>'
  )
}

function buildSlideRelsXml(): string {
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    `<Relationship Id="rId1" Type="${REL}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>` +
    '</Relationships>'
  )
}

function buildSlideMasterXml(): string {
  return (
    XML_DECLARATION +
    `<p:sldMaster xmlns:a="${A_NS}" xmlns:r="${R_NS}" xmlns:p="${P_NS}">` +
    '<p:cSld><p:spTree>' +
    '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>' +
    '<p:grpSpPr/>' +
    '</p:spTree></p:cSld>' +
    '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>' +
    '<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>' +
    '<p:txStyles>' +
    '<p:titleStyle><a:lvl1pPr><a:defRPr sz="4400" b="1"/></a:lvl1pPr></p:titleStyle>' +
    '<p:bodyStyle><a:lvl1pPr><a:defRPr sz="2400"/></a:lvl1pPr></p:bodyStyle>' +
    '<p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle>' +
    '</p:txStyles>' +
    '</p:sldMaster>'
  )
}

function buildSlideMasterRelsXml(): string {
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    `<Relationship Id="rId1" Type="${REL}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>` +
    `<Relationship Id="rId2" Type="${REL}/theme" Target="../theme/theme1.xml"/>` +
    '</Relationships>'
  )
}

function buildSlideLayoutXml(): string {
  return (
    XML_DECLARATION +
    `<p:sldLayout xmlns:a="${A_NS}" xmlns:r="${R_NS}" xmlns:p="${P_NS}" type="blank" preserve="1">` +
    '<p:cSld name="Blank"><p:spTree>' +
    '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>' +
    '<p:grpSpPr/>' +
    '</p:spTree></p:cSld>' +
    '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>' +
    '</p:sldLayout>'
  )
}

function buildSlideLayoutRelsXml(): string {
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    `<Relationship Id="rId1" Type="${REL}/slideMaster" Target="../slideMasters/slideMaster1.xml"/>` +
    '</Relationships>'
  )
}

/** A complete theme, including the `fmtScheme` PowerPoint's importer insists on: at least three entries in each of `fillStyleLst`, `lnStyleLst`, `effectStyleLst` and `bgFillStyleLst`. */
function buildThemeXml(): string {
  return (
    XML_DECLARATION +
    `<a:theme xmlns:a="${A_NS}" name="Verevon">` +
    '<a:themeElements>' +
    '<a:clrScheme name="Verevon">' +
    '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>' +
    '<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>' +
    '<a:dk2><a:srgbClr val="1F2937"/></a:dk2>' +
    '<a:lt2><a:srgbClr val="F3F4F6"/></a:lt2>' +
    '<a:accent1><a:srgbClr val="2B5CD9"/></a:accent1>' +
    '<a:accent2><a:srgbClr val="9333EA"/></a:accent2>' +
    '<a:accent3><a:srgbClr val="059669"/></a:accent3>' +
    '<a:accent4><a:srgbClr val="D97706"/></a:accent4>' +
    '<a:accent5><a:srgbClr val="DC2626"/></a:accent5>' +
    '<a:accent6><a:srgbClr val="0891B2"/></a:accent6>' +
    '<a:hlink><a:srgbClr val="2B5CD9"/></a:hlink>' +
    '<a:folHlink><a:srgbClr val="9333EA"/></a:folHlink>' +
    '</a:clrScheme>' +
    '<a:fontScheme name="Verevon">' +
    '<a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>' +
    '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>' +
    '</a:fontScheme>' +
    '<a:fmtScheme name="Verevon">' +
    '<a:fillStyleLst>' +
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>' +
    '<a:solidFill><a:schemeClr val="phClr"><a:lumMod val="110000"/></a:schemeClr></a:solidFill>' +
    '<a:solidFill><a:schemeClr val="phClr"><a:lumMod val="105000"/></a:schemeClr></a:solidFill>' +
    '</a:fillStyleLst>' +
    '<a:lnStyleLst>' +
    '<a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>' +
    '<a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>' +
    '<a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>' +
    '</a:lnStyleLst>' +
    '<a:effectStyleLst>' +
    '<a:effectStyle><a:effectLst/></a:effectStyle>' +
    '<a:effectStyle><a:effectLst/></a:effectStyle>' +
    '<a:effectStyle><a:effectLst/></a:effectStyle>' +
    '</a:effectStyleLst>' +
    '<a:bgFillStyleLst>' +
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>' +
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>' +
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>' +
    '</a:bgFillStyleLst>' +
    '</a:fmtScheme>' +
    '</a:themeElements>' +
    '</a:theme>'
  )
}

function buildPresPropsXml(): string {
  return XML_DECLARATION + `<p:presentationPr xmlns:a="${A_NS}" xmlns:p="${P_NS}"/>`
}

function buildViewPropsXml(): string {
  return XML_DECLARATION + `<p:viewPr xmlns:a="${A_NS}" xmlns:p="${P_NS}"/>`
}

function buildTableStylesXml(): string {
  return XML_DECLARATION + `<a:tblStyleLst xmlns:a="${A_NS}" def="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"/>`
}

function buildContentTypesXml(slideCount: number): string {
  const slideOverrides = Array.from(
    { length: slideCount },
    (_, index) => `<Override PartName="/ppt/slides/slide${index + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>`,
  ).join('')
  return (
    XML_DECLARATION +
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
    '<Default Extension="xml" ContentType="application/xml"/>' +
    '<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>' +
    '<Override PartName="/ppt/presProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presProps+xml"/>' +
    '<Override PartName="/ppt/viewProps.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml"/>' +
    '<Override PartName="/ppt/tableStyles.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml"/>' +
    '<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>' +
    '<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>' +
    '<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>' +
    slideOverrides +
    '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>' +
    '<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>' +
    '</Types>'
  )
}

/** Builds a complete, valid PresentationML (.pptx) deck for one finished chat answer. */
export function buildPptx(blocks: readonly MarkdownBlock[], meta: ExportMeta): Uint8Array {
  const contentSlides = planSlides(blocks, meta)
  const slideCount = contentSlides.length + 1 // + the title slide

  const entries: ZipEntry[] = [
    { name: '[Content_Types].xml', data: encodeUtf8(buildContentTypesXml(slideCount)) },
    { name: '_rels/.rels', data: encodeUtf8(buildPackageRelsXml('ppt/presentation.xml')) },
    { name: 'ppt/presentation.xml', data: encodeUtf8(buildPresentationXml(slideCount)) },
    { name: 'ppt/_rels/presentation.xml.rels', data: encodeUtf8(buildPresentationRelsXml(slideCount)) },
    { name: 'ppt/presProps.xml', data: encodeUtf8(buildPresPropsXml()) },
    { name: 'ppt/viewProps.xml', data: encodeUtf8(buildViewPropsXml()) },
    { name: 'ppt/tableStyles.xml', data: encodeUtf8(buildTableStylesXml()) },
    { name: 'ppt/theme/theme1.xml', data: encodeUtf8(buildThemeXml()) },
    { name: 'ppt/slideMasters/slideMaster1.xml', data: encodeUtf8(buildSlideMasterXml()) },
    { name: 'ppt/slideMasters/_rels/slideMaster1.xml.rels', data: encodeUtf8(buildSlideMasterRelsXml()) },
    { name: 'ppt/slideLayouts/slideLayout1.xml', data: encodeUtf8(buildSlideLayoutXml()) },
    { name: 'ppt/slideLayouts/_rels/slideLayout1.xml.rels', data: encodeUtf8(buildSlideLayoutRelsXml()) },
    { name: 'docProps/core.xml', data: encodeUtf8(buildCorePropsXml(meta.title, meta.createdAt)) },
    { name: 'docProps/app.xml', data: encodeUtf8(buildAppPropsXml()) },
    { name: 'ppt/slides/slide1.xml', data: encodeUtf8(renderTitleSlideXml(meta)) },
    { name: 'ppt/slides/_rels/slide1.xml.rels', data: encodeUtf8(buildSlideRelsXml()) },
  ]
  contentSlides.forEach((plan, index) => {
    const slideNumber = index + 2
    entries.push({ name: `ppt/slides/slide${slideNumber}.xml`, data: encodeUtf8(renderSlideXml(plan)) })
    entries.push({ name: `ppt/slides/_rels/slide${slideNumber}.xml.rels`, data: encodeUtf8(buildSlideRelsXml()) })
  })
  return buildZip(entries)
}
