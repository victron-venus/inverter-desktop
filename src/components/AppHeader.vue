<template>
  <div class="classic-card px-1.5 py-1 flex items-center gap-1 w-full">
    <div class="flex flex-wrap gap-1 items-center flex-1 min-w-0">
      <UiButton
        class="min-w-[28px]"
        size="sm"
        toggle
        :active="dryRun"
        @click="$emit('send', 'dry_run')"
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

      <template v-if="showHeaderToggles !== false && headerToggles.length > 0">
        <div class="soft-divider mx-0.5"></div>

        <UiButton
          v-for="toggle in headerToggles"
          :key="toggle.id"
          class="min-w-[55px]"
          size="sm"
          toggle
          :active="toggleStates?.[toggle.id] === 'on'"
          :unavailable="isToggleUnavailable(toggleStates?.[toggle.id])"
          @click="$emit('send', 'toggle', { entity: toggle.entity })"
        >
          {{ toggle.label }}
        </UiButton>
      </template>
    </div>

    <UiButton
      v-if="showCameraToggle"
      class="min-w-[22px] !px-1.5 shrink-0"
      toggle
      :active="cameraEnabled"
      :title="$t('actions.cameraMotion')"
      @click="$emit('toggle-camera')"
    >
      <Camera v-if="cameraEnabled" :size="11" />
      <CameraOff v-else :size="11" />
    </UiButton>

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
import { Camera, CameraOff, FlaskConical, Zap, Sun, Moon } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import UiButton from './UiButton.vue'
import { isHaUnavailableState } from '../utils'

defineProps<{
  dryRun: boolean
  essClass: string
  essText: string
  headerToggles: Array<{ id: string; label: string; entity: string }>
  toggleStates: Record<string, string> | undefined
  isDark: boolean
  showHeaderToggles?: boolean
  showCameraToggle?: boolean
  cameraEnabled?: boolean
}>()

defineEmits<{
  send: [action: string, payload?: Record<string, unknown>]
  'toggle-theme': []
  'toggle-camera': []
}>()

const { t: $t } = useI18n()

function isToggleUnavailable(state: string | undefined): boolean {
  return isHaUnavailableState(state)
}
</script>
