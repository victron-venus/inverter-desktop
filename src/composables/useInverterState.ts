import { ref, shallowRef } from 'vue'
import type { AppConfig } from '../config'

export interface InverterState {
  gt?: number
  g1?: number
  g2?: number
  tt?: number
  t1?: number
  t2?: number
  solar_total?: number
  mppt_total?: number
  tasmota_total?: number
  battery_soc?: number
  battery_power?: number
  battery_voltage?: number
  battery_current?: number
  setpoint?: number
  inverter_state?: string
  version?: string
  uptime?: number
  ha_connected?: boolean
  ha_direct_connected?: boolean
  dry_run?: boolean
  ess_mode?: { mode_name?: string; is_external?: boolean }
  booleans?: Record<string, boolean>
  features?: Record<string, boolean>
  mppt_individual?: number[]
  tasmota_individual?: number[]
  mppt_chargers?: Array<{ name?: string; pv_voltage?: number; current?: number; power?: number }>
  batteries?: Array<{
    name?: string
    voltage?: number
    current?: number
    power?: number
    soc?: number
    state?: string
    time_to_go?: string
  }>
  loads?: Record<string, number>
  ui_config?: {
    loads?: { hidden?: string[]; min_watts?: number }
    home_buttons?: Array<{ id: string; label: string; entity: string; state_key?: string }>
    header_toggles?: Array<{ id: string; label: string; entity: string }>
  }
  daily_stats?: {
    produced_today?: number
    produced_dollars?: number
    grid_kwh?: number
    battery_in?: number
    battery_out?: number
    battery_in_yesterday?: number
    battery_out_yesterday?: number
    tasmota_daily?: number[]
    mppt_daily?: number[]
    pv_total_daily?: number
  }
  ev_charging_kw?: number
  ev_power?: number
  car_soc?: number
  water_level?: number
  water_valve?: boolean
  pump_switch?: boolean
  dishwasher_running?: boolean
  dishwasher_duration?: number
  washer_time?: number
  washer_power?: boolean
  dryer_time?: number
  dryer_power?: boolean
  latest_version?: string
  console?: string[]
}

export const state = shallowRef<InverterState>({
  booleans: {},
  features: {},
  loads: {},
  ui_config: {},
})

export const mqttConnected = ref(false)
export const haMqttConnected = ref<boolean | null>(null)
export const appConfig = ref<AppConfig | null>(null)

export interface NotificationEntry {
  id: number
  title: string
  body: string
  timestamp: number
  read: boolean
}

const notifId = ref(0)
const MAX_NOTIFICATIONS = 100

export const notifications = ref<NotificationEntry[]>([])

export function addNotification(title: string, body: string) {
  notifications.value = [
    { id: ++notifId.value, title, body, timestamp: Date.now(), read: false },
    ...notifications.value,
  ].slice(0, MAX_NOTIFICATIONS)
}

export function markNotificationRead(id: number) {
  const n = notifications.value.find((n) => n.id === id)
  if (n) n.read = true
}

export function markAllNotificationsRead() {
  for (const n of notifications.value) {
    n.read = true
  }
}

export function clearNotifications() {
  notifications.value = []
}

export function unreadNotificationCount() {
  return notifications.value.filter((n) => !n.read).length
}
