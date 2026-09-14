import { INVERTER_CONTROL_FLAGS } from './inverterControl'

const inverterControlTargets: readonly string[] = INVERTER_CONTROL_FLAGS

/** Header controls accept native inverter-control flags and optional HA entities. */
export function isDashboardControlTarget(target: string): boolean {
  if (target !== target?.trim()) return false
  return inverterControlTargets.includes(target) || /^[a-z_][a-z0-9_]*\.[a-z0-9_]+$/.test(target)
}
