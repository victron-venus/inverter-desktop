<template>
  <section aria-label="Electricity tariff" class="flex flex-col gap-2 text-[12px]">
    <h2 class="classic-section-title">Electricity tariff (optional)</h2>
    <p>
      Enter energy prices, seasonal months, a time zone and the billing period start day. This
      tariff is included in configuration backups. Without one, the dashboard uses the tariff
      supplied by inverter-control. A local dashboard override takes priority.
    </p>
    <p v-if="plan">
      {{ plan.name }} · {{ plan.currency }} · {{ plan.timeZone }}
      {{ plan.billingDay ? ` · Billing starts on day ${plan.billingDay}` : '' }}
    </p>
    <p v-if="error" role="alert">{{ error }}</p>
    <button type="button" class="classic-input" @click="editorOpen = true">
      {{ plan ? 'Edit configuration tariff' : 'Set configuration tariff' }}
    </button>
    <button
      v-if="modelValue != null"
      type="button"
      class="classic-input"
      @click="emit('update:modelValue', null)"
    >
      Use controller tariff
    </button>
    <TariffEditor
      v-if="editorOpen"
      :plan="plan"
      :persist="false"
      clear-label="Use controller tariff"
      @saved="apply"
      @close="editorOpen = false"
    />
  </section>
</template>
<script setup lang="ts">
import { computed, defineAsyncComponent, ref } from 'vue'
import { validatePlan, type TariffPlan } from '../tariffs/model'
const props = defineProps<{ modelValue?: unknown }>()
const emit = defineEmits<{ 'update:modelValue': [value: TariffPlan | null] }>()
const TariffEditor = defineAsyncComponent(() => import('../tariffs/TariffEditor.vue'))
const editorOpen = ref(false)
const parsed = computed(() => {
  if (props.modelValue == null) return { plan: null, error: '' }
  try {
    return { plan: validatePlan(props.modelValue), error: '' }
  } catch {
    return {
      plan: null,
      error:
        'This configuration tariff is invalid or newer than this app. It will not be applied. Import or enter a supported tariff to replace it.',
    }
  }
})
const plan = computed(() => parsed.value.plan)
const error = computed(() => parsed.value.error)
function apply(value: TariffPlan | null) {
  emit('update:modelValue', value)
  editorOpen.value = false
}
</script>
