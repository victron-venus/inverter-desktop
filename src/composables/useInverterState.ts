import { ref } from 'vue'
// Use ref for deep reactivity so that nested properties (like state.value.loads) updates are tracked.
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
    tasmota_daily?: number[]
    mppt_daily?: number[]
    pv_total_daily?: number
  }
  latest_version?: string
  console?: string[]
}

export const state = ref<InverterState>({
  booleans: {},
  features: {},
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
