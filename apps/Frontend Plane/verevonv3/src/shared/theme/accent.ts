/**
 * Organisation accent (F-t3-098), solved in OKLCH so that it can be read on both surfaces.
 *
 * `solveAccent` keeps the hue of the organisation's colour and, where sRGB allows, its chroma. It
 * moves the lightness to the value nearest the original at which the accent reaches the contrast
 * target against the light and the dark surface, and it picks the surface that reads best on the
 * accent for `onAccent`. When no lightness reaches the target it returns null, and nothing is
 * applied: the accent fails closed. With the surfaces below no accent reaches 4.5:1 on both (the best
 * is about 4.23:1), so the solver returns null until the target or a surface changes.
 *
 * `applyOrgAccent` sets `--org-accent` and `--org-accent-on` on the document root. It sets them only
 * when `VITE_ORG_ACCENT` is the literal `true` and the organisation has a colour. In every other case
 * it removes them, so one organisation's accent never lingers after a switch.
 *
 * This file is the only place colour arithmetic and hex literals belong. The surface constants mirror
 * `--verevon-surface` and `--verevon-dark-surface` in `src/styles/global.css`; change them together.
 */

/** WCAG 2.x contrast the accent must reach against both surfaces (SC 1.4.3, text). */
export const ORG_ACCENT_MIN_CONTRAST = 4.5

/** `--verevon-surface`, the light surface. */
const LIGHT_SURFACE = '#ffffff'
/** `--verevon-dark-surface`, the dark surface. */
const DARK_SURFACE = '#171717'

const ACCENT_PROPERTY = '--org-accent'
const ON_ACCENT_PROPERTY = '--org-accent-on'

const HEX_COLOR = /^#?([0-9a-f]{6}|[0-9a-f]{3})$/i
const LIGHTNESS_STEPS = 1000
const CHROMA_SEARCH_STEPS = 24
const GAMUT_TOLERANCE = 1e-6

/** Three channels: linear light for the colour maths. */
type Triple = readonly [number, number, number]

export interface AccentPair {
  /** The accent, as `#rrggbb`. */
  readonly accent: string
  /** The colour for content drawn on `accent`, as `#rrggbb`: one of the two surfaces. */
  readonly onAccent: string
}

const clamp01 = (value: number): number => Math.min(1, Math.max(0, value))

/** The sRGB transfer function: one encoded channel to linear light. */
function decodeSrgb(encoded: number): number {
  return encoded <= 0.04045 ? encoded / 12.92 : ((encoded + 0.055) / 1.055) ** 2.4
}

/** The inverse transfer function, for a linear channel already clamped to 0..1. */
function encodeSrgb(linear: number): number {
  return linear <= 0.0031308 ? 12.92 * linear : 1.055 * linear ** (1 / 2.4) - 0.055
}

/** An 8-bit channel, from linear light. */
function toByte(linear: number): number {
  return Math.round(encodeSrgb(clamp01(linear)) * 255)
}

/** Linear light of an 8-bit channel: the value a hex string actually carries. */
function fromByte(byte: number): number {
  return decodeSrgb(byte / 255)
}

/** Linear-light channels of `#rgb` or `#rrggbb`, or null for anything else. */
function parseHex(input: string): Triple | null {
  const raw = HEX_COLOR.exec(input.trim())?.[1]
  if (raw === undefined) return null
  const digits = raw.length === 3 ? raw.replace(/./g, '$&$&') : raw
  const channel = (offset: number): number => fromByte(Number.parseInt(digits.slice(offset, offset + 2), 16))
  return [channel(0), channel(2), channel(4)]
}

/** WCAG 2.x relative luminance of linear-light channels. */
function luminance([red, green, blue]: Triple): number {
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue
}

/** WCAG 2.x contrast ratio between two relative luminances. */
function contrast(first: number, second: number): number {
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05)
}

/** Linear light to OKLab, with Björn Ottosson's matrices. */
function toOklab([red, green, blue]: Triple): Triple {
  const l = Math.cbrt(0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue)
  const m = Math.cbrt(0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue)
  const s = Math.cbrt(0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

/** OKLCH (hue in radians) to linear light. The result may lie outside sRGB. */
function fromOklch(lightness: number, chroma: number, hue: number): Triple {
  const a = chroma * Math.cos(hue)
  const b = chroma * Math.sin(hue)
  const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

function inGamut(linear: Triple): boolean {
  return linear.every((channel) => channel >= -GAMUT_TOLERANCE && channel <= 1 + GAMUT_TOLERANCE)
}

/** The colour at this lightness and hue with the most chroma, up to `chroma`, that sRGB can show. */
function fitToGamut(lightness: number, chroma: number, hue: number): Triple {
  const at = (value: number): Triple => fromOklch(lightness, value, hue)
  if (inGamut(at(chroma))) return at(chroma)
  let low = 0
  let high = chroma
  for (let step = 0; step < CHROMA_SEARCH_STEPS; step += 1) {
    const middle = (low + high) / 2
    if (inGamut(at(middle))) low = middle
    else high = middle
  }
  return at(low)
}

/** The linear light that an 8-bit hex string would carry for these channels. */
function quantise([red, green, blue]: Triple): Triple {
  return [fromByte(toByte(red)), fromByte(toByte(green)), fromByte(toByte(blue))]
}

function toHex([red, green, blue]: Triple): string {
  return `#${[red, green, blue].map((channel) => toByte(channel).toString(16).padStart(2, '0')).join('')}`
}

function surfaceLuminance(hex: string): number {
  const linear = parseHex(hex)
  return linear === null ? 0 : luminance(linear)
}

/** The surfaces the accent must clear, each with its luminance; they are also the two colours that can sit on it. */
const SURFACES = [
  { hex: LIGHT_SURFACE, luminance: surfaceLuminance(LIGHT_SURFACE) },
  { hex: DARK_SURFACE, luminance: surfaceLuminance(DARK_SURFACE) },
]

/** The surface with the best contrast against `y`, when that contrast reaches `minimum`. */
function onAccentFor(y: number, minimum: number): string | null {
  const options = SURFACES.map((surface) => ({ hex: surface.hex, ratio: contrast(y, surface.luminance) }))
    .filter((option) => option.ratio >= minimum)
    .sort((first, second) => second.ratio - first.ratio)
  return options[0]?.hex ?? null
}

/** Every lightness from 0 to 1 in small steps, the one nearest `origin` first. */
function lightnessByDistance(origin: number): number[] {
  const steps = Array.from({ length: LIGHTNESS_STEPS + 1 }, (_, index) => index / LIGHTNESS_STEPS)
  return steps.sort((first, second) => Math.abs(first - origin) - Math.abs(second - origin))
}

/**
 * The accent and its `onAccent` for an organisation colour, or null when no lightness of its hue
 * reaches `minimum` against both surfaces. The default target is {@link ORG_ACCENT_MIN_CONTRAST}.
 */
export function solveAccent(hex: string, minimum = ORG_ACCENT_MIN_CONTRAST): AccentPair | null {
  const source = parseHex(hex)
  if (source === null) return null
  const [lightness, a, b] = toOklab(source)
  const chroma = Math.hypot(a, b)
  const hue = Math.atan2(b, a)
  for (const candidate of lightnessByDistance(lightness)) {
    const linear = quantise(fitToGamut(candidate, chroma, hue))
    const y = luminance(linear)
    if (!SURFACES.every((surface) => contrast(y, surface.luminance) >= minimum)) continue
    const onAccent = onAccentFor(y, minimum)
    if (onAccent !== null) return { accent: toHex(linear), onAccent }
  }
  return null
}

/**
 * Write this organisation's accent to the document root, or remove it. Nothing is written unless
 * `VITE_ORG_ACCENT` is the literal `true`. Without a colour, or with one that cannot be solved, the
 * two properties are removed.
 */
export function applyOrgAccent(hex: string | null | undefined): void {
  if (typeof document === 'undefined') return
  const solved = import.meta.env.VITE_ORG_ACCENT === 'true' && hex ? solveAccent(hex) : null
  const style = document.documentElement.style
  if (solved === null) {
    style.removeProperty(ACCENT_PROPERTY)
    style.removeProperty(ON_ACCENT_PROPERTY)
    return
  }
  style.setProperty(ACCENT_PROPERTY, solved.accent)
  style.setProperty(ON_ACCENT_PROPERTY, solved.onAccent)
}
