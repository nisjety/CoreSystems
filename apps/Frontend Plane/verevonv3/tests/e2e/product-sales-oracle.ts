// Judge rounding at the precision actually displayed, without a tolerance that
// would allow an incorrect final digit. The brief does not prescribe decimals.
export function containsRoundedNumber(text: string, expected: number): boolean {
  if (!Number.isFinite(expected)) return false
  const normalized = text
    .replace(/(?<=\d)[ \u00a0\u202f](?=\d{3}(?:\D|$))/g, '')
    .replace(/,/g, '.')
    .replace(/−/g, '-')
  const numbers = normalized.matchAll(/(?<![\p{L}\p{N}_.+\-])([+\-]?[ \t]*\d+(?:\.\d+)?)(?![\p{L}\p{N}_]|\.\d)/gu)
  for (const [token] of numbers) {
    const value = Number(token.replace(/[ \t]/g, ''))
    const precision = token.split('.')[1]?.length ?? 0
    // Whole percentages are fine for exact integer margins. Fractional margins
    // still need at least one decimal, as the original acceptance check required.
    if (precision === 0 ? value === expected : precision <= 15 && value === Number(expected.toFixed(precision))) return true
  }
  return false
}

export function grossProfitChangeClaims(text: string, groups: readonly string[]) {
  const plain = text.replace(/[*`]/g, '')
  const escape = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const groupPattern = groups.map(escape).join('|')
  // A scope may be named in its definite form ("Nettbutikken",
  // "Bedriftssalget", "Belysningen"), never as part of a longer compound
  // ("Belysningskampanjen").
  const definite = '(?:en|et|a|ene)?'
  const scopePattern = new RegExp(`\\b(?:(?:${groupPattern})${definite}|totalt?|samlet)\\b`, 'giu')
  const scopeOf = (named: string | undefined) =>
    groups.find(group => named !== undefined && new RegExp(`^${escape(group)}${definite}$`, 'iu').test(named))
  // The scope a claim names: the last one before its amount, except a group
  // that is only the object of a comparison ("Belysning økte mer enn
  // Oppbevaring, med … 12 000 kr i bruttofortjeneste" is Belysning's claim).
  const lastScope = (prefix: string) => [...prefix.matchAll(scopePattern)]
    .filter(match => !/(?:\benn|\bsammenlignet med|\bi forhold til|\bmot)\s+$/iu.test(prefix.slice(0, match.index)))
    .at(-1)?.[0]
  const AMOUNT = '[+−-]?[ \\t]*\\d[\\d .\\u00a0\\u202f]*?(?:,\\d+)?'
  // "18 720", "18.720" and "18 720,00" are one amount; "1,5" is a decimal.
  const kroner = (raw: string) => Number(raw.replace(/[ \t\u00a0\u202f]/g, '').replace(/\.(?=\d{3}(?!\d))/g, '').replace(',', '.').replace('−', '-'))
  const RISING = 'økte|steg|vokste|gikk opp'
  const FALLING = 'falt|sank|minsket|reduserte|gikk ned|svekket'
  const verbPattern = new RegExp(`(?<![\\p{L}\\p{N}_])(${RISING}|${FALLING})(?![\\p{L}\\p{N}_])`, 'giu')
  const falls = (verb: string) => new RegExp(`^(?:${FALLING})$`, 'iu').test(verb)
  const METRIC_WORDS = /\b(?:bruttofortjenesten?|omsetning(?:en)?|bruttomargin(?:en)?|margin(?:en)?|varekost(?:naden)?)\b/iu
  const claims: Array<{ group: string; amount: number; percentage: string | undefined; scope: 'explicit' | 'inherited' | 'default' }> = []
  for (const line of plain.split('\n')) {
    // "Skrivebord sto for hele nettofallet. Bruttofortjenesten falt 18 720 kr"
    // names its group one sentence earlier in the same bullet or paragraph.
    let lineScope: string | undefined
    for (const sentence of line.split(/(?<=[.!?])\s+/)) {
      for (const metric of sentence.matchAll(/\bbruttofortjenesten?\b/giu)) {
        const start = metric.index + metric[0].length
        const push = (amount: number, percentage: string | undefined, prefix: string) => {
          const explicit = lastScope(prefix)
          const named = explicit ?? lineScope
          claims.push({ group: scopeOf(named) ?? 'Totalt', amount, percentage, scope: explicit ? 'explicit' : lineScope ? 'inherited' : 'default' })
        }
        // An amount stated BEFORE the metric, "… 12 000 kr i
        // bruttofortjeneste" (Q16b 02 take 7), is a change claim when:
        // - it is the end of a span, "fra 59 000 kr til 53 320 kr i
        //   bruttofortjeneste" (take 17), so the claim is the difference;
        // - its clause has a change verb, which gives its sign; or
        // - it carries its own sign, "Skrivebord: −18 720 kr i
        //   bruttofortjeneste".
        // The text after the metric then belongs to another claim. Otherwise
        // the amount is a level ("uken ga 124 680 kr i bruttofortjeneste",
        // take 9), and the claim, if any, follows the metric as before.
        const before = sentence.slice(0, metric.index)
        const leading = new RegExp(`(${AMOUNT})\\s*(?:NOK|kr|kroner)\\s+i\\s+$`, 'iu').exec(before)
        if (leading) {
          const ahead = before.slice(0, leading.index)
          const span = new RegExp(`fra\\s+(${AMOUNT})\\s*(?:NOK|kr|kroner)?\\s+til\\s*$`, 'iu').exec(ahead)
          const clause = ahead.split(/;|,\s+(?:og|mens|men)\b/iu).at(-1) ?? ''
          const verb = [...clause.matchAll(verbPattern)].at(-1)?.[1]
          const raw = leading[1].trim()
          const signed = /^[+−-]/.test(raw)
          if (span || verb || signed) {
            const amount = span ? kroner(raw) - kroner(span[1]) : (verb && falls(verb) && !signed ? -1 : 1) * kroner(raw)
            push(amount, undefined, ahead.slice(0, span?.index ?? ahead.length))
            continue
          }
        }
        // A later sentence or metric cannot supply the amount for this claim:
        // a profit change followed by revenue growth is not one long claim
        // just because both occur on the same Markdown line. Nor can another
        // scope's clause: after ";" or ", og" / ", mens" / ", men", the
        // amount belongs to the scope named there ("…, og Oppbevaring økte
        // med 24 000 kr"). A clause that names no other scope still counts.
        // An elliptical amount with no metric word ("og 8 400 kr") is not
        // read as a claim.
        let tail = sentence.slice(start).split(METRIC_WORDS)[0]
        for (const boundary of tail.matchAll(/;|,\s+(?:og|mens|men)\b/giu)) {
          const next = tail.slice(boundary.index + boundary[0].length)
          const nextAmount = new RegExp(`\\d`).exec(next)?.index ?? next.length
          if (new RegExp(scopePattern.source, 'iu').test(next.slice(0, nextAmount))) {
            tail = tail.slice(0, boundary.index)
            break
          }
        }
        const parenthesized = /\(([+−-]?[\d .,\u00a0\u202f]+)\s*(?:NOK|kr|kroner)\b(?:[,;]\s*([+−-]?[\d.,]+)\s*%)?/iu.exec(tail)
        const direct = new RegExp(`(?<![\\p{L}\\p{N}_])(${RISING}|${FALLING})\\s+(?:med\\s+)?(${AMOUNT})\\s*(?:NOK|kr|kroner)\\b(?:\\s*\\(([+−-]?[\\d.,]+)\\s*%)?`, 'iu').exec(tail)
        const span = new RegExp(`^\\s*fra\\s+(${AMOUNT})\\s*(?:NOK|kr|kroner)?\\s+til\\s+(${AMOUNT})\\s*(?:NOK|kr|kroner)\\b`, 'iu').exec(tail)
        if (parenthesized || direct) {
          const rawAmount = parenthesized?.[1] ?? direct![2]
          const falling = !parenthesized && falls(direct![1])
          const direction = falling && !/^[+−-]/.test(rawAmount.trim()) ? -1 : 1
          const rawPercentage = parenthesized?.[2] ?? direct?.[3]
          const percentage = rawPercentage && falling && !/^[+−-]/.test(rawPercentage) ? `-${rawPercentage}` : rawPercentage
          // Scope belongs to this sentence and this occurrence of the metric;
          // failing that, to a group named earlier in the same line.
          push(direction * kroner(rawAmount), percentage, sentence.slice(0, start + (parenthesized?.index ?? direct!.index)))
        } else if (span) {
          // "bruttofortjenesten fra 64 000 kr til 71 360 kr": the difference.
          push(kroner(span[2]) - kroner(span[1]), undefined, sentence.slice(0, start))
        }
      }
      lineScope = lastScope(sentence) ?? lineScope
    }
  }
  return claims
}

/**
 * An explicit or unscoped claim must equal its own scope's change (an
 * unscoped claim is the total, as before). A claim whose group was named in
 * an earlier sentence of the same line may equal that group's change or the
 * total's, since the prose can return to the total after naming a group; any
 * other figure still fails.
 */
export function acceptableProfitChange(
  claim: { group: string; amount: number; scope: 'explicit' | 'inherited' | 'default' },
  changes: Readonly<Record<string, number>>,
) {
  if (changes[claim.group] === claim.amount) return true
  return claim.scope === 'inherited' && changes.Totalt === claim.amount
}
