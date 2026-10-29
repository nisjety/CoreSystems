import { describe, expect, it } from 'vitest'
import { toCalendar, toCsv } from '@/features/anbud/lib/export'

describe('tender export', () => {
  it('writes CSV with semicolons, a byte order mark, and quotes only what needs them', () => {
    const csv = toCsv([
      {
        id: '2026-100001',
        title: 'Åpen drift; "særskilt" lot',
        buyer: 'Eksempelkommune',
        deadline: '2026-11-30',
        plannedNotice: '',
        cpv: '72212000',
        nuts: 'NO081',
        stage: 'Vurderes',
      },
    ])
    expect(csv.startsWith('\uFEFF')).toBe(true)
    const lines = csv.slice(1).split('\r\n')
    expect(lines[0]).toBe('Id;Tittel;Innkjøper;Frist;Planlagt utlysning;CPV;Region;Pipeline-status')
    expect(lines[1]).toBe('2026-100001;"Åpen drift; ""særskilt"" lot";Eksempelkommune;2026-11-30;;72212000;NO081;Vurderes')
  })

  it('writes one all-day event per deadline, with iCalendar escapes and folded long lines', () => {
    const stamp = new Date('2026-10-28T18:05:09.123Z')
    const ics = toCalendar(
      [
        { uid: '2026-100001@anbud', day: '2026-11-30', summary: 'Anbudsfrist: Åpen drift, Oslo; lot', description: 'Kommune\nNO081' },
        { uid: '2026-100002@anbud', day: 'not a day', summary: 'skipped', description: '' },
        { uid: 'long@anbud', day: '2026-12-01', summary: 'x'.repeat(90), description: '' },
      ],
      stamp,
    )
    expect(ics.startsWith('BEGIN:VCALENDAR\r\n')).toBe(true)
    expect(ics).toContain('DTSTAMP:20261028T180509Z\r\n')
    expect(ics).toContain('DTSTART;VALUE=DATE:20261130\r\n')
    expect(ics).toContain('SUMMARY:Anbudsfrist: Åpen drift\\, Oslo\; lot\r\n')
    expect(ics).toContain('DESCRIPTION:Kommune\\nNO081\r\n')
    expect(ics).not.toContain('skipped')
    expect(ics.split('BEGIN:VEVENT').length - 1).toBe(2)
    const folded = ics.split('\r\n').filter((line) => line.startsWith(' '))
    expect(folded.length).toBeGreaterThan(0)
    expect(ics.split('\r\n').every((line) => line.length <= 75)).toBe(true)
  })
})
