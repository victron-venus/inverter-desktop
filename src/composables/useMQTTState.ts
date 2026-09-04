import { computed, ref, watchEffect } from 'vue'
import { state } from './useInverterState'

// Water from Cerbo MQTT (dbus-pump)
const waterLevel = computed(() => state.value.water_level ?? null)
const pumpSwitchState = computed(() => state.value.pump_switch ?? null)
const waterValveState = computed(() => state.value.water_valve ?? null)
const waterPumpMode = computed(() => state.value.water_pump_mode ?? null)
const waterValveMode = computed(() => state.value.water_valve_mode ?? null)
const waterSectionVisible = computed(
  () =>
    state.value.water_level != null ||
    state.value.water_valve != null ||
    state.value.pump_switch != null
)

// EV from Cerbo MQTT (dbus-ev / dbus-evcharger)
const evSoc = computed(() => {
  const v = state.value.car_soc
  if (v == null) return null
  return Math.max(0, Math.min(100, v))
})

// Car AC power (N/<portal>/ev/<i>/Ac/Power)
const evChargingWatts = computed(() => state.value.car_charging_power ?? null)
const evChargingKw = computed(() => {
  const w = evChargingWatts.value
  return w === null ? null : w / 1000
})

// Wallbox clamp power (N/<portal>/evcharger/<i>/Ac/Power)
const evClampWatts = computed(() => {
  const v = state.value.ev_charging_power
  if (v == null) return null
  return Math.abs(v)
})
const evPowerWatts = computed(() => evClampWatts.value)

// Latch: once any ev/evcharger MQTT message seen, never hide the card
const evLatch = ref(false)
watchEffect(() => {
  if (state.value.ev_present || state.value.evcharger_present) {
    evLatch.value = true
  }
})
const evSectionVisible = computed(() => evLatch.value)

// Active loads from MQTT (clamp sensors via Cerbo)
const acloads = computed(() => {
  const mqttLoads = state.value.loads
  if (!mqttLoads || Object.keys(mqttLoads).length === 0) return []
  const nameMap = state.value.load_names ?? {}
  const items: Array<{ id: string; name: string; value: number; isGeneration: boolean }> = []
  for (const [key, val] of Object.entries(mqttLoads)) {
    const v = typeof val === 'number' ? val : Number(val)
    if (!Number.isNaN(v) && Math.abs(v) > 2) {
      // Prefer Cerbo CustomName/ProductName cache; never flash raw instance id
      // once a name is known. Fallback formats legacy daemon keys.
      const cached = nameMap[key]
      const name =
        (cached && cached.trim()) || key.replace(/ Power 1S$/i, '').replace(/_/g, ' ') || key
      items.push({ id: key, name, value: v, isGeneration: v < 0 })
    }
  }
  items.sort((a, b) => {
    const absDiff = Math.abs(b.value) - Math.abs(a.value)
    if (absDiff !== 0) return absDiff
    return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
  })
  return items
})

export function useMQTTState() {
  return {
    waterLevel,
    pumpSwitchState,
    waterValveState,
    waterPumpMode,
    waterValveMode,
    waterSectionVisible,
    evSoc,
    evChargingKw,
    evPowerWatts,
    evSectionVisible,
    acloads,
    /** Exposed for tests — resets the EV latch between test runs. */
    evLatch,
  }
}
