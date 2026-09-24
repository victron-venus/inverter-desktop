import type { AppConfig } from './config'
import type { TariffPlan } from './tariffs/model'

export const TARIFF_MODULE = 'victron.energy-tariff'

/** Keep prices in the portable configuration without putting account data in it. */
export function configurationTariff(config: AppConfig | null | undefined): unknown {
  const module = config?.modules?.[TARIFF_MODULE]
  if (!module) return undefined
  // Let the tariff validator reject an unsupported envelope instead of ignoring it.
  return module.schema_version === 1 ? module.values.plan : module
}

export function setConfigurationTariff(config: AppConfig, plan: TariffPlan | null): void {
  config.modules = {
    ...config.modules,
    [TARIFF_MODULE]: {
      schema_version: 1,
      values: { ...config.modules?.[TARIFF_MODULE]?.values, plan },
    },
  }
}
