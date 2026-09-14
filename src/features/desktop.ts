/** Desktop compatibility composition. Installable package delivery is tracked in TODO.md. */
import { provide, type Component } from 'vue'
import { Home, Lightbulb, WashingMachine, PlugZap } from '@lucide/vue'
import { useHA } from '../composables/useHA'
import { initHomeNotifications } from './desktop/homeNotifications'
import CameraVideo from '../CameraVideo.vue'
import type { AppConfig } from '../config'

export { default as DashboardFeaturePanels } from './desktop/DashboardPanels.vue'
export { default as DashboardFeatureActions } from './desktop/CameraAction.vue'
export { default as DashboardFeatureStatus } from './desktop/HomeStatus.vue'
export { default as DashboardConnectionStatus } from './desktop/CameraStatus.vue'
export { default as FeatureConfigSection } from './desktop/IntegrationConfig.vue'
export { default as FeatureSectionVisibility } from './desktop/SectionVisibility.vue'
export { default as FeatureControlsEditor } from './desktop/HomeControlsEditor.vue'
export { default as FeatureDiscoveryDialog } from './desktop/DiscoveryDialog.vue'
export { default as FeatureSetup } from './desktop/Setup.vue'
export { default as HeaderControlsEditor } from '../components/HeaderTogglesEditor.vue'
export { default as ControlTargetInput } from '../components/EntityAutocompleteInput.vue'
export { useDashboardControlsConfig as useConfigControls } from '../composables/useDashboardControlsConfig'
export { cameraConnection as featureConnection } from './desktop/cameraConnection'
export { featureDefaultConfig } from './desktop/defaultConfig'

export const featureConfigSections = [
  { id: 'integrations', label: 'Home Assistant & Cameras', icon: Home },
]
export const featureSetupAvailable = true

export function prepareFeatureConfig(config: AppConfig) {
  config.ha_use_direct_api = !!(config.ha_url?.trim() && config.ha_longlived_token?.trim())
  config.ha_port ??= 8123
  config.mqtt_ha_port ??= 1883
}

export function getFeatureView(path: string): Component | undefined {
  return path === '/camera-video' ? CameraVideo : undefined
}

export function useDashboardFeatures() {
  const home = useHA()
  provide('desktop-home', home)
  let stopNotifications: (() => void) | undefined
  return {
    allowHomeControls: true,
    controlsConnected: home.haConnected,
    getControlState: home.getHaControlState,
    getControlLabel: (label: string) =>
      label
        .replace(/\b(laundry|washer|washing|guard)\b/gi, '')
        .replace(/\s{2,}/g, ' ')
        .trim()
        .split(' ')
        .filter(Boolean)
        .join('\n'),
    getControlIcon: (entity: string, label: string): Component | null => {
      if (entity.split('.')[0] === 'light') return Lightbulb
      if (/laundry|washer|washing/.test(label.toLowerCase())) return WashingMachine
      if (label.toLowerCase().includes('guard')) return PlugZap
      return null
    },
    async init() {
      await home.initHa()
      stopNotifications ??= initHomeNotifications(home.haEntityStates, home.haEntityAttributes)
    },
    cleanup() {
      home.cleanupHa()
      stopNotifications?.()
      stopNotifications = undefined
    },
    setWindowHidden: home.setHaWindowHidden,
  }
}
