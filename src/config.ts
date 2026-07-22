import { invoke } from '@tauri-apps/api/core'
import { logger } from './logger'

/** Default MQTT broker address – configure to match your local setup */
const DEFAULT_MQTT_HOST = 'Cerbo'

export interface AppConfig {
  mqtt_host: string
  mqtt_port: number
  mqtt_login?: string | null
  mqtt_password?: string | null
  mqtt_ha_host?: string
  mqtt_ha_port?: number
  mqtt_ha_login?: string | null
  mqtt_ha_password?: string | null
  ha_longlived_token?: string | null
  ha_url?: string | null
  ha_port?: number | null
  ha_use_direct_api?: boolean
  ha_entities?: Array<{
    id: string
    label: string
    entity: string
    domain: string
    enabled: boolean
  }>
  header_toggles_config?: Array<{ id: string; label: string; entity: string }>
  color_scheme?: string | null
  portal_id?: string | null
  camera_topic?: string | null
  camera_enabled?: boolean

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
  show_console?: boolean
  auto_start?: boolean
  auth_enabled?: boolean
  auth_username?: string | null
  auth_password?: string | null
  auth_biometric?: boolean
}

// Single source of truth for section visibility defaults
const SHOW_DEFAULTS = {
  show_batteries: true,
  show_solar_production: true,
  show_active_loads: true,
  show_daily_stats: true,
  show_ev: true,
  show_washer: true,
  show_dryer: true,
  show_dishwasher: true,
  show_home_section: true,
  show_header_toggles: true,
  show_ha_sensors: true,
  show_ha_numbers: true,
  show_ha_covers: true,
  show_ha_media: true,
  show_ha_scenes: true,
  show_ha_weather: true,
  show_console: true,
  auto_start: false,
  auth_enabled: false,
  auth_username: null,
  auth_password: null,
  auth_biometric: false,
} as const

const defaultConfig: AppConfig = {
  mqtt_host: DEFAULT_MQTT_HOST,
  mqtt_port: 1883,
  mqtt_login: null,
  mqtt_password: null,
  mqtt_ha_host: 'HA',
  mqtt_ha_port: 1883,
  mqtt_ha_login: null,
  mqtt_ha_password: null,
  ha_longlived_token: null,
  ha_url: null,
  ha_port: null,
  ha_use_direct_api: false,
  ha_entities: undefined,
  header_toggles_config: undefined,
  color_scheme: 'dark',
  portal_id: null,
  camera_topic: 'frigate/+/events',
  camera_enabled: false,

  ...SHOW_DEFAULTS,
}

let config: AppConfig = defaultConfig

export async function getAppConfig(): Promise<AppConfig> {
  try {
    const fetched = await invoke<AppConfig>('get_config')
    config = { ...defaultConfig, ...fetched }
    return config
  } catch (e) {
    logger.warn('Failed to load config, using defaults', e)
    return { ...defaultConfig }
  }
}

// Export defaultConfig for use in computed properties if needed
export { defaultConfig }
