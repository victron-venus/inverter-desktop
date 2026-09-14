import type { AppConfig } from '../config'

/** Mobile has no optional feature defaults; opaque stored values remain untouched. */
export const featureDefaultConfig: Partial<AppConfig> = {}
