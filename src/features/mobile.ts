/** Fixed mobile composition: no desktop extension code or package lifecycle is imported. */
import { defineComponent, ref, type Component } from 'vue'
import type { AppConfig } from '../config'
import type { ControlState, DashboardControl } from '../inverterControl'
export { default as HeaderControlsEditor } from '../components/HeaderTogglesEditor.vue'
export { default as ControlTargetInput } from './CoreControlInput.vue'
export { useCoreControlsConfig as useConfigControls } from './coreControlsConfig'
const Empty = defineComponent({ inheritAttrs: false, setup: () => () => null })
export const DashboardFeaturePanels = Empty
export const DashboardFeatureActions = Empty
export const DashboardFeatureStatus = Empty
export const DashboardConnectionStatus = Empty
export const FeatureConfigSection = Empty
export const FeaturePluginManager = Empty
export const featurePluginManagerTabId = undefined
export const FeatureSectionVisibility = Empty
export const FeatureControlsEditor = Empty
export const FeatureDiscoveryDialog = Empty
export const FeatureSetup = Empty
export const featureConfigSections: Array<{
  id: string
  label: string
  labelKey?: string
  icon: Component
}> = []
export const featureSetupAvailable = false
export const isMobileApp = true
export { featureDefaultConfig } from './defaults.mobile'
export function prepareFeatureConfig(_config: AppConfig) {
  // Mobile preserves opaque desktop settings without interpreting or normalizing them.
}
export function getFeatureView(_path: string): Component | undefined {
  return undefined
}
export const featureConnection = { async connect(_config: AppConfig) {}, cleanup() {} }
export function useDashboardFeatures() {
  return {
    allowHomeControls: false,
    getControlLabel: (label: string) => label,
    getControlIcon: (_entity: string, _label: string): Component | null => null,
    controlsConnected: ref(true),
    getControlState: (_control: DashboardControl): ControlState | undefined => undefined,
    async init() {},
    cleanup() {},
    async setWindowHidden(_hidden: boolean) {},
  }
}
