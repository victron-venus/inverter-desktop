<template>
  <UiButton
    v-if="configured"
    class="min-w-[22px] !px-1.5 shrink-0"
    toggle
    :active="config?.camera_enabled"
    :title="$t('actions.cameraMotion')"
    @click="toggle"
  >
    <Camera v-if="config?.camera_enabled" :size="11" />
    <CameraOff v-else :size="11" />
  </UiButton>
</template>
<script setup lang="ts">
import { computed } from 'vue'
import { Camera, CameraOff } from '@lucide/vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../components/UiButton.vue'
import { appConfig as config } from '../../composables/useInverterState'
import { cameraConnection } from './cameraConnection'
import { logger } from '../../logger'
const emit = defineEmits<{ error: [message: string] }>()
const { t: $t } = useI18n()
const configured = computed(() => !!config.value?.mqtt_ha_host?.trim())
async function toggle() {
  try {
    await cameraConnection.toggleCameraMotion()
  } catch (error) {
    logger.error('Failed to toggle camera motion:', error)
    emit('error', `Failed to toggle camera: ${String(error)}`)
  }
}
</script>
