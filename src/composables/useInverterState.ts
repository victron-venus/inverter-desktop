import { markRaw, ref, shallowRef } from 'vue'
// shallowRef + replace-with-new-object (see applyInverterState): nested loads/etc.
// update when MQTT sends a fresh snapshot. markRaw avoids deep-proxying big payloads.
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
  mppt_chargers?: Array<{
    name?: string
    serial?: string
    instance?: number
    pv_voltage?: number
    current?: number
    power?: number
  }>
  // AC PV inverters of any vendor (V/I/P per device)
  pv_inverters?: Array<{
    name?: string
    serial?: string
    instance?: number
    voltage?: number
    current?: number
    power?: number
  }>
  // Legacy daemon aggregate: power per inverter (no V/I), fallback when pv_inverters empty
  pv_inverter_individual?: number[]
  batteries?: Array<{
    name?: string
    serial?: string
    instance?: number
    voltage?: number
    current?: number
    power?: number
    soc?: number
    state?: string
    time_to_go?: string
  }>
  loads?: Record<string, number>
  ui_config?: {
    home_buttons?: Array<{ id: string; label: string; entity: string; state_key?: string }>
    header_toggles?: Array<{ id: string; label: string; entity: string }>
  }
  daily_stats?: {
    produced_today?: number
    produced_yesterday?: number
    produced_dollars?: number
    grid_kwh?: number
    battery_in?: number
    battery_out?: number
    battery_in_yesterday?: number
    battery_out_yesterday?: number
    pv_inverter_daily?: number[]
    mppt_daily?: number[]
    pv_total_daily?: number
  }
  solar_forecast?: {
    date?: string
    today_kwh?: number
    tomorrow_kwh?: number
  }
  latest_version?: string
  console?: string[]
  /** Water system, fed by dbus-pump via Cerbo GX MQTT */
  water_level?: number | null
  water_valve?: boolean | null
  pump_switch?: boolean | null
  /** dbus-pump /Mode (0 auto, 1 always-on, 2 always-off); null until known */
  water_pump_mode?: number | null
  water_valve_mode?: number | null
  /** EV vehicle battery % from dbus-ev via Cerbo MQTT (N/<portal>/ev/<i>/Soc) */
  car_soc?: number | null
  /** EV vehicle charging power (W) from dbus-ev (N/<portal>/ev/<i>/Ac/Power) */
  car_charging_power?: number | null
  /** Wallbox charging power (W) from dbus-evcharger (N/<portal>/evcharger/<i>/Ac/Power) */
  ev_charging_power?: number | null
  /** True once any ev/<i>/... message for the configured instance has been
   *  seen on Cerbo MQTT. Survives daemon merges; gates the EV card. */
  ev_present?: boolean
  /** True once any evcharger/<i>/... message has been seen on Cerbo MQTT. */
  evcharger_present?: boolean
}

export const state = shallowRef<InverterState>({
  booleans: {},
  features: {},
  ui_config: {},
})

export const mqttConnected = ref(false)
export const haMqttConnected = ref<boolean | null>(null)
export const appConfig = ref<AppConfig | null>(null)

/** Non-destructive merge into dashboard state. Skips null/undefined so partial
 *  MQTT snapshots and serde nulls cannot wipe live telemetry. Always assigns a
 *  new markRaw object so shallowRef watchers/tiles re-render. */
export function applyInverterState(newState: InverterState) {
  const prev = state.value
  const merged: InverterState = { ...prev }
  for (const [key, val] of Object.entries(newState)) {
    if (val !== undefined && val !== null) {
      ;(merged as Record<string, unknown>)[key] = val
    }
  }
  state.value = markRaw(merged)
}

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
  const entry = notifications.value.find((n) => n.id === id)
  if (entry) entry.read = true
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

// ---------------------------------------------------------------------------
// Persistent banner notifications (inverter/notifications MQTT topic + Victron alarms)
// ---------------------------------------------------------------------------

export interface BannerNotification {
  id: string
  level: 'info' | 'warning' | 'alarm'
  title: string
  body: string
  source?: string
  ts?: string
}

export const bannerNotifications = ref<BannerNotification[]>([])

const DISMISSED_KEY = 'dismissed_banner_ids'
const MAX_DISMISSED = 200

function loadDismissedIds(): Set<string> {
  try {
    const raw = localStorage.getItem(DISMISSED_KEY)
    const arr: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(arr)
      ? new Set(arr.filter((x): x is string => typeof x === 'string'))
      : new Set()
  } catch {
    return new Set()
  }
}

const dismissedIds = loadDismissedIds()

function saveDismissedIds() {
  const arr = [...dismissedIds].slice(-MAX_DISMISSED)
  dismissedIds.clear()
  for (const id of arr) dismissedIds.add(id)
  localStorage.setItem(DISMISSED_KEY, JSON.stringify(arr))
}

export function isBannerDismissed(id: string): boolean {
  return dismissedIds.has(id)
}

/** User dismissed the banner — hidden until a new notification reuses a fresh id. */
export function dismissBanner(id: string) {
  dismissedIds.add(id)
  saveDismissedIds()
  clearBanner(id)
}

/** Add or replace by id (dedupe for hourly re-publishes). */
export function upsertBanner(notification: BannerNotification) {
  if (dismissedIds.has(notification.id)) return
  const idx = bannerNotifications.value.findIndex((b) => b.id === notification.id)
  if (idx >= 0) {
    bannerNotifications.value[idx] = notification
  } else {
    bannerNotifications.value = [...bannerNotifications.value, notification]
  }
}

/** Alarm resolved (value back to 0) — remove without recording a dismissal. */
export function clearBanner(id: string) {
  bannerNotifications.value = bannerNotifications.value.filter((b) => b.id !== id)
}
