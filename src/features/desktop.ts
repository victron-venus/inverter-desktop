/** Desktop owns only generic installed-package UI. Integrations live in workers. */
import { defineComponent, provide, ref, type Component } from 'vue'
import { Puzzle } from '@lucide/vue'
import { listen } from '@tauri-apps/api/event'
import type { AppConfig } from '../config'
import type { ControlState, DashboardControl } from '../inverterControl'
import PluginMedia from './desktop/plugins/PluginMedia.vue'
import { createPluginPresentation, pluginPresentationKey } from './desktop/plugins/presentation'

export { default as DashboardFeaturePanels } from './desktop/plugins/PluginCompactPanels.vue'
export { default as DashboardFeatureActions } from './desktop/plugins/PluginGroupActions.vue'
export { default as DashboardFeatureStatus } from './desktop/plugins/PluginConnectionStatus.vue'
export { default as FeaturePluginManager } from './desktop/plugins/PluginManager.vue'
export { featureDefaultConfig } from './desktop/defaultConfig'
const Empty = defineComponent({ inheritAttrs: false, setup: () => () => null })
export const DashboardConnectionStatus = Empty
export const FeatureSetup = Empty
export const featureConfigSections = [
  { id: 'plugins', label: '', labelKey: 'plugins.manager.title', icon: Puzzle },
]
export const featurePluginManagerTabId = 'plugins'
export const featureSetupAvailable = false
export const isMobileApp = false
export async function subscribeFeatureConfig(config: AppConfig, current: () => boolean) {
  return listen<{ desktop_plugins: NonNullable<AppConfig['desktop_plugins']> }>(
    'plugin-configuration-changed',
    (event) => {
      if (current() && Array.isArray(event.payload?.desktop_plugins))
        config.desktop_plugins = structuredClone(event.payload.desktop_plugins)
    }
  )
}
export function prepareFeatureConfig(_config: AppConfig) {}
export function getFeatureView(path: string): Component | undefined {
  return path === '/camera-video' ? PluginMedia : undefined
}
/** Core reconnects never establish optional integration connections. */
export const featureConnection = { async connect(_config: AppConfig) {}, cleanup() {} }
export function useDashboardFeatures() {
  const context = createPluginPresentation()
  provide(pluginPresentationKey, context)
  return {
    allowHomeControls: false,
    controlsConnected: ref(true),
    getControlState: (_control: DashboardControl): ControlState | undefined => undefined,
    getControlLabel: (label: string) => label,
    getControlIcon: (_entity: string, _label: string): Component | null => null,
    mergeControls: context.mergeControls,
    init: context.dashboard.start,
    cleanup: context.dashboard.stop,
    async setWindowHidden(_hidden: boolean) {},
  }
}
