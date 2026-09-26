import type { AppConfig } from '../config'

/** Keep the existing scope keys so saved local tariffs and intervals remain available. */
export function tariffScope(config: Partial<AppConfig> | null | undefined): string {
  return config?.portal_id || config?.gateway_url || config?.mqtt_host || 'dashboard'
}
