// A date in a project-plan table is not an approved milestone merely because
// its status says "ikke vedtatt" or is conditional on a schema being approved.
// Inspect the status cell, not the whole row.
export function unsupportedDatedApprovalRows(text: string): string[] {
  const failures: string[] = []
  let statusColumn = -1
  for (const line of text.split(/\r?\n/)) {
    if (!/^\s*\|/.test(line)) { statusColumn = -1; continue }
    const cells = line.trim().replace(/^\||\|$/g, '').split('|').map(cell => cell.trim().replace(/[*`]/g, ''))
    const headerStatus = cells.findIndex(cell => /\b(?:status|vedtak|approval)\b/i.test(cell))
    if (headerStatus >= 0) { statusColumn = headerStatus; continue }
    if (statusColumn < 0 || /^:?-{3,}:?$/.test(cells[statusColumn] ?? '')) continue
    if (!cells.some(cell => /\b(?:16|30)\.\s*sep(?:tember)?\b/i.test(cell))) continue
    const status = cells[statusColumn] ?? ''
    for (const match of status.matchAll(/\b(?:vedtatt|godkjent|approved)\b/gi)) {
      const before = status.slice(0, match.index)
      const negated = /\b(?:ikke|ei|not)\b(?:\s+\S+){0,3}\s*$/i.test(before)
      const conditional = /\b(?:betinget av|avhengig av|forutsatt|hvis|dersom|når|if|when)\b[^,;.!?]{0,80}$/i.test(before)
      if (!negated && !conditional) {
        failures.push(line)
        break
      }
    }
  }
  return failures
}
