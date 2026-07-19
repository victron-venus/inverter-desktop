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
let initialized = false

export function initSystemNotifications(haEntityStates: Ref<Record<string, string>>) {
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
      for (const [entityId, st] of Object.entries(states) as [string, string][]) {
        const prevSt = prev[entityId]
        if (prevSt !== undefined && prevSt !== st) {
          const domain = entityId.split('.')[0]
          if (
            domain === 'switch' ||
            domain === 'input_boolean' ||
            domain === 'light' ||
            domain === 'fan'
          ) {
            const name = entityId.split('.').pop() || entityId
            notify('Home Control', `${name}: ${st.toUpperCase()}`)
          }
        }
      }
      prevHomeStates.value = { ...states }
    },
    { deep: true }
  )
}
