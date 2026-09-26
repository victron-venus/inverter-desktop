/** MQTT contract owned by inverter-control. HA may expose these flags as switches. */
export const INVERTER_CONTROL_FLAGS = [
  'only_charging',
  'no_feed',
  'house_support',
  'charge_battery',
  'do_not_supply_charger',
  'set_limit_to_ev_charger',
  'minimize_charging',
] as const

export interface DashboardControl {
  id: string
  label: string
  /** Bare inverter-control key, legacy input_boolean alias, or a home-device entity. */
  entity: string
  state_key?: string
}

export type ControlState = 'on' | 'off' | 'unavailable'

/** Only the documented legacy alias is a flag; switch.no_feed is a real HA entity. */
export function inverterControlFlagKey(entityOrId: string): string | null {
  const raw = entityOrId.trim()
  const key = raw.startsWith('input_boolean.') ? raw.slice('input_boolean.'.length) : raw
  return (INVERTER_CONTROL_FLAGS as readonly string[]).includes(key) ? key : null
}

export function isInverterControlFlag(entityOrId: string): boolean {
  return inverterControlFlagKey(entityOrId) !== null
}

export function normalizeControlTarget(control: DashboardControl): DashboardControl {
  return { ...control, entity: inverterControlFlagKey(control.entity) ?? control.entity }
}

export function mqttBooleanState(raw: unknown): ControlState {
  if (typeof raw === 'string') return raw === 'true' || raw === '1' ? 'on' : 'off'
  return raw ? 'on' : 'off'
}

export function mqttControlState(
  control: Pick<DashboardControl, 'id' | 'entity' | 'state_key'>,
  booleans: Record<string, unknown>,
  fallbackKey = control.id
): ControlState {
  const flag = inverterControlFlagKey(control.entity)
  // The canonical flag is authoritative even when a saved button has another id.
  if (flag) return mqttBooleanState(booleans[flag] ?? booleans[control.entity])
  const entityKey = control.entity.split('.').pop() || control.id
  return mqttBooleanState(
    booleans[control.state_key ?? fallbackKey] ?? booleans[entityKey] ?? booleans[control.entity]
  )
}
