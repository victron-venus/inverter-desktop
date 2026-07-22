import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'

import type { AppConfig } from '../config'
import { logger } from '../logger'

export interface DiscoveredEntity {
  entity_id: string
  friendly_name: string
  domain: string
  state: string
}

export function useHAEntityManager() {
  const haEntitiesList = ref<
    Array<{ id: string; label: string; entity: string; domain: string; enabled: boolean }>
  >([])
  const headerTogglesList = ref<Array<{ id: string; label: string; entity: string }>>([])
  const discoveryDialog = ref(false)
  const discoveredEntities = ref<DiscoveredEntity[]>([])
  const selectedDiscovery = ref<string[]>([])
  const discoveryLoading = ref(false)
  const discoveryTargetGroup = ref<'home' | 'toggle'>('home')
  const dragOverIndex = ref<number | null>(null)
  const dragTarget = ref<'home' | 'toggle' | null>(null)

  function loadFromConfig(config: AppConfig) {
    haEntitiesList.value = config.ha_entities || []

    headerTogglesList.value = config.header_toggles_config || []
  }

  async function fetchHaEntities(
    haUrl: string,
    haPort: number | null | undefined,
    haToken: string
  ) {
    discoveryLoading.value = true
    try {
      const entities = await invoke<DiscoveredEntity[]>('discover_ha_entities', {
        url: haUrl,
        port: haPort || 8123,
        token: haToken,
      })
      discoveredEntities.value = entities
      selectedDiscovery.value = []
      discoveryTargetGroup.value = 'home'
      discoveryDialog.value = true
    } finally {
      discoveryLoading.value = false
    }
  }

  function addDiscoveredEntities() {
    const target = discoveryTargetGroup.value
    const toAdd = discoveredEntities.value.filter((e) =>
      selectedDiscovery.value.includes(e.entity_id)
    )
    const targetList = target === 'home' ? haEntitiesList.value : headerTogglesList.value
    const existingKey = target === 'home' ? 'entity' : 'entity'

    for (const de of toAdd) {
      if (targetList.some((e) => e[existingKey] === de.entity_id)) continue
      const base = {
        id: de.entity_id.replace(/\./g, '_'),
        label: de.friendly_name || de.entity_id,
        entity: de.entity_id,
      }
      if (target === 'home') {
        haEntitiesList.value.push({ ...base, domain: de.domain, enabled: true })
      } else {
        headerTogglesList.value.push(base)
      }
    }
    discoveryDialog.value = false
  }

  function addHaEntity() {
    haEntitiesList.value.push({ id: '', label: '', entity: '', domain: 'switch', enabled: true })
  }

  function removeHaEntity(index: number) {
    haEntitiesList.value.splice(index, 1)
  }

  function moveEntityUp(index: number) {
    if (index <= 0) return
    const item = haEntitiesList.value.splice(index, 1)[0]
    haEntitiesList.value.splice(index - 1, 0, item)
  }

  function moveEntityDown(index: number) {
    if (index >= haEntitiesList.value.length - 1) return
    const item = haEntitiesList.value.splice(index, 1)[0]
    haEntitiesList.value.splice(index + 1, 0, item)
  }

  function addHeaderToggle() {
    headerTogglesList.value.push({ id: '', label: '', entity: '' })
  }

  function removeHeaderToggle(index: number) {
    headerTogglesList.value.splice(index, 1)
  }

  function moveToggleUp(index: number) {
    if (index <= 0) return
    const item = headerTogglesList.value.splice(index, 1)[0]
    headerTogglesList.value.splice(index - 1, 0, item)
  }

  function moveToggleDown(index: number) {
    if (index >= headerTogglesList.value.length - 1) return
    const item = headerTogglesList.value.splice(index, 1)[0]
    headerTogglesList.value.splice(index + 1, 0, item)
  }

  async function ensureEntitiesFetched(
    haUrl: string,
    haPort: number | null | undefined,
    haToken: string
  ) {
    if (discoveredEntities.value.length > 0 || discoveryLoading.value) return
    if (!haUrl || !haToken) return

    discoveryLoading.value = true
    try {
      const entities = await invoke<DiscoveredEntity[]>('discover_ha_entities', {
        url: haUrl,
        port: haPort || 8123,
        token: haToken,
      })
      discoveredEntities.value = entities
    } catch (e) {
      logger.error('Failed to auto-fetch HA entities:', e)
    } finally {
      discoveryLoading.value = false
    }
  }
  return {
    haEntitiesList,
    headerTogglesList,
    discoveryDialog,
    discoveredEntities,
    selectedDiscovery,
    discoveryLoading,
    discoveryTargetGroup,
    dragOverIndex,
    dragTarget,
    loadFromConfig,
    fetchHaEntities,
    addDiscoveredEntities,
    addHaEntity,
    removeHaEntity,
    moveEntityUp,
    moveEntityDown,
    addHeaderToggle,
    removeHeaderToggle,
    moveToggleUp,
    moveToggleDown,
    ensureEntitiesFetched,
  }
}
