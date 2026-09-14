import { INVERTER_CONTROL_FLAGS } from './inverterControl'

/** Header controls accept native inverter-control flags and optional HA entities. */
export function isDashboardControlTarget(target: string): boolean {
  if (!target || target !== target.trim()) return false
  return (
    INVERTER_CONTROL_FLAGS.some((flag) => flag === target) ||
    /^[a-z_][a-z0-9_]*\.[a-z0-9_]+$/.test(target)
  )
}
