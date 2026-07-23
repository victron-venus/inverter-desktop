import { invoke } from '@tauri-apps/api/core'
import { type Ref, ref, watch } from 'vue'
import { state } from './useInverterState'

export async function notify(title: string, body: string) {
  try {
    await invoke('send_notification', { title, body })
  } catch {
    // Plugin may not be available — ignore
  }
}

const prevEvChargingKw = ref<number | undefined>(undefined)
const prevWaterValve = ref<boolean | undefined>(undefined)
const prevPumpSwitch = ref<boolean | undefined>(undefined)
const prevHomeStates = ref<Record<string, string>>({})
const lastNotifyTime = new Map<string, number>()
const NOTIFY_COOLDOWN_MS = 60_000
let initialized = false
const NOTIFIABLE_DOMAINS = new Set(['switch', 'input_boolean', 'light', 'fan', 'binary_sensor'])

function shouldNotifyEntity(domain: string, st: string, entityId: string, now: number): boolean {
  if (!NOTIFIABLE_DOMAINS.has(domain)) return false
  if (domain === 'binary_sensor' && st !== 'on') return false
  const lastTime = lastNotifyTime.get(entityId) || 0
  return now - lastTime >= NOTIFY_COOLDOWN_MS
}

function getEntityName(entityId: string, attrs?: Record<string, unknown>): string {
  return (attrs?.friendly_name as string) || entityId.split('.').pop() || entityId
}

export function initSystemNotifications(
  haEntityStates: Ref<Record<string, string>>,
  haEntityAttributes: Ref<Record<string, Record<string, unknown>>>
) {
  if (initialized) return
  initialized = true

  watch(
    () => state.value.ev_charging_kw,
    (val) => {
      const prev = prevEvChargingKw.value
      if (prev !== undefined && val !== undefined) {
        if (prev === 0 && val > 0) {
          notify('EV Charging Started', `Charging at ${val.toFixed(1)} kW`)
        } else if (prev > 0 && val === 0) {
          notify('EV Charging Stopped', 'Charging has ended')
        }
      }
      prevEvChargingKw.value = val
    }
  )

  watch(
    () => state.value.water_valve,
    (val) => {
      const prev = prevWaterValve.value
      if (prev !== undefined && val !== prev) {
        notify('Water Valve', val ? 'Valve OPENED' : 'Valve CLOSED')
      }
      prevWaterValve.value = val
    }
  )

  watch(
    () => state.value.pump_switch,
    (val) => {
      const prev = prevPumpSwitch.value
      if (prev !== undefined && val !== prev) {
        notify('Water Pump', val ? 'Pump ON' : 'Pump OFF')
      }
      prevPumpSwitch.value = val
    }
  )

  watch(
    haEntityStates,
    (states) => {
      const prev = prevHomeStates.value
      const now = Date.now()
      for (const [entityId, st] of Object.entries(states) as [string, string][]) {
        const prevSt = prev[entityId]
        if (prevSt === undefined || prevSt === st) continue
        const domain = entityId.split('.')[0]
        if (!shouldNotifyEntity(domain, st, entityId, now)) continue
        lastNotifyTime.set(entityId, now)
        const name = getEntityName(entityId, haEntityAttributes.value[entityId])
        notify('Home Control', `${name}: ${st.toUpperCase()}`)
      }
      prevHomeStates.value = { ...states }
    },
    { deep: true }
  )
}
