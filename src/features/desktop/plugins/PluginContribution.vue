<template>
  <div class="min-w-0 break-words" :data-contribution-id="item.id">
    <p v-if="showTitle" class="text-[10px] font-medium text-muted">{{ item.title }}</p>
    <p v-if="item.kind === 'text'" class="text-[11px] text-main whitespace-pre-wrap">
      {{ item.text }}
    </p>
    <p v-else-if="item.kind === 'metric'" class="text-[11px] font-semibold text-main tabular">
      {{ item.value }}{{ item.unit }}
    </p>
    <output
      v-else-if="item.kind === 'status'"
      class="text-[11px] font-semibold"
      :class="statusClass[item.tone]"
    >
      {{ item.value }}
    </output>
    <template v-else-if="item.kind === 'action'">
      <UiButton
        class="plugin-action"
        size="sm"
        :disabled="disabled"
        :loading="pending"
        :aria-label="`${item.title}: ${item.label}`"
        @click="emit('action', item)"
      >
        <span class="min-w-0">{{ item.label }}</span>
      </UiButton>
      <p v-if="failed" role="alert" class="text-[10px] text-consumption mt-0.5">
        {{ $t('plugins.actionFailed') }}
      </p>
    </template>
    <PluginNumberInput
      v-else-if="item.kind === 'number_input'"
      :input="item"
      :disabled="disabled"
      :pending="pending"
      :failed="failed"
      @submit="(input, value) => emit('number', input, value)"
    />
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import PluginNumberInput from './PluginNumberInput.vue'
import type { ActionContribution, DashboardContribution, NumberInputContribution } from './types'

defineProps<{
  item: DashboardContribution
  showTitle: boolean
  disabled: boolean
  pending: boolean
  failed: boolean
}>()
const emit = defineEmits<{
  action: [action: ActionContribution]
  number: [input: NumberInputContribution, valueScaled: number]
}>()
const { t: $t } = useI18n()
const statusClass = {
  neutral: 'text-main',
  success: 'text-battery',
  warning: 'text-solar',
  error: 'text-consumption',
}
</script>

<style scoped>
.plugin-action {
  max-width: 100%;
  height: auto;
  padding-block: 0.2rem;
  line-height: 1.2;
  white-space: normal;
  overflow-wrap: anywhere;
}
</style>
