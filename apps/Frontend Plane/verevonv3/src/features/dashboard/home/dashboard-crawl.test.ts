import { describe, expect, it } from 'vitest'
import type { RunItem, ScheduleItem } from '@/shared/api/ingestions-client'
import { crawlCardRows, crawlDateLabel, crawlKindLabel, crawlMetaLine, crawlStatusLabel, latestCrawlRuns, upcomingCrawlSchedules } from './dashboard-crawl'

const no = (nb: string) => nb
const en = (_nb: string, english: string) => english
const run = (id: string, createdAt: string) => ({ id, kind: 'crawl', status: 'completed', createdAt, target: id, progress: {}, stats: {} }) as RunItem
const schedule = (id: string, nextRunAt: string | null | undefined, createdAt = '2026-09-01T12:00:00Z') => ({ id, name: id, kind: 'crawl', status: 'active', createdAt, nextRunAt, target: id, config: {} }) as ScheduleItem
const ids = (items: Array<{ id: string }>) => items.map((item) => item.id)

describe('latestCrawlRuns', () => {
  it('lists the newest runs first and keeps at most four', () => {
    const runs = ['a', 'b', 'c', 'd', 'e', 'f'].map((id, index) => run(id, `2026-09-0${index + 1}T12:00:00Z`))
    expect(crawlCardRows).toBe(4)
    expect(ids(latestCrawlRuns(runs))).toEqual(['f', 'e', 'd', 'c'])
    expect(ids(runs)).toEqual(['a', 'b', 'c', 'd', 'e', 'f'])
  })

  it('keeps the API order for equal or unreadable dates instead of guessing', () => {
    const runs = [run('x', 'not a date'), run('y', '2026-09-02T12:00:00Z'), run('z', 'not a date')]
    expect(ids(latestCrawlRuns(runs))).toEqual(['y', 'x', 'z'])
    expect(ids(latestCrawlRuns([run('p', '2026-09-02T12:00:00Z'), run('q', '2026-09-02T12:00:00Z')]))).toEqual(['p', 'q'])
  })
})

describe('upcomingCrawlSchedules', () => {
  it('puts the soonest next run first and schedules without one after, newest first', () => {
    const schedules = [
      schedule('none-old', null, '2026-08-01T12:00:00Z'),
      schedule('later', '2026-10-05T12:00:00Z'),
      schedule('none-new', undefined, '2026-09-20T12:00:00Z'),
      schedule('soon', '2026-10-01T12:00:00Z'),
      schedule('garbled', 'soon-ish'),
    ]
    expect(ids(upcomingCrawlSchedules(schedules))).toEqual(['soon', 'later', 'none-new', 'garbled'])
    expect(ids(schedules)).toEqual(['none-old', 'later', 'none-new', 'soon', 'garbled'])
  })
})

describe('crawlDateLabel', () => {
  it('formats day and month the way the recent-work card does, per locale', () => {
    const value = '2026-09-28T12:00:00Z'
    expect(crawlDateLabel(value, 'no')).toBe(new Date(value).toLocaleDateString('nb-NO', { day: 'numeric', month: 'short' }))
    expect(crawlDateLabel(value, 'en')).toBe(new Date(value).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' }))
  })

  it('is blank for a missing or unreadable date', () => {
    for (const value of [undefined, null, '', 'tomorrow']) expect(crawlDateLabel(value, 'en')).toBe('')
  })
})

describe('labels', () => {
  it('names the run and schedule kinds as the ingestions forms do', () => {
    expect(['scrape', 'crawl', 'extract', 'batch', 'search', 'agent'].map((kind) => crawlKindLabel(kind, no))).toEqual(['Scrape', 'Crawl', 'Uttrekk', 'Batch', 'Søk', 'Agent'])
    expect(crawlKindLabel('extract', en)).toBe('Extract')
    expect(crawlKindLabel(' CRAWL ', en)).toBe('Crawl')
  })

  it('capitalises a kind it does not know and tolerates a missing one', () => {
    expect(crawlKindLabel('monitor', en)).toBe('Monitor')
    for (const kind of [undefined, null, 4, '']) expect(crawlKindLabel(kind, en)).toBe('')
  })

  it('translates the statuses the ingestions page groups on', () => {
    const groups: Array<[string[], string, string]> = [
      [['completed', 'success', 'done'], 'Fullført', 'Completed'],
      [['running', 'in_progress', 'syncing'], 'Kjører', 'Running'],
      [['queued', 'pending'], 'I kø', 'Queued'],
      [['active'], 'Aktiv', 'Active'],
      [['paused'], 'Pauset', 'Paused'],
      [['failed', 'error'], 'Feilet', 'Failed'],
      [['timeout'], 'Tidsavbrudd', 'Timed out'],
    ]
    for (const [statuses, norwegian, english] of groups) {
      for (const status of statuses) {
        expect(crawlStatusLabel(status, no)).toBe(norwegian)
        expect(crawlStatusLabel(status.toUpperCase(), en)).toBe(english)
      }
    }
  })

  it('shows any other status as sent, the way the status badge does', () => {
    expect(crawlStatusLabel('waiting_for_review', en)).toBe('waiting for review')
    for (const status of [undefined, null, 7, '']) expect(crawlStatusLabel(status, en)).toBe('')
  })

  it('joins the parts that exist', () => {
    expect(crawlMetaLine('Crawl', 'Completed')).toBe('Crawl · Completed')
    expect(crawlMetaLine('', 'Completed')).toBe('Completed')
    expect(crawlMetaLine('Crawl', '')).toBe('Crawl')
    expect(crawlMetaLine('', '')).toBe('')
  })
})
