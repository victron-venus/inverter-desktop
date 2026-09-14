<template>
  <HomePanels
    v-bind="panels"
    @send="(action, payload) => $emit('send', action, payload)"
    @number-set="(entity, value) => $emit('send', 'number_set', { entity, value })"
    @cover-position="
      (entity, position) => $emit('send', 'set_cover_position', { entity, position })
    "
    @media-control="(entity, mp_action) => $emit('send', 'media_player', { entity, mp_action })"
    @scene-activate="(entity) => $emit('send', 'scene_activate', { entity })"
  />
</template>
<script setup lang="ts">
import { computed, inject, reactive } from 'vue'
import type { useHA } from '../../composables/useHA'
import { useDashboardControls } from '../../composables/useDashboardControls'
import { appConfig } from '../../composables/useInverterState'
import HomePanels from './HomePanels.vue'
const home = inject<ReturnType<typeof useHA>>('desktop-home')!
const { homeButtons } = useDashboardControls(home.getHaControlState)
const panels = reactive({
  ...home,
  homeButtons,
  appConfig,
  showWasher: computed(() => appConfig.value?.show_washer !== false),
  showDryer: computed(() => appConfig.value?.show_dryer !== false),
  showDishwasher: computed(() => appConfig.value?.show_dishwasher !== false),
})
defineEmits<{ send: [action: string, payload?: Record<string, unknown>] }>()
</script>
