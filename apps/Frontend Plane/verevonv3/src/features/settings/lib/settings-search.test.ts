import { describe, expect, it } from 'vitest'
import { searchSettingsSections } from '@/features/settings/lib/settings-search'
import { workspaceSettingsSections } from '@/features/settings/lib/settings-sections'

const idsFor = (query: string) => searchSettingsSections(query).map((section) => section.id)

describe('searchSettingsSections', () => {
  it('finds a section by a Norwegian word and by an English word', () => {
    expect(idsFor('tidssone')).toEqual(['cron'])
    expect(idsFor('invoice')[0]).toBe('billing')
    expect(idsFor('tone')[0]).toBe('persona')
    expect(idsFor('login')[0]).toBe('sso')
    expect(idsFor('e-post')[0]).toBe('members')
  })

  it('ranks a section by its own name above a synonym that another section also lists', () => {
    expect(idsFor('minne')).toEqual(['memory', 'org-security'])
    expect(idsFor('memory')[0]).toBe('memory')
    expect(idsFor('learning')[0]).toBe('learning')
  })

  it('ignores case, diacritics and hyphens', () => {
    expect(idsFor('LÆRING')).toEqual(idsFor('laering'))
    expect(idsFor('laering')[0]).toBe('learning')
    expect(idsFor('fine-tune')[0]).toBe('finetune')
  })

  it('needs every word of a multi-word query to match', () => {
    expect(idsFor('tone pers')).toEqual(['persona'])
    expect(idsFor('tone faktura')).toEqual([])
  })

  it('returns nothing for a blank query or for a query no setting matches', () => {
    expect(searchSettingsSections('')).toEqual([])
    expect(searchSettingsSections('   ')).toEqual([])
    expect(idsFor('xyzzy')).toEqual([])
  })

  it('gives every section synonyms and finds each section by its own id first', () => {
    for (const section of workspaceSettingsSections) {
      expect(section.keywords.length, section.id).toBeGreaterThan(0)
      expect(idsFor(section.id)[0], section.id).toBe(section.id)
    }
  })
})
