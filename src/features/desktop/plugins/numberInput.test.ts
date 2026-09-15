import { describe, expect, it } from 'vitest'
import {
  formatNumberValue,
  numberInputAcceptsValue,
  parseNumberDraft,
  validNumberInput,
} from './numberInput'
import type { NumberInputContribution } from './types'

const input: NumberInputContribution = {
  kind: 'number_input',
  id: 'temperature',
  title: 'Temperature',
  action_id: 'set-temperature',
  label: 'Set temperature',
  input_revision: 'input-1',
  unit: '°C',
  value_scaled: 25,
  min_scaled: -135,
  max_scaled: 165,
  step_scaled: 10,
  decimal_places: 2,
}

describe('exact numeric drafts', () => {
  it.each([
    ['0.3', 1, 3],
    ['-1.35', 2, -135],
    ['.005', 3, 5],
    ['-0.10', 2, -10],
    [' +001.23000 ', 2, 123],
    ['2.000', 0, 2],
    ['-0', 6, 0],
    ['1000000000', 6, 1_000_000_000_000_000],
    ['-1000000000000000', 0, -1_000_000_000_000_000],
  ])('parses %s exactly at precision %i', (draft, precision, expected) => {
    expect(parseNumberDraft(String(draft), Number(precision))).toBe(expected)
  })

  it.each([
    '',
    ' ',
    '-',
    '+',
    '.',
    '1.',
    '1e2',
    'NaN',
    'Infinity',
    '0x10',
    '1,25',
    '1.2.3',
    '0.001',
    '-0.001',
    '10000000000000.01',
    '1'.repeat(65),
  ])('rejects empty, invalid or unrepresentable text %j', (draft) => {
    expect(parseNumberDraft(draft, 2)).toBeNull()
  })

  it('formats bounded coefficients without scientific notation or binary rounding', () => {
    expect(formatNumberValue(-5, 2)).toBe('-0.05')
    expect(formatNumberValue(3, 1)).toBe('0.3')
    expect(formatNumberValue(0, 6)).toBe('0.000000')
    expect(formatNumberValue(1_000_000_000_000_000, 6)).toBe('1000000000.000000')
    expect(formatNumberValue(Number.NaN, 2)).toBe('—')
    expect(parseNumberDraft('1', 7)).toBeNull()
  })

  it('anchors the exact grid to a negative minimum rather than zero', () => {
    expect(validNumberInput(input)).toBe(true)
    for (const value of [-135, -125, 25, 35, 165]) {
      expect(numberInputAcceptsValue(input, value)).toBe(true)
    }
    for (const value of [-145, -130, 30, 175, 35.5, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(numberInputAcceptsValue(input, value)).toBe(false)
    }
    const wide = {
      ...input,
      min_scaled: -1_000_000_000_000_000,
      max_scaled: 1_000_000_000_000_000,
      step_scaled: 1,
    }
    expect(numberInputAcceptsValue(wide, 1_000_000_000_000_000)).toBe(true)
    expect(numberInputAcceptsValue(wide, 1_000_000_000_000_001)).toBe(false)
  })

  it.each([
    { value_scaled: 30 },
    { value_scaled: 175 },
    { min_scaled: Number.NaN },
    { max_scaled: Number.POSITIVE_INFINITY },
    { step_scaled: 0 },
    { step_scaled: 0.1 },
    { min_scaled: 200 },
    { decimal_places: 7 },
    { decimal_places: 1.5 },
    { input_revision: '' },
    { value_scaled: Number.MAX_SAFE_INTEGER + 1 },
  ])('rejects a malformed numeric advertisement: %j', (change) => {
    expect(validNumberInput({ ...input, ...change })).toBe(false)
  })
})
