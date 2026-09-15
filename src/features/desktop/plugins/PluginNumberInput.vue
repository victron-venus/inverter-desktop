<template>
  <form class="flex flex-col gap-1" :aria-busy="pending || undefined" @submit.prevent="submit">
    <label :for="fieldId" class="text-[10px] text-muted">{{ input.label }}</label>
    <output :id="`${fieldId}-observed`" class="text-[11px] text-main tabular">
      {{ $t('plugins.manager.numericObserved', { value: observed, unit }) }}
    </output>
    <p :id="`${fieldId}-range`" class="text-[10px] text-muted">
      {{ $t('plugins.manager.numericRange', range) }}
    </p>
    <div class="flex flex-wrap items-center gap-1">
      <input
        :id="fieldId"
        :value="draft"
        type="text"
        inputmode="decimal"
        autocomplete="off"
        maxlength="64"
        class="classic-input min-w-0 flex-1 text-[11px] tabular"
        :aria-label="`${input.title}: ${input.label}`"
        :aria-describedby="descriptionIds"
        :aria-invalid="(dirty && invalid) || undefined"
        :disabled="disabled || pending || needsReview || !valid"
        @input="edit(($event.target as HTMLInputElement).value)"
      />
      <UiButton
        type="submit"
        size="sm"
        :disabled="!canSubmit"
        :loading="pending"
        :aria-label="`${input.title}: ${$t('plugins.manager.numericApply')}`"
      >
        {{ $t('plugins.manager.numericApply') }}
      </UiButton>
    </div>
    <p v-if="needsReview" :id="`${fieldId}-review`" role="status" class="text-[10px] text-muted">
      {{ $t('plugins.manager.numericChanged') }}
    </p>
    <p
      v-else-if="dirty && invalid"
      :id="`${fieldId}-invalid`"
      role="alert"
      class="text-[10px] text-consumption"
    >
      {{ $t('plugins.manager.numericInvalid', range) }}
    </p>
    <UiButton
      v-if="needsReview || dirty"
      size="sm"
      class="self-start"
      :disabled="disabled || pending || !valid"
      @click="reset"
    >
      {{ $t(needsReview ? 'plugins.manager.numericReview' : 'plugins.manager.numericReset') }}
    </UiButton>
    <p v-if="failed" role="alert" class="text-[10px] text-consumption">
      {{ $t('plugins.actionFailed') }}
    </p>
    <p v-if="!valid" role="status" class="text-[10px] text-muted">
      {{ $t('plugins.unavailable') }}
    </p>
  </form>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../../components/UiButton.vue'
import {
  formatNumberValue,
  numberInputAcceptsValue,
  parseNumberDraft,
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
const emit = defineEmits<{ submit: [input: NumberInputContribution, valueScaled: number] }>()
const { t: $t } = useI18n()
const fieldId = useId()
const authority = shallowRef({ ...props.input })
const draft = ref(formatNumberValue(props.input.value_scaled, props.input.decimal_places))
const dirty = ref(false)
const needsReview = ref(false)
const valid = computed(() => validNumberInput(props.input))
const observed = computed(() =>
  formatNumberValue(props.input.value_scaled, props.input.decimal_places)
)
const unit = computed(() => (props.input.unit ? ` ${props.input.unit}` : ''))
const range = computed(() => ({
  min: formatNumberValue(props.input.min_scaled, props.input.decimal_places),
  max: formatNumberValue(props.input.max_scaled, props.input.decimal_places),
  step: formatNumberValue(props.input.step_scaled, props.input.decimal_places),
  unit: unit.value,
}))
const parsed = computed(() => parseNumberDraft(draft.value, authority.value.decimal_places))
const invalid = computed(
  () => parsed.value === null || !numberInputAcceptsValue(authority.value, parsed.value)
)
const canSubmit = computed(
  () =>
    !props.disabled &&
    !props.pending &&
    valid.value &&
    dirty.value &&
    !needsReview.value &&
    !invalid.value &&
    sameNumberInputAuthority(authority.value, props.input)
)
const descriptionIds = computed(() =>
  [
    `${fieldId}-observed`,
    `${fieldId}-range`,
    ...(needsReview.value ? [`${fieldId}-review`] : []),
    ...(dirty.value && invalid.value && !needsReview.value ? [`${fieldId}-invalid`] : []),
  ].join(' ')
)

watch(
  () => props.input,
  (input) => {
    if (!sameNumberInputAuthority(authority.value, input)) {
      // Keep the draft visible but never reuse it under newly advertised authority.
      needsReview.value = true
    } else if (!dirty.value && !needsReview.value) {
      draft.value = formatNumberValue(input.value_scaled, input.decimal_places)
    }
  },
  { deep: true }
)

function edit(value: string) {
  draft.value = value
  dirty.value = true
}

function reset() {
  if (props.disabled || props.pending || !valid.value) return
  authority.value = { ...props.input }
  draft.value = observed.value
  dirty.value = false
  needsReview.value = false
}

function submit() {
  if (!canSubmit.value || parsed.value === null) return
  emit('submit', { ...authority.value }, parsed.value)
}
</script>
