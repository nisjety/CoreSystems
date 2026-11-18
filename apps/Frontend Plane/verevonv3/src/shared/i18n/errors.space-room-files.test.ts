import { describe, expect, it } from 'vitest'

import { translateApiError } from './errors'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

describe('WS14 room file error copy', () => {
  const genericNo = translateApiError({ code: 'a_code_nobody_registered' }, no)
  const genericEn = translateApiError({ code: 'a_code_nobody_registered' }, en)

  // The imports-core refusal a file's own row carries when the room's allowlist does not take its type.
  it('room_file_type_not_allowed has its own Norwegian and English sentence', () => {
    const norwegian = translateApiError({ code: 'room_file_type_not_allowed' }, no)
    const english = translateApiError({ code: 'room_file_type_not_allowed' }, en)
    expect(norwegian).not.toBe(genericNo)
    expect(english).not.toBe(genericEn)
    expect(norwegian).not.toBe(english)
    expect(norwegian).toBe('Denne filtypen er ikke tillatt i rommet.')
    expect(english).toBe('This file type is not allowed in the room.')
  })
})
