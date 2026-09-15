import type { NumberInputContribution } from './types'

const MAX_SCALED_VALUE = 1_000_000_000_000_000

function coefficient(value: number): boolean {
  return Number.isSafeInteger(value) && Math.abs(value) <= MAX_SCALED_VALUE
}

function precision(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 6
}

export function numberInputAcceptsValue(input: NumberInputContribution, value: number): boolean {
  if (
    !coefficient(value) ||
    !coefficient(input.min_scaled) ||
    !coefficient(input.max_scaled) ||
    !coefficient(input.step_scaled) ||
    input.step_scaled <= 0 ||
    value < input.min_scaled ||
    value > input.max_scaled
  )
    return false
  return (BigInt(value) - BigInt(input.min_scaled)) % BigInt(input.step_scaled) === 0n
}

export function validNumberInput(input: NumberInputContribution): boolean {
  return (
    input.kind === 'number_input' &&
    typeof input.input_revision === 'string' &&
    input.input_revision.length > 0 &&
    precision(input.decimal_places) &&
    numberInputAcceptsValue(input, input.value_scaled)
  )
}

/** Observed values and display names do not change the authority of a draft. */
export function sameNumberInputAuthority(
  left: NumberInputContribution,
  right: NumberInputContribution
): boolean {
  return (
    left.id === right.id &&
    left.action_id === right.action_id &&
    left.input_revision === right.input_revision &&
    left.min_scaled === right.min_scaled &&
    left.max_scaled === right.max_scaled &&
    left.step_scaled === right.step_scaled &&
    left.decimal_places === right.decimal_places &&
    (left.unit ?? null) === (right.unit ?? null)
  )
}

/** Convert decimal text to an integer coefficient without floating-point rounding. */
export function parseNumberDraft(draft: string, decimalPlaces: number): number | null {
  if (!precision(decimalPlaces) || draft.length > 64) return null
  const match = /^([+-]?)(\d*)(?:\.(\d+))?$/.exec(draft.trim())
  if (!match || (!match[2] && !match[3])) return null
  const fraction = match[3] ?? ''
  if (/[^0]/.test(fraction.slice(decimalPlaces))) return null
  const digits = (match[2] || '0') + fraction.slice(0, decimalPlaces).padEnd(decimalPlaces, '0')
  const value = BigInt(digits) * (match[1] === '-' ? -1n : 1n)
  if (value < -BigInt(MAX_SCALED_VALUE) || value > BigInt(MAX_SCALED_VALUE)) return null
  return Number(value)
}

export function formatNumberValue(value: number, decimalPlaces: number): string {
  if (!coefficient(value) || !precision(decimalPlaces)) return '—'
  const negative = value < 0 ? '-' : ''
  const digits = BigInt(value < 0 ? -value : value)
    .toString()
    .padStart(decimalPlaces + 1, '0')
  if (decimalPlaces === 0) return negative + digits
  return `${negative}${digits.slice(0, -decimalPlaces)}.${digits.slice(-decimalPlaces)}`
}
