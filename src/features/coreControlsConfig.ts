import { ref } from 'vue'
import type { AppConfig } from '../config'
import {
  isInverterControlFlag,
  normalizeControlTarget,
  type DashboardControl,
} from '../inverterControl'

/** Core control editor has no entity discovery or optional service dependency. */
export function useCoreControlsConfig() {
  const haEntitiesList = ref<NonNullable<AppConfig['ha_entities']>>([])
  const headerTogglesList = ref<DashboardControl[]>([])
  const discoveredEntities = ref<never[]>([])
  let originalHome: NonNullable<AppConfig['ha_entities']> = []
  let originalHeader: DashboardControl[] = []

  function mergeVisible<T extends { entity: string }>(original: T[], edited: T[]): T[] {
    const remaining = edited.map((entry) => ({ ...entry }))
    const merged: T[] = []
    for (const entry of original) {
      if (!isInverterControlFlag(entry.entity)) merged.push({ ...entry })
      else if (remaining.length) merged.push(remaining.shift()!)
    }
    return [...merged, ...remaining]
  }

  function getSavedControls() {
    return {
      home: mergeVisible(originalHome, haEntitiesList.value),
      header: mergeVisible(originalHeader, headerTogglesList.value),
      editableHeader: headerTogglesList.value,
    }
  }
  function loadFromConfig(config: AppConfig) {
    originalHome = (config.ha_entities ?? []).map((entry) => ({ ...entry }))
    originalHeader = (config.header_toggles_config ?? []).map((entry) => ({ ...entry }))
    haEntitiesList.value = originalHome
      .filter((entry) => isInverterControlFlag(entry.entity))
      .map((entry) => ({ ...entry }))
    headerTogglesList.value = originalHeader
      .map(normalizeControlTarget)
      .filter((entry) => isInverterControlFlag(entry.entity))
  }
  function addHeaderToggle(control?: DashboardControl) {
    if (!control) {
      headerTogglesList.value.push({ id: '', label: '', entity: '' })
      return
    }
    let id = control.id
    let suffix = 2
    while (
      headerTogglesList.value.some((entry) => entry.id === id) ||
      originalHeader.some((entry) => !isInverterControlFlag(entry.entity) && entry.id === id)
    )
      id = `${control.id}_${suffix++}`
    headerTogglesList.value.push({ ...control, id })
  }
  function removeHeaderToggle(index: number) {
    headerTogglesList.value.splice(index, 1)
  }
  function moveToggleUp(index: number) {
    if (index <= 0) return
    const [entry] = headerTogglesList.value.splice(index, 1)
    headerTogglesList.value.splice(index - 1, 0, entry)
  }
  function moveToggleDown(index: number) {
    if (index >= headerTogglesList.value.length - 1) return
    const [entry] = headerTogglesList.value.splice(index, 1)
    headerTogglesList.value.splice(index + 1, 0, entry)
  }
  return {
    getSavedControls,
    haEntitiesList,
    headerTogglesList,
    discoveredEntities,
    loadFromConfig,
    addHeaderToggle,
    removeHeaderToggle,
    moveToggleUp,
    moveToggleDown,
    refreshSuggestions: (_config: AppConfig) => {},
  }
}
