import { describe, expect, it } from 'vitest'
import { acceptableProfitChange, containsRoundedNumber, grossProfitChangeClaims } from './e2e/product-sales-oracle'

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
    expect(grossProfitChangeClaims(text, groups)).toEqual([{ group: 'Skrivebord', amount: -18720, percentage: '−34,6667', scope: 'explicit' }])
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
    const claims = grossProfitChangeClaims(String(text), groups)
    expect(claims).toHaveLength(1)
    expect(claims[0]).toMatchObject({ group, amount, percentage })
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
      .toEqual([{ group: 'Belysning', amount: 12000, percentage: undefined, scope: 'explicit' }, { group: 'Skrivebord', amount: -18720, percentage: undefined, scope: 'explicit' }])
  })
})

describe('sales profit change context scope (Q16 02 take 2 false failure)', () => {
  const groups = ['Belysning', 'Skrivebord', 'Oppbevaring']
  const changes: Record<string, number> = { Totalt: 1680, Skrivebord: -18720, Belysning: 12000, Oppbevaring: 8400 }

  it('binds a claim to the group named one sentence earlier in the same bullet', () => {
    const [claim] = grossProfitChangeClaims('Skrivebord sto for hele nettofallet. Bruttofortjenesten falt 18 720 kr, og marginen gikk ned 2,00 prosentpoeng.', groups)
    expect(claim).toMatchObject({ group: 'Skrivebord', amount: -18720, scope: 'inherited' })
    expect(acceptableProfitChange(claim, changes)).toBe(true)
  })

  it('still rejects a wrong figure after an earlier group sentence', () => {
    const [claim] = grossProfitChangeClaims('Skrivebord sto for hele nettofallet. Bruttofortjenesten falt 9 000 kr.', groups)
    expect(acceptableProfitChange(claim, changes)).toBe(false)
  })

  it("accepts a return to the total after naming a group, but not another group's figure", () => {
    const [total] = grossProfitChangeClaims('Belysning og Oppbevaring vokste. Bruttofortjenesten økte med 1 680 kr.', groups)
    expect(total.scope).toBe('inherited')
    expect(acceptableProfitChange(total, changes)).toBe(true)
    const [other] = grossProfitChangeClaims('Oppbevaring vokste. Bruttofortjenesten økte med 12 000 kr.', groups)
    expect(acceptableProfitChange(other, changes)).toBe(false)
  })

  it('keeps an unscoped claim strict to the total', () => {
    const [claim] = grossProfitChangeClaims('Bruttofortjenesten økte med 12 000 kr.', groups)
    expect(claim).toMatchObject({ group: 'Totalt', scope: 'default' })
    expect(acceptableProfitChange(claim, changes)).toBe(false)
  })

  it('reads an amount stated before the metric, and not the next group clause (Q16b 02 take 7)', () => {
    const text = 'I de målte tallene endret produktmiksen seg: Belysning økte med 30 000 kr i omsetning og 12 000 kr i bruttofortjeneste, og Oppbevaring økte med 24 000 kr og 8 400 kr. Skrivebord falt med 54 000 kr i omsetning og 18 720 kr i bruttofortjeneste; gruppens margin falt 2,0000 prosentpoeng.'
    const claims = grossProfitChangeClaims(text, groups)
    expect(claims).toEqual([
      { group: 'Belysning', amount: 12000, percentage: undefined, scope: 'explicit' },
      { group: 'Skrivebord', amount: -18720, percentage: undefined, scope: 'explicit' },
    ])
    expect(claims.every(claim => acceptableProfitChange(claim, changes))).toBe(true)
  })

  it('still rejects a wrong amount stated before the metric', () => {
    const [claim] = grossProfitChangeClaims('Belysning økte med 30 000 kr i omsetning og 24 000 kr i bruttofortjeneste.', groups)
    expect(claim).toMatchObject({ group: 'Belysning', amount: 24000 })
    expect(acceptableProfitChange(claim, changes)).toBe(false)
    const [falling] = grossProfitChangeClaims('Skrivebord falt med 54 000 kr i omsetning og 18 720 kr i bruttofortjeneste.', groups)
    expect(falling.amount).toBe(-18720)
  })

  it('reads a level stated before the metric as no change claim (Q16b 02 take 9)', () => {
    expect(grossProfitChangeClaims('Eksempel totalt uke 37: 360 000 kr − 235 320 kr = 124 680 kr i bruttofortjeneste; 124 680 kr / 360 000 kr × 100 = 34,6333 % vektet bruttomargin.', groups)).toEqual([])
    expect(grossProfitChangeClaims('Uken ga 124 680 kr i bruttofortjeneste (+1 680 kr).', groups))
      .toEqual([{ group: 'Totalt', amount: 1680, percentage: undefined, scope: 'default' }])
    const [claim] = grossProfitChangeClaims('Bruttofortjenesten økte fra 123 000 kr til 124 680 kr (+1 680 kr; +1,3659 %), og vektet bruttomargin økte.', groups)
    expect(claim).toMatchObject({ group: 'Totalt', amount: 1680 })
    expect(acceptableProfitChange(claim, changes)).toBe(true)
  })

  it('binds a claim to a sales channel named in its definite form (Q16b 02 take 16)', () => {
    const scopes = ['Belysning', 'Skrivebord', 'Oppbevaring', 'Nettbutikk', 'Bedriftssalg']
    const byScope: Record<string, number> = { ...changes, Nettbutikk: 7360, Bedriftssalg: -5680 }
    const text = '- Nettbutikken økte omsetningen fra 180 000 kr til 198 000 kr (+18 000 kr; +10,0000 %) og bruttofortjenesten fra 64 000 kr til 71 360 kr (+7 360 kr; +11,5000 %).\n- Bedriftssalg falt fra 180 000 kr til 162 000 kr (−18 000 kr; −10,0000 %) og bruttofortjenesten fra 59 000 kr til 53 320 kr (−5 680 kr; −9,6271 %).'
    const claims = grossProfitChangeClaims(text, scopes)
    expect(claims.map(({ group, amount, scope }) => ({ group, amount, scope }))).toEqual([
      { group: 'Nettbutikk', amount: 7360, scope: 'explicit' },
      { group: 'Bedriftssalg', amount: -5680, scope: 'explicit' },
    ])
    expect(claims.every(claim => acceptableProfitChange(claim, byScope))).toBe(true)
    // A wrong channel figure still fails, and a longer compound is no scope.
    const [wrong] = grossProfitChangeClaims('Bedriftssalget hadde lavere bruttofortjeneste (−7 360 kr).', scopes)
    expect(wrong).toMatchObject({ group: 'Bedriftssalg', amount: -7360 })
    expect(acceptableProfitChange(wrong, byScope)).toBe(false)
    expect(grossProfitChangeClaims('Belysningskampanjen løftet bruttofortjenesten (+1 680 kr).', scopes)[0]).toMatchObject({ group: 'Totalt', scope: 'default' })
  })

  it('reads a span before or after the metric as its difference (Q16b 02 take 17)', () => {
    const scopes = ['Belysning', 'Skrivebord', 'Oppbevaring', 'Nettbutikk', 'Bedriftssalg']
    const byScope: Record<string, number> = { ...changes, Nettbutikk: 7360, Bedriftssalg: -5680 }
    const text = '- Nettbutikk økte omsetningen fra 180 000 kr til 198 000 kr og bruttofortjenesten fra 64 000 kr til 71 360 kr. Bedriftssalg falt tilsvarende fra 180 000 kr til 162 000 kr i omsetning og fra 59 000 kr til 53 320 kr i bruttofortjeneste.'
    const claims = grossProfitChangeClaims(text, scopes)
    expect(claims.map(({ group, amount }) => ({ group, amount }))).toEqual([{ group: 'Nettbutikk', amount: 7360 }, { group: 'Bedriftssalg', amount: -5680 }])
    expect(claims.every(claim => acceptableProfitChange(claim, byScope))).toBe(true)
    const [wrong] = grossProfitChangeClaims('Bedriftssalg falt fra 59 000 kr til 52 320 kr i bruttofortjeneste.', scopes)
    expect(acceptableProfitChange(wrong, byScope)).toBe(false)
  })

  it('binds a comparison to its subject, not to the group it is compared with (oracle review)', () => {
    const [right] = grossProfitChangeClaims('Belysning økte mer enn Oppbevaring, med 30 000 kr i omsetning og 12 000 kr i bruttofortjeneste.', groups)
    expect(right).toMatchObject({ group: 'Belysning', amount: 12000 })
    expect(acceptableProfitChange(right, changes)).toBe(true)
    const [wrong] = grossProfitChangeClaims('Belysning økte mindre enn Oppbevaring, med 24 000 kr i omsetning og 8 400 kr i bruttofortjeneste.', groups)
    expect(wrong).toMatchObject({ group: 'Belysning', amount: 8400 })
    expect(acceptableProfitChange(wrong, changes)).toBe(false)
  })

  it.each([
    ['Skrivebord reduserte med 54 000 kr i omsetning og 30 000 kr i bruttofortjeneste.', 'Skrivebord', -30000],
    ['Skrivebord gikk ned med 54 000 kr i omsetning og 30 000 kr i bruttofortjeneste.', 'Skrivebord', -30000],
    ['Skrivebord: −30 000 kr i bruttofortjeneste.', 'Skrivebord', -30000],
    ['Bruttofortjenesten; falt 30 000 kr.', 'Totalt', -30000],
    ['Skrivebord: bruttofortjenesten falt 18.720 kr.', 'Skrivebord', -18720],
  ])('reads a claim the oracle review found unchecked: %s', (text, group, amount) => {
    const [claim] = grossProfitChangeClaims(String(text), groups)
    expect(claim).toMatchObject({ group, amount })
  })

  it('reads a percentage after a semicolon for the rounding check', () => {
    const [claim] = grossProfitChangeClaims('Bruttofortjenesten økte (+1 680 kr; +1,3659 %).', groups)
    expect(claim).toMatchObject({ group: 'Totalt', amount: 1680, percentage: '+1,3659' })
  })

  it('stops a claim at the next clause instead of borrowing its amount', () => {
    expect(grossProfitChangeClaims('Belysning bidro mest til bruttofortjenesten, og Oppbevaring økte med 24 000 kr.', groups)).toEqual([])
    expect(grossProfitChangeClaims('Bruttofortjenesten var stabil; Oppbevaring økte med 24 000 kr.', groups)).toEqual([])
  })

  it('does not carry a group across lines', () => {
    const [claim] = grossProfitChangeClaims('Skrivebord sto for hele nettofallet.\nBruttofortjenesten økte med 1 680 kr.', groups)
    expect(claim).toMatchObject({ group: 'Totalt', scope: 'default' })
    expect(acceptableProfitChange(claim, changes)).toBe(true)
  })
})
