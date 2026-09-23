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
  const groupPattern = groups.map(group => group.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')
  const scopePattern = new RegExp(`\\b(?:${groupPattern}|totalt?|samlet)\\b`, 'giu')
  const claims = []
  for (const sentence of plain.split(/\n|(?<=[.!?])\s+/)) {
    for (const metric of sentence.matchAll(/\bbruttofortjenesten?\b/giu)) {
      const start = metric.index + metric[0].length
      // A later sentence or metric cannot supply the amount for this claim.
      // In particular, a profit change followed by revenue growth is not one
      // long claim just because both happen to occur on the same Markdown line.
      const tail = sentence.slice(start).split(/\b(?:bruttofortjenesten?|omsetning(?:en)?|bruttomargin(?:en)?|margin(?:en)?|varekost(?:naden)?)\b/iu)[0]
      const parenthesized = /\(([+−-]?[\d .,\u00a0\u202f]+)\s*(?:NOK|kr|kroner)\b(?:,\s*([+−-]?[\d.,]+)\s*%)?/iu.exec(tail)
      const direct = /(?<![\p{L}\p{N}_])(økte|steg|vokste|falt|sank)\s+(?:med\s+)?([+−-]?[ \t]*\d[\d .,\u00a0\u202f]*?)\s*(?:NOK|kr|kroner)\b(?:\s*\(([+−-]?[\d.,]+)\s*%)?/iu.exec(tail)
      if (!parenthesized && !direct) continue
      const rawAmount = parenthesized?.[1] ?? direct![2]
      const falling = !parenthesized && /^(?:falt|sank)$/i.test(direct![1])
      const direction = falling && !/^[+−-]/.test(rawAmount.trim()) ? -1 : 1
      const amount = direction * Number(rawAmount.replace(/[ \u00a0\u202f]/g, '').replace(',', '.').replace('−', '-'))
      const rawPercentage = parenthesized?.[2] ?? direct?.[3]
      const percentage = rawPercentage && falling && !/^[+−-]/.test(rawPercentage) ? `-${rawPercentage}` : rawPercentage
      // Scope belongs to this sentence and this occurrence of the metric.
      const prefix = sentence.slice(0, start + (parenthesized?.index ?? direct!.index))
      const scope = [...prefix.matchAll(scopePattern)].at(-1)?.[0]
      const group = groups.find(group => group.toLowerCase() === scope?.toLowerCase()) ?? 'Totalt'
      claims.push({ group, amount, percentage })
    }
  }
  return claims
}
