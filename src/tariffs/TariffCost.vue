<template>
  <span class="tariff-cost">
    <span v-if="cost !== null" :title="'Estimated energy cost; excludes taxes and other charges'"
      >≈ {{ money(cost) }}</span
    >
    <button v-if="!readOnly" type="button" class="tariff-open" @click="editorOpen = true">
      {{ plan ? 'Edit tariff' : 'Set tariff' }}
    </button>
    <span
      v-if="plan && cost === null"
      :title="'Time-of-use daily cost needs interval consumption; today’s total alone is insufficient.'"
      >{{ rateNow }} {{ plan.currency }}/kWh now</span
    >
    <span
      v-if="period"
      :title="`Billing period in ${plan?.timeZone}; energy invoice total requires interval data.`"
    >
      Billing period: {{ period.start }} – {{ period.end }}
    </span>
    <span v-if="error" role="alert">{{ error }}</span>
    <TariffEditor
      v-if="editorOpen && !readOnly"
      :plan="plan"
      :tariff-scope="tariffScope"
      @saved="saved"
      @close="editorOpen = false"
    />
  </span>
</template>
<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { billingPeriod, currentRate, estimateDailyCost, type TariffPlan } from './model'
import { loadTariff, tariffKey } from './storage'
const props = withDefaults(
  defineProps<{ kwh?: number; tariffScope: string; readOnly?: boolean }>(),
  {
    readOnly: false,
  }
)
const TariffEditor = defineAsyncComponent(() => import('./TariffEditor.vue'))
const editorOpen = ref(false)
const plan = ref<TariffPlan | null>(null)
const error = ref('')
const now = ref(new Date())
let timer: ReturnType<typeof setInterval> | undefined
function load() {
  const result = loadTariff(props.tariffScope)
  plan.value = result.plan
  error.value = result.error
}
watch(
  () => props.tariffScope,
  () => {
    editorOpen.value = false
    load()
  },
  { immediate: true }
)
function changed(event: StorageEvent) {
  if (event.key === tariffKey(props.tariffScope) || event.key === null) load()
}
onMounted(() => {
  window.addEventListener('storage', changed)
  timer = setInterval(() => {
    now.value = new Date()
  }, 30_000)
})
onBeforeUnmount(() => {
  window.removeEventListener('storage', changed)
  if (timer) clearInterval(timer)
})
const period = computed(() => (plan.value ? billingPeriod(plan.value, now.value) : null))
const cost = computed(() => estimateDailyCost(plan.value, props.kwh))
const rateNow = computed(() => (plan.value ? currentRate(plan.value, now.value).toFixed(4) : ''))
const money = (value: number) =>
  new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency: plan.value?.currency ?? 'USD',
  }).format(value)
function saved(value: TariffPlan | null) {
  plan.value = value
  error.value = ''
  editorOpen.value = false
}
</script>
<style scoped>
.tariff-cost {
  display: inline-flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
}
.tariff-open {
  text-decoration: underline;
  text-underline-offset: 3px;
  cursor: pointer;
}
</style>
