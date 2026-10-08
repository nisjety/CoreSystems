/**
 * A chart's on-screen SVG as a file that looks the same anywhere.
 *
 * The chart is drawn with CSS classes and custom properties (`--chart-s1`,
 * theme pairs for dark mode), none of which exist once the SVG is saved on its
 * own. This clones the live SVG, writes the resolved colours and text style
 * onto every element as inline style, drops the invisible hover and focus
 * targets, and puts the chart surface behind it. DOM-dependent (it reads
 * computed style), so it is not part of the pure `spec.ts` / `geometry.ts`.
 */

const INLINE_PROPERTIES = [
  'fill',
  'fill-opacity',
  'stroke',
  'stroke-width',
  'stroke-linecap',
  'stroke-linejoin',
  'opacity',
  'font-family',
  'font-size',
  'font-weight',
  'text-anchor',
  'dominant-baseline',
] as const

const SVG_NS = 'http://www.w3.org/2000/svg'

/** `omsetning per kvartal` -> `omsetning-per-kvartal`; `diagram` when nothing usable is left. */
export function chartFileName(title: string, extension: string): string {
  const slug = title
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80)
  return `${slug || 'diagram'}.${extension}`
}

export type ChartSvgOptions = {
  /** The chart surface colour, resolved (not a `var()`); empty leaves the file transparent. */
  background: string
  /** The chart's visible title lives outside the SVG on screen; when given, it is drawn above the plot in the file. */
  title?: string
  /** Resolved ink colour for the title. */
  titleColor?: string
}

const TITLE_BAND = 30

/** The standalone SVG text for `svg`, see the file header. */
export function serializeChartSvg(svg: SVGSVGElement, options: ChartSvgOptions): string {
  const clone = svg.cloneNode(true) as SVGSVGElement
  const sourceNodes = [svg, ...Array.from(svg.querySelectorAll('*'))]
  const cloneNodes = [clone, ...Array.from(clone.querySelectorAll('*'))]

  sourceNodes.forEach((source, index) => {
    const target = cloneNodes[index]
    if (!target || !(target instanceof Element)) return
    const computed = window.getComputedStyle(source)
    for (const property of INLINE_PROPERTIES) {
      const value = computed.getPropertyValue(property)
      if (value) (target as SVGElement).style.setProperty(property, value)
    }
  })

  for (const skip of Array.from(clone.querySelectorAll('[data-chart-hit]'))) skip.remove()
  for (const element of [clone, ...Array.from(clone.querySelectorAll('*'))]) {
    element.removeAttribute('tabindex')
    element.removeAttribute('class')
  }

  clone.setAttribute('xmlns', SVG_NS)
  const viewBox = clone.getAttribute('viewBox')?.split(/\s+/).map(Number) ?? []
  const [, , width, height] = viewBox
  if (width && height) {
    clone.setAttribute('width', String(width))
    clone.setAttribute('height', String(height))
  }
  clone.removeAttribute('style')

  const isDrawn = (child: Element) => child.tagName !== 'title' && child.tagName !== 'desc'
  if (options.title) {
    const document = clone.ownerDocument
    const group = document.createElementNS(SVG_NS, 'g')
    group.setAttribute('transform', `translate(0 ${TITLE_BAND})`)
    for (const child of Array.from(clone.children).filter(isDrawn)) group.appendChild(child)
    const heading = document.createElementNS(SVG_NS, 'text')
    heading.setAttribute('x', '2')
    heading.setAttribute('y', '18')
    heading.setAttribute('font-size', '13')
    heading.setAttribute('font-weight', '650')
    heading.setAttribute('font-family', window.getComputedStyle(svg).fontFamily || 'sans-serif')
    if (options.titleColor) heading.setAttribute('fill', options.titleColor)
    heading.textContent = options.title
    clone.appendChild(heading)
    clone.appendChild(group)
    if (width && height) {
      clone.setAttribute('viewBox', `0 0 ${width} ${height + TITLE_BAND}`)
      clone.setAttribute('height', String(height + TITLE_BAND))
    }
  }

  if (options.background) {
    const rect = clone.ownerDocument.createElementNS(SVG_NS, 'rect')
    rect.setAttribute('width', '100%')
    rect.setAttribute('height', '100%')
    rect.setAttribute('fill', options.background)
    const firstDrawn = Array.from(clone.children).find(isDrawn)
    clone.insertBefore(rect, firstDrawn ?? null)
  }

  return `<?xml version="1.0" encoding="UTF-8"?>\n${new XMLSerializer().serializeToString(clone)}`
}
