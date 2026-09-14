import { describe, expect, it } from 'vitest'

import { coerceBoolean } from '../utils'

describe('coerceBoolean', () => {
  it('returns true for boolean true', () => {
    expect(coerceBoolean(true)).toBe(true)
  })

  it('returns true for number 1', () => {
    expect(coerceBoolean(1)).toBe(true)
  })

  it('returns true for string "true"', () => {
    expect(coerceBoolean('true')).toBe(true)
  })

  it('returns true for string "1"', () => {
    expect(coerceBoolean('1')).toBe(true)
  })

  it('returns true for string "on"', () => {
    expect(coerceBoolean('on')).toBe(true)
  })

  it('returns true for string "online"', () => {
    expect(coerceBoolean('online')).toBe(true)
  })

  it('returns false for boolean false', () => {
    expect(coerceBoolean(false)).toBe(false)
  })

  it('returns false for number 0', () => {
    expect(coerceBoolean(0)).toBe(false)
  })

  it('returns false for string "false"', () => {
    expect(coerceBoolean('false')).toBe(false)
  })

  it('returns false for string "0"', () => {
    expect(coerceBoolean('0')).toBe(false)
  })

  it('returns false for string "off"', () => {
    expect(coerceBoolean('off')).toBe(false)
  })

  it('returns false for undefined', () => {
    expect(coerceBoolean(undefined)).toBe(false)
  })

  it('returns false for null', () => {
    expect(coerceBoolean(null)).toBe(false)
  })

  it('returns false for empty string', () => {
    expect(coerceBoolean('')).toBe(false)
  })
})
