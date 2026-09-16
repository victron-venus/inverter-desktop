<template>
  <span class="inline-flex flex-col min-w-0">
    <UiButton
      size="sm"
      :disabled="!action || !context.dashboard.canAct(plugin)"
      :loading="context.pending(plugin, reference.id)"
      @click="context.runAction(plugin, reference.id)"
    >
      {{ reference.label }}
    </UiButton>
    <span
      v-if="context.failed(plugin, reference.id)"
      role="alert"
      class="text-[9px] text-consumption"
      >{{ $t('plugins.actionFailed') }}</span
    >
  </span>
</template>
<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import { actionContribution, usePluginPresentation } from './presentation'
import type { PluginSnapshot, PresentationAction } from './types'
const props = defineProps<{ plugin: PluginSnapshot; reference: PresentationAction }>()
const context = usePluginPresentation()
const action = computed(() => actionContribution(props.plugin, props.reference.id))
const { t: $t } = useI18n()
</script>
