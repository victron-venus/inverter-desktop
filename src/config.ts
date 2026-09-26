import { featureDefaultConfig } from '@feature-defaults'
import { invoke } from '@tauri-apps/api/core'
import { logger } from './logger'
import type { DashboardControl } from './inverterControl'

/** Default MQTT broker address – configure to match your local setup */
const DEFAULT_MQTT_HOST = 'Cerbo'

/** Portable package declarations; only desktop interprets or installs them. */
export interface DesktopPluginArtifact {
  url: string
  sha256: string
  [key: string]: unknown
}

export interface DesktopPluginConfig {
  plugin_id: string
  version: string
  /** Omission enables the configured plugin; false retains an inactive installation. */
  enabled?: boolean
  artifacts: Record<string, DesktopPluginArtifact>
  [key: string]: unknown
}

/** Passive module data; core/mobile preserve payloads without interpreting them. */
export interface ModuleConfig {
  /** Positive 32-bit integer version of the module's payload schema. */
  schema_version: number
  /** Portable, non-secret JSON settings; arbitrary future payload fields are retained. */
  values: Record<string, unknown>
  /** Stored only natively; omitted from frontend reads and portable exports. */
  secrets?: Record<string, string>
}

export interface AppConfig {
  modules?: Record<string, ModuleConfig>
  mqtt_host: string
  mqtt_port: number
  /** TLS with certificate/hostname validation; false permits anonymous LAN MQTT only. */
  mqtt_tls?: boolean
  mqtt_login?: string | null
  mqtt_password?: string | null
  mqtt_ha_host?: string
  mqtt_ha_port?: number
  mqtt_ha_login?: string | null
  mqtt_ha_password?: string | null
  ha_longlived_token?: string | null
  ha_url?: string | null
  ha_port?: number | null
  /** Passive legacy field. Installed packages receive migrated settings natively. */
  ha_use_direct_api?: boolean
  ha_entities?: Array<{
    id: string
    label: string
    entity: string
    domain: string
    enabled: boolean
  }>
  ha_dryer_entity?: string
  ha_washer_entity?: string
  ha_washer_start_entity?: string
  ha_washer_pause_entity?: string
  ha_dryer_start_entity?: string
  ha_dryer_pause_entity?: string
  ha_dishwasher_running_entity?: string
  ha_dishwasher_duration_entity?: string
  /** Cerbo GX tank instance for water level (auto-discovered when unset) */
  water_tank_instance?: number
  /** dbus-pump startstop instances on the Cerbo GX (defaults 1/2) */
  water_pump_instance?: number
  water_valve_instance?: number
  /** Cerbo GX EV charger instance (default 40) */
  evcharger_instance?: number
  /** Cerbo GX EV instance (default 22) */
  ev_instance?: number
  ha_consumption_clamps?: string[]
  ha_generation_clamps?: string[]
  /** Passive migration data for installed Home Assistant plugins; never overrides controller UI. */
  header_toggles_config?: DashboardControl[]
  color_scheme?: string | null
  portal_id?: string | null
  camera_topic?: string | null
  /** Frigate HTTP base for clip URLs, e.g. http://192.168.151.21:5005 */
  frigate_base_url?: string | null
  /**
   * HTTP(S) snapshot URL template for Ring-MQTT motion/ding.
   * Placeholders: {device_id}, {location_id}, {event}.
   * Example: http://ha:8123/api/camera_proxy/camera.front_door_snapshot
   */
  ring_snapshot_url_template?: string | null
  /** Retained legacy live-view mappings; migration is native-only. */
  camera_live_urls?: Record<string, string>
  camera_enabled?: boolean
  show_advanced_settings?: boolean

  show_batteries?: boolean
  show_solar_production?: boolean
  show_active_loads?: boolean
  show_daily_stats?: boolean
  show_ev?: boolean
  show_washer?: boolean
  show_dryer?: boolean
  show_dishwasher?: boolean
  show_home_section?: boolean
  show_header_toggles?: boolean
  show_ha_sensors?: boolean
  show_ha_numbers?: boolean
  show_ha_covers?: boolean
  show_ha_media?: boolean
  show_ha_scenes?: boolean
  show_ha_weather?: boolean
  auto_start?: boolean
  auth_enabled?: boolean
  auth_username?: string | null
  auth_password?: string | null
  auth_biometric?: boolean
  /** Enable an HTTPS inverter-gateway connection */
  gateway_enabled?: boolean
  /** Local or public HTTPS base URL (e.g. https://gateway.example.com:9151) */
  gateway_url?: string | null
  /** Optional CF-Access-Client-Id; supply both Access fields or neither */
  gateway_access_client_id?: string | null
  /** Optional CF-Access-Client-Secret; supply both Access fields or neither */
  gateway_access_client_secret?: string | null
  /** Authorization: Bearer (GATEWAY_API_TOKEN) */
  gateway_api_token?: string | null
  /** First-run setup wizard completed (migrated true for existing installs) */
  setup_completed?: boolean
  /** Preserved on mobile without loading the desktop package lifecycle. */
  desktop_plugins?: DesktopPluginConfig[]
}

// Single source of truth for section visibility defaults
const SHOW_DEFAULTS = {
  show_batteries: true,
  show_solar_production: true,
  show_active_loads: false,
  show_daily_stats: false,
  show_ev: true,
  show_home_section: false,
  show_header_toggles: false,
  show_advanced_settings: false,
  auto_start: false,
} as const

const defaultConfig: AppConfig = {
  mqtt_host: DEFAULT_MQTT_HOST,
  mqtt_port: 1883,
  mqtt_tls: false,
  mqtt_login: null,
  mqtt_password: null,
  color_scheme: 'dark',
  portal_id: null,

  gateway_enabled: false,
  gateway_url: null,
  gateway_access_client_id: null,
  gateway_access_client_secret: null,
  gateway_api_token: null,
  setup_completed: false,
  ...SHOW_DEFAULTS,
  ...featureDefaultConfig,
}

export const sectionKeys = [
  'show_batteries',
  'show_solar_production',
  'show_active_loads',
  'show_daily_stats',
  'show_ev',
  'show_home_section',
  'show_header_toggles',
] as const

/** Older stores used null/missing for visible sections. Native migrates these too. */
export function normalizeConfig(stored: AppConfig): AppConfig {
  const normalized = { ...defaultConfig, ...stored }
  for (const key of sectionKeys) normalized[key] = stored[key] ?? true
  return normalized
}

/** Initial rendering and settings use the same explicit section defaults. */
export function sectionVisibility(config: AppConfig | null | undefined) {
  return Object.fromEntries(
    sectionKeys.map((key) => [key, config?.[key] ?? defaultConfig[key]])
  ) as Record<(typeof sectionKeys)[number], boolean>
}

let config: AppConfig = defaultConfig

export async function getAppConfig(): Promise<AppConfig> {
  try {
    const fetched = await invoke<AppConfig>('get_config')
    config = normalizeConfig(fetched)
    return config
  } catch (e) {
    logger.error('Failed to load config', e)
    throw e
  }
}

// Export defaultConfig for use in computed properties if needed
export { defaultConfig }

/** True when the first-run setup wizard should be shown. */
export function needsSetup(config: AppConfig | null | undefined): boolean {
  return !(config?.setup_completed ?? false)
}
