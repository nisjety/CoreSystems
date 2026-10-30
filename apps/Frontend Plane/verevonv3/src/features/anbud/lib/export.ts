/** One tender as the export writes it: the words the page shows, not the codes behind them. */
export type ExportRow = {
  id: string
  title: string
  buyer: string
  deadline: string
  plannedNotice: string
  cpv: string
  nuts: string
  stage: string
}

const csvHeader = ['Id', 'Tittel', 'Innkjøper', 'Frist', 'Planlagt utlysning', 'CPV', 'Region', 'Pipeline-status']

/** A cell in semicolon-separated CSV, which Norwegian spreadsheets read as separate columns. */
function csvCell(value: string): string {
  return /[";\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value
}

/** The rows as CSV. A byte order mark first, so a spreadsheet reads the Norwegian letters as UTF-8. */
export function toCsv(rows: ExportRow[]): string {
  const lines = [
    csvHeader,
    ...rows.map((row) => [row.id, row.title, row.buyer, row.deadline, row.plannedNotice, row.cpv, row.nuts, row.stage]),
  ]
  return '\uFEFF' + lines.map((line) => line.map(csvCell).join(';')).join('\r\n') + '\r\n'
}

export type CalendarDeadline = { uid: string; day: string; summary: string; description: string }

/** iCalendar text escapes a backslash, a semicolon, a comma and a line break. */
function icsText(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/;/g, '\;').replace(/,/g, '\\,').replace(/\r?\n/g, '\\n')
}

/** Lines are folded at 75 characters, as iCalendar asks: a continuation line starts with a space. */
function fold(line: string): string {
  const parts: string[] = []
  let rest = line
  while (rest.length > 75) {
    parts.push(rest.slice(0, 75))
    rest = rest.slice(75)
  }
  parts.push(rest)
  return parts.join('\r\n ')
}

/**
 * One all-day event per deadline, on the day the tender closes. The day is a Norwegian calendar day, as the core
 * reads it, so it is written as a date with no time and no zone.
 */
export function toCalendar(deadlines: CalendarDeadline[], stamp: Date): string {
  const dtstamp = stamp.toISOString().replace(/[-:]/g, '').replace(/\.\d{3}/, '')
  const events = deadlines
    .filter((item) => /^\d{4}-\d{2}-\d{2}$/.test(item.day))
    .flatMap((item) => [
      'BEGIN:VEVENT',
      `UID:${item.uid}`,
      `DTSTAMP:${dtstamp}`,
      `DTSTART;VALUE=DATE:${item.day.replace(/-/g, '')}`,
      `SUMMARY:${icsText(item.summary)}`,
      `DESCRIPTION:${icsText(item.description)}`,
      'END:VEVENT',
    ])
  return [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//Triodelab//Anbud//NO',
    'CALSCALE:GREGORIAN',
    'METHOD:PUBLISH',
    ...events,
    'END:VCALENDAR',
  ].map(fold).join('\r\n') + '\r\n'
}

/** Saves text as a file the browser downloads. */
export function downloadFile(name: string, type: string, content: string): void {
  const url = URL.createObjectURL(new Blob([content], { type }))
  const link = document.createElement('a')
  link.href = url
  link.download = name
  link.click()
  URL.revokeObjectURL(url)
}
