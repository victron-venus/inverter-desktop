<template>
  <div class="h-px bg-slate-100 dark:bg-slate-800"></div>

  <HaEntitiesEditor
    :haEntitiesList="haEntitiesList"
    :discoveredEntities="discoveredEntities"
    :entityRules="entityRules"
    @add="addHaEntity"
    @remove="removeHaEntity"
    @move-up="moveEntityUp"
    @move-down="moveEntityDown"
    @focus-entity="
      ensureEntitiesFetched(config.ha_url || '', config.ha_port, config.ha_longlived_token || '')
    "
  />
</template>
<script setup lang="ts">
import type { AppConfig } from '../../config'
import type { useDashboardControlsConfig } from '../../composables/useDashboardControlsConfig'
import HaEntitiesEditor from '../../components/HaEntitiesEditor.vue'
const { config, controls } = defineProps<{
  config: AppConfig
  controls: ReturnType<typeof useDashboardControlsConfig>
}>()
const {
  haEntitiesList,
  discoveredEntities,
  addHaEntity,
  removeHaEntity,
  moveEntityUp,
  moveEntityDown,
  ensureEntitiesFetched,
} = controls
const entityRules = [(value: string) => !!value || 'Required']
</script>
