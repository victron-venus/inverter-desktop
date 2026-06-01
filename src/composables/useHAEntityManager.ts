import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

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

  function loadFromConfig(config: any) {
    let entities = config.ha_entities || []
    if (entities.length === 0 && config.ha_switch_entities) {
      entities = Object.entries(config.ha_switch_entities).map(([id, data]: [string, any]) => ({
        id,
        label: data.label || id,
        entity: data.entity || id,
        domain: 'switch',
        enabled: true,
      }))
    }
    haEntitiesList.value = entities

    let toggles = config.header_toggles_config || []
    if (toggles.length === 0 && config.header_toggles) {
      toggles = config.header_toggles
    }
    if (toggles.length === 0 && config.ha_boolean_entities) {
      toggles = Object.entries(config.ha_boolean_entities).map(([id, entity]) => ({
        id,
        label: id.replace(/_/g, ' ').toUpperCase(),
        entity,
      }))
    }
    headerTogglesList.value = toggles
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
    } catch (e: any) {
      throw e
    } finally {
      discoveryLoading.value = false
    }
  }

  function addDiscoveredEntities() {
    const target = discoveryTargetGroup.value
    const toAdd = discoveredEntities.value.filter((e) =>
      selectedDiscovery.value.includes(e.entity_id)
    )

    if (target === 'home') {
      toAdd.forEach((de) => {
        if (haEntitiesList.value.some((e) => e.entity === de.entity_id)) return
        haEntitiesList.value.push({
          id: de.entity_id.replace(/\./g, '_'),
          label: de.friendly_name || de.entity_id,
          entity: de.entity_id,
          domain: de.domain,
          enabled: true,
        })
      })
    } else {
      toAdd.forEach((de) => {
        if (headerTogglesList.value.some((t) => t.entity === de.entity_id)) return
        headerTogglesList.value.push({
          id: de.entity_id.replace(/\./g, '_'),
          label: de.friendly_name || de.entity_id,
          entity: de.entity_id,
        })
      })
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

  function autofillDomain(domain: string) {
    const toAdd = discoveredEntities.value.filter(
      (e) => e.domain === domain && e.state !== 'unavailable'
    )
    for (const de of toAdd) {
      if (haEntitiesList.value.some((ex) => ex.entity === de.entity_id)) continue
      haEntitiesList.value.push({
        id: de.entity_id.replace(/\./g, '_'),
        label: de.friendly_name,
        entity: de.entity_id,
        domain: de.domain,
        enabled: true,
      })
    }
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
    } catch (e: any) {
      console.error('Failed to auto-fetch HA entities:', e)
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
    autofillDomain,
    ensureEntitiesFetched,
  }
}
