import { describe, expect, it } from 'vitest'
import { formatUsd } from './chat-media-markdown'

describe('formatUsd', () => {
  it('renders an exact zero as $0.00', () => {
    expect(formatUsd(0)).toBe('$0.00')
  })

  it('renders a typical sub-cent cost with trailing zeros trimmed', () => {
    expect(formatUsd(0.0031)).toBe('$0.0031')
  })

  it('trims a value that needs fewer decimals than the sub-cent precision', () => {
    expect(formatUsd(0.031)).toBe('$0.031')
  })

  it('never rounds a genuinely nonzero tiny cost down to $0', () => {
    const result = formatUsd(0.0000001)
    expect(result).not.toBe('$0.00')
    expect(result).not.toBe('$0')
    expect(Number(result.slice(1))).toBeGreaterThan(0)
  })

  it('formats a dollar-or-larger cost with plain 2-decimal currency', () => {
    expect(formatUsd(1.5)).toBe('$1.50')
    expect(formatUsd(12.3456)).toBe('$12.35')
  })

  it('never shows $NaN for a non-finite value', () => {
    expect(formatUsd(Number.NaN)).toBe('—')
    expect(formatUsd(Number.POSITIVE_INFINITY)).toBe('—')
  })
})
