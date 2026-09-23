import { describe, expect, it } from 'vitest'
import { containsRoundedNumber, grossProfitChangeClaims } from './e2e/product-sales-oracle'

describe('sales margin rounding', () => {
  const margin = 100 * 123000 / 360000

  it.each(['34.2%', '34,17 %', '| 34.167 |', '**34,1667 %**', '+34.1667%', '+ 34,1667 %'])('accepts correct displayed precision: %s', text => {
    expect(containsRoundedNumber(text, margin)).toBe(true)
  })

  it.each(['34.1%', '34.16%', '34.1666%', '34%', '134.1667%', '0.34.1667%', '-34.1667%', '−34,1667%', '- 34.1667%', '− 34,1667%', '1 034,1667%', '1\u00a0034,1667%', '1\u202f034,1667%', 'sku34.1667', '34.1667e2', '34.16670%'])('rejects wrong values and partial matches: %s', text => {
    expect(containsRoundedNumber(text, margin)).toBe(false)
  })

  it.each(['40', '40%', '40.0%', '40,0000%', '| 40.0000 |', 'Margin: 40.'])('accepts exact whole margins and trailing zeros: %s', text => {
    expect(containsRoundedNumber(text, 40)).toBe(true)
  })

  it('preserves negative signs, including spaced and Unicode minus', () => {
    expect(containsRoundedNumber('− 34,1667%', -margin)).toBe(true)
    expect(containsRoundedNumber('+34.1667%', -margin)).toBe(false)
  })

  it('supports correctly grouped thousands without matching their suffix', () => {
    expect(containsRoundedNumber('1\u202f234,17', 1234.1667)).toBe(true)
    expect(containsRoundedNumber('1\u202f234,17', 234.1667)).toBe(false)
  })

  it('checks the independently computed total margins at four decimals', () => {
    const row = '| Totalt | 360 000 | 360 000 | 123 000 | 124 680 | 34,1667 % | 34,6333 % |'
    expect(containsRoundedNumber(row, 100 * 123000 / 360000)).toBe(true)
    expect(containsRoundedNumber(row, 100 * 124680 / 360000)).toBe(true)
    expect(containsRoundedNumber(row, 34.1666)).toBe(false)
  })

  it('rejects non-finite expectations', () => {
    expect(containsRoundedNumber('Infinity NaN', Infinity)).toBe(false)
    expect(containsRoundedNumber('Infinity NaN', NaN)).toBe(false)
  })
})

describe('sales profit change scope', () => {
  const groups = ['Belysning', 'Skrivebord', 'Oppbevaring']
  const expected: Record<string, number> = { Totalt: 1680, Skrivebord: -18720, Belysning: 12000, Oppbevaring: 8400 }

  it.each([
    ['Bruttofortjenesten økte (+1 680 kr, +1,37 %).', 'Totalt', 1680],
    ['Vekst i Belysning (+30 000 kr) og Oppbevaring (+24 000 kr) veide opp for fallet i Skrivebord (−54 000 kr). Skrivebord hadde også svakere margin, fra 30,00 % til 28,00 %, og lavere bruttofortjeneste (−18 720 kr).', 'Skrivebord', -18720],
    ['Belysning økte. Bruttofortjenesten økte totalt (+1 680 NOK).', 'Totalt', 1680],
    ['Belysning fikk økt bruttofortjeneste (**+12 000 kr**).', 'Belysning', 12000],
    ['Bruttofortjenesten for Oppbevaring økte (+8 400 kr).', 'Oppbevaring', 8400],
    ['Skrivebord falt, mens samlet bruttofortjeneste økte (+1 680 kr).', 'Totalt', 1680],
  ])('binds the claim to its stated scope: %s', (text, group, amount) => {
    const claims = grossProfitChangeClaims(String(text), groups)
    expect(claims).toHaveLength(1)
    expect(claims[0]).toMatchObject({ group, amount })
    expect(claims[0].amount).toBe(expected[String(group)])
  })

  it.each([
    'Samlet bruttofortjeneste økte (+12 000 kr).',
    'Skrivebord hadde lavere bruttofortjeneste (+18 720 kr).',
    'Skrivebord hadde lavere bruttofortjeneste (−1 680 kr).',
    'Belysning hadde høyere bruttofortjeneste (+1 680 kr).',
  ])('keeps incorrect values distinguishable from the expected change: %s', text => {
    const claims = grossProfitChangeClaims(text, groups)
    expect(claims).toHaveLength(1)
    expect(claims[0].amount).not.toBe(expected[claims[0].group])
  })

  it('retains a wrong percentage for independent rounding validation', () => {
    const [claim] = grossProfitChangeClaims('Bruttofortjenesten økte (+1 680 kr, +9,37 %).', groups)
    expect(containsRoundedNumber(claim.percentage!, 100 * 1680 / 123000)).toBe(false)
  })

  it('keeps the saved profit decrease separate from the next sentence revenue increase', () => {
    const text = 'Den største målte endringen var skrivebord: omsetningen falt **54 000,00 kr** (−**30,0000 %**), bruttofortjenesten falt **18 720,00 kr** (−**34,6667 %**), og marginen falt **2,0000 prosentpoeng** til **28,0000 %**. Dette ble motvirket av belysning (+**30 000,00 kr** i omsetning) og oppbevaring (+**24 000,00 kr**).'
    expect(grossProfitChangeClaims(text, groups)).toEqual([{ group: 'Skrivebord', amount: -18720, percentage: '−34,6667' }])
  })

  it.each([
    'Bruttofortjenesten var 124 680 kr. Belysning hadde høyere omsetning (+30 000 kr).',
    'Skrivebord hadde lavere bruttofortjeneste, mens Belysning hadde høyere omsetning (+30 000 kr).',
    'Bruttofortjenesten steg 1,37 %. Omsetningen i Oppbevaring økte (+24 000 kr).',
  ])('does not borrow money from a later sentence or metric: %s', text => {
    expect(grossProfitChangeClaims(text, groups)).toEqual([])
  })

  it.each([
    ['Bruttofortjenesten økte med 1 680 kr (+1,3659 %).', 'Totalt', 1680, '+1,3659'],
    ['Skrivebord: bruttofortjenesten falt 18 720 kr (34,6667 %).', 'Skrivebord', -18720, '-34,6667'],
    ['Belysning: bruttofortjenesten steg 12 000 kroner (+25 %).', 'Belysning', 12000, '+25'],
    ['Skrivebord: bruttofortjenesten falt −18 720 kr (−34,6667 %).', 'Skrivebord', -18720, '−34,6667'],
  ])('checks direct monetary changes as well as parentheses: %s', (text, group, amount, percentage) => {
    expect(grossProfitChangeClaims(String(text), groups)).toEqual([{ group, amount, percentage }])
  })

  it.each([
    'Skrivebord: bruttofortjenesten falt +18 720 kr.',
    'Skrivebord: bruttofortjenesten falt 1 680 kr.',
    'Belysning: bruttofortjenesten steg −12 000 kr.',
  ])('retains wrong direct amounts and explicit signs for rejection: %s', text => {
    const claims = grossProfitChangeClaims(text, groups)
    expect(claims).toHaveLength(1)
    expect(claims[0].amount).not.toBe(expected[claims[0].group])
  })

  it('retains separate claims in a sentence with multiple groups', () => {
    expect(grossProfitChangeClaims('Belysning hadde høyere bruttofortjeneste (+12 000 kr), og Skrivebord hadde lavere bruttofortjeneste (−18 720 kr).', groups))
      .toEqual([{ group: 'Belysning', amount: 12000, percentage: undefined }, { group: 'Skrivebord', amount: -18720, percentage: undefined }])
  })
})
