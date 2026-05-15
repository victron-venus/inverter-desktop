import { invoke } from '@tauri-apps/api/core'

export interface AppConfig {
  mqtt_host: string
  mqtt_port: number
  ha_boolean_entities: Record<string, string>
  ha_switch_entities: Record<string, { label?: string; entity: string }>
  ha_water_valve_entity: string
  ha_pump_switch_entity: string
  header_toggles: Array<{ id: string; label: string; entity: string }>
}

const defaultConfig: AppConfig = {
  mqtt_host: '192.168.160.150',
  mqtt_port: 1883,
  ha_boolean_entities: {
    only_charging: 'input_boolean.only_charging',
    no_feed: 'input_boolean.no_feed',
    house_support: 'input_boolean.house_support',
    charge_battery: 'input_boolean.charge_battery',
    do_not_supply_charger: 'input_boolean.do_not_supply_charger',
    set_limit_to_ev_charger: 'input_boolean.set_limit_to_ev_charger',
    minimize_charging: 'input_boolean.minimize_charging'
  },
  ha_switch_entities: {},
  ha_water_valve_entity: 'switch.shutoff_valve',
  ha_pump_switch_entity: 'switch.pump_switch',
  header_toggles: [
    { id: 'only_charging', label: 'ONLY CHARGING', entity: 'input_boolean.only_charging' },
    { id: 'no_feed', label: 'NO FEED', entity: 'input_boolean.no_feed' },
    { id: 'house_support', label: 'HOUSE SUPPORT', entity: 'input_boolean.house_support' },
    { id: 'charge_battery', label: 'CHARGE BATTERY', entity: 'input_boolean.charge_battery' },
    { id: 'do_not_supply_charger', label: 'DO NOT SUPPLY EV', entity: 'input_boolean.do_not_supply_charger' },
    { id: 'set_limit_to_ev_charger', label: 'LIMIT TO EV', entity: 'input_boolean.set_limit_to_ev_charger' },
    { id: 'minimize_charging', label: 'MINIMIZE CHARGING', entity: 'input_boolean.minimize_charging' }
  ]
}

let config: AppConfig = defaultConfig

export async function getAppConfig(): Promise<AppConfig> {
  try {
    const fetched = await invoke<any>('get_config')
    config = { ...defaultConfig, ...fetched }
    return config
  } catch (e) {
    console.warn('Failed to load config, using defaults', e)
    return config
  }
}

// Export defaultConfig for use in computed properties if needed
export { defaultConfig }