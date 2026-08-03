import { describe, expect, it } from 'vitest'
import { formatPercent } from '@/utils/docker-format'

describe('formatPercent', () => {
  it('formats zero, ordinary, and multi-core percentages', () => {
    expect(formatPercent(0)).toBe('0.0%')
    expect(formatPercent(12.34)).toBe('12.3%')
    expect(formatPercent(125.45)).toBe('125.5%')
  })

  it('distinguishes low positive usage from zero', () => {
    expect(formatPercent(0.0017)).toBe('<0.1%')
    expect(formatPercent(0.099)).toBe('<0.1%')
    expect(formatPercent(0.1)).toBe('0.1%')
  })

  it('uses the unavailable fallback for missing or invalid values', () => {
    expect(formatPercent()).toBe('0.0%')
    expect(formatPercent(Number.NaN)).toBe('0.0%')
  })
})
