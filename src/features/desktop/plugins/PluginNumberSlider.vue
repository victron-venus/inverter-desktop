<template>
  <div class="flex flex-col gap-0.5">
    <input
      :id="id"
      type="range"
      :aria-label="input.title"
      :min="minimum"
      :max="maximum"
      :step="step"
      :value="observed"
      :disabled="disabled || pending || !valid"
      class="w-full h-1 accent-accent cursor-pointer disabled:opacity-40"
      @pointerdown="capture"
      @keydown="capture"
      @change="submit"
    />
    <p v-if="failed" role="alert" class="text-[10px] text-consumption">
      {{ $t('plugins.actionFailed') }}
    </p>
  </div>
</template>
<script setup lang="ts">
import { computed, shallowRef, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  formatNumberValue,
  parseNumberDraft,
  numberInputAcceptsValue,
  sameNumberInputAuthority,
  validNumberInput,
} from './numberInput'
import type { NumberInputContribution } from './types'
const props = defineProps<{
  input: NumberInputContribution
  disabled: boolean
  pending: boolean
  failed: boolean
}>()
const emit = defineEmits<{ submit: [input: NumberInputContribution, value: number] }>()
const id = useId()
const { t: $t } = useI18n()
const authority = shallowRef<NumberInputContribution | null>(null)
const valid = computed(() => validNumberInput(props.input))
const minimum = computed(() =>
  formatNumberValue(props.input.min_scaled, props.input.decimal_places)
)
const maximum = computed(() =>
  formatNumberValue(props.input.max_scaled, props.input.decimal_places)
)
const step = computed(() => formatNumberValue(props.input.step_scaled, props.input.decimal_places))
const observed = computed(() =>
  formatNumberValue(props.input.value_scaled, props.input.decimal_places)
)
function capture() {
  if (!props.disabled && !props.pending) authority.value = { ...props.input }
}
function submit(event: Event) {
  const input = authority.value
  authority.value = null
  if (
    !input ||
    props.disabled ||
    props.pending ||
    !valid.value ||
    !sameNumberInputAuthority(input, props.input)
  )
    return
  const value = parseNumberDraft((event.target as HTMLInputElement).value, input.decimal_places)
  if (value !== null && numberInputAcceptsValue(input, value)) emit('submit', input, value)
}
</script>
