<template>
  <div class="classic-card px-1.5 py-1 flex items-center gap-1 w-full">
    <div class="flex flex-wrap gap-1 items-center flex-1 min-w-0">
      <UiButton
        class="min-w-[28px]"
        size="sm"
        toggle
        :active="dryRun"
        @click="$emit('send', 'dry_run', { value: !dryRun })"
      >
        <FlaskConical :size="10" /> DRY
      </UiButton>

      <UiButton
        class="min-w-[45px]"
        size="sm"
        toggle
        :active="essClass === 'on'"
        @click="$emit('send', 'ess_mode')"
      >
        <Zap :size="10" /> {{ essText }}
      </UiButton>

      <template v-if="showHeaderToggles !== false && headerControls.length > 0">
        <div class="soft-divider mx-0.5"></div>

        <UiButton
          v-for="toggle in headerControls"
          :key="toggle.id"
          class="min-w-[55px]"
          size="sm"
          toggle
          :active="controlStates?.[toggle.id] === 'on'"
          :unavailable="isToggleUnavailable(controlStates?.[toggle.id])"
          @click="$emit('send', 'toggle', { entity: toggle.entity })"
        >
          {{ toggle.label }}
        </UiButton>
      </template>
    </div>

    <slot name="actions" />

    <UiButton
      class="min-w-[22px] !px-1.5 shrink-0"
      variant="ghost"
      :title="isDark ? 'Light mode' : 'Dark mode'"
      @click="$emit('toggle-theme')"
    >
      <Sun v-if="isDark" :size="11" />
      <Moon v-else :size="11" />
    </UiButton>
  </div>
</template>

<script setup lang="ts">
import { FlaskConical, Zap, Sun, Moon } from '@lucide/vue'
import UiButton from './UiButton.vue'
import type { DashboardControl } from '../inverterControl'

defineProps<{
  dryRun: boolean
  essClass: string
  essText: string
  headerControls: DashboardControl[]
  controlStates: Record<string, string> | undefined
  isDark: boolean
  showHeaderToggles?: boolean
}>()

defineEmits<{
  send: [action: string, payload?: Record<string, unknown>]
  'toggle-theme': []
}>()

function isToggleUnavailable(state: string | undefined): boolean {
  return state === 'unavailable'
}
</script>
