import { describe, expect, it } from 'vitest'
import { unsupportedDatedApprovalRows } from './product-project-oracle'

describe('project milestone status oracle', () => {
  it('accepts the source-qualified status from the approved diagnostic document', () => {
    const table = '| Dato | Status | Betydning |\n|---|---|---|\n| 16. september | Mulig dato, ikke vedtatt | Skjemaet kan godkjennes. |\n| 30. september | Mål-/pilotdato | Prosjektleder tar beslutning. |'
    expect(unsupportedDatedApprovalRows(table)).toEqual([])
  })

  it('rejects an invented approval in a status cell, even if another cell negates it', () => {
    const row = '| 16. september | Vedtatt | Ikke vedtatt i kilden. |'
    const table = '| Dato | Status | Betydning |\n|---|---|---|\n' + row
    expect(unsupportedDatedApprovalRows(table)).toEqual([row])
  })

  it('accepts a proposed export date conditional on later schema approval', () => {
    const row = '| Utvikle og kontrollere eksport | Teknisk ansvarlig | 17.–18. september | Skjema | Et testuttrekk samsvarer med godkjent skjema. | *Forslag*, betinget av skjema godkjent 16. september |'
    const table = '| Oppgave | Rolle | Frist | Avhengighet | Ferdigkriterium | Status |\n|---|---|---|---|---|---|\n' + row
    expect(unsupportedDatedApprovalRows(table)).toEqual([])
  })

  it('still rejects an affirmative approval followed by a condition', () => {
    const row = '| 16. september | Vedtatt, betinget av skjema godkjent 16. september |'
    const table = '| Dato | Status |\n|---|---|\n' + row
    expect(unsupportedDatedApprovalRows(table)).toEqual([row])
  })
})
