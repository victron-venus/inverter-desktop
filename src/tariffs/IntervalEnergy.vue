<template>
  <dialog
    ref="dialog"
    class="interval-dialog"
    aria-labelledby="interval-title"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 id="interval-title">Interval energy cost</h2>
      <button type="button" @click="emit('close')">Close</button>
    </header>
    <p>
      Measured grid import × the selected tariff:
      <strong>{{ plan.name }}</strong> ({{ plan.timeZone }}).
    </p>
    <p>
      Energy charges only. Excludes export credits, taxes and fees. Confirm this tariff applied
      throughout the selected dates; changing it recalculates the entire period.
    </p>
    <label class="file-label"
      >Import measured intervals (CSV or JSON)
      <input
        type="file"
        accept=".csv,.json,text/csv,application/json"
        :disabled="busy"
        @change="importFile"
      />
    </label>
    <p>
      CSV columns: <code>start,end,import_kwh</code>. Use timestamps with Z or an explicit offset,
      and separate grid-import kWh. Do not use net energy, power, cumulative meters or daily totals.
      Import replaces this dashboard’s saved intervals.
    </p>
    <details>
      <summary>CSV example and data requirements</summary>
      <pre>
start,end,import_kwh
2026-09-24T00:00:00-07:00,2026-09-24T00:30:00-07:00,0.25
2026-09-24T00:30:00-07:00,2026-09-24T01:00:00-07:00,0</pre>
      <p>
        Example values are illustrative. Map your utility’s separate import/consumption export to
        these columns. Blank or absent readings are gaps; measured zero is valid. Intervals last at
        most one hour. Split at price boundaries using actual finer readings; never divide an
        aggregate arbitrarily.
      </p>
      <p>
        Emporia’s chart API currently omits MainsFromGrid history; net mains data cannot recover
        gross import. Automatic history synchronization is not available.
      </p>
    </details>
    <p v-if="error" role="alert">{{ error }}</p>
    <p v-if="message" role="status">{{ message }}</p>
    <template v-if="history">
      <div class="range">
        <label>First local date<input v-model="range.start" type="date" /></label>
        <label>Last local date (inclusive)<input v-model="range.end" type="date" /></label>
        <button type="button" @click="resetRange">
          {{ plan.billingDay ? 'Billing period of latest reading' : 'Day of latest reading' }}
        </button>
      </div>
      <p>
        {{ history.intervals.length }} intervals saved on this device for this dashboard. They are
        not uploaded or included in configuration backups.
      </p>
      <p v-if="result.error" role="alert">{{ result.error }}</p>
      <section v-if="result.value" class="result" aria-label="Interval cost result">
        <h3>
          {{ result.value.complete ? 'Period energy charge' : 'Partial energy charge' }}
        </h3>
        <strong class="amount">{{
          result.value.cost === null ? 'Unavailable' : money(result.value.cost)
        }}</strong>
        <p>
          {{ result.value.importKwh.toFixed(3) }} kWh priced ·
          {{ ((100 * result.value.pricedMinutes) / result.value.expectedMinutes).toFixed(1) }}% of
          period duration priced
        </p>
        <p v-if="result.value.missingMinutes > 0">
          {{ duration(result.value.missingMinutes) }} without readings, including any dates that
          have not elapsed. Missing consumption is not zero.
        </p>
        <p v-if="result.value.unpricedIntervals">
          {{ result.value.unpricedIntervals }} intervals excluded because they cross a price or
          selected-period boundary. Import finer measured intervals to price them.
        </p>
        <p v-if="!result.value.complete">
          This is a subtotal of supported readings, not the complete period cost.
        </p>
        <p v-else>
          Complete time coverage; accuracy still depends on the meter data and selected tariff.
        </p>
      </section>
      <button type="button" class="remove" @click="clear">Remove saved intervals</button>
    </template>
  </dialog>
</template>
<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import type { TariffPlan } from './model'
import {
  MAX_INTERVAL_BYTES,
  defaultRange,
  intervalCost,
  intervalKey,
  loadIntervals,
  parseIntervals,
  saveIntervals,
  type IntervalHistory,
} from './intervals'
const props = defineProps<{ plan: TariffPlan; tariffScope: string }>()
const emit = defineEmits<{ close: [] }>()
const dialog = ref<HTMLDialogElement | null>(null)
const history = ref<IntervalHistory | null>(null)
const range = ref({ start: '', end: '' })
const error = ref('')
const message = ref('')
const busy = ref(false)
let generation = 0
function resetRange() {
  if (history.value) range.value = defaultRange(props.plan, history.value)
}
function load() {
  generation++
  error.value = ''
  message.value = ''
  history.value = null
  try {
    history.value = loadIntervals(props.tariffScope)
    resetRange()
  } catch {
    error.value = 'Saved intervals could not be read. Import a valid file to replace them.'
  }
}
watch(() => props.tariffScope, load, { immediate: true })
// Equivalent tariff objects arrive with live telemetry; retain the selected dates.
const planJson = computed(() => JSON.stringify(props.plan))
const calculationPlan = computed(() => JSON.parse(planJson.value) as TariffPlan)
function storageChanged(event: StorageEvent) {
  if (event.key === null || event.key === intervalKey(props.tariffScope)) load()
}
onMounted(() => {
  dialog.value?.showModal()
  window.addEventListener('storage', storageChanged)
})
onBeforeUnmount(() => {
  generation++
  window.removeEventListener('storage', storageChanged)
})
async function importFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  const current = ++generation
  const scope = props.tariffScope
  busy.value = true
  error.value = ''
  message.value = ''
  try {
    if (file.size > MAX_INTERVAL_BYTES)
      throw new Error('Interval files must be no larger than 2 MB.')
    const parsed = parseIntervals(await file.text())
    if (current !== generation || scope !== props.tariffScope) return
    saveIntervals(scope, parsed)
    history.value = parsed
    resetRange()
    message.value =
      'Imported and saved measured intervals. Review the date range and coverage below.'
  } catch (cause) {
    if (current === generation)
      error.value = cause instanceof Error ? cause.message : 'Could not import interval data.'
  } finally {
    busy.value = false
    input.value = ''
  }
}
function clear() {
  try {
    localStorage.removeItem(intervalKey(props.tariffScope))
    generation++
    history.value = null
    error.value = ''
    message.value = 'Saved intervals removed.'
  } catch {
    error.value = 'Could not remove saved intervals on this device.'
  }
}
const result = computed(() => {
  if (!history.value) return { value: null, error: '' }
  try {
    return {
      value: intervalCost(calculationPlan.value, history.value, range.value),
      error: '',
    }
  } catch (cause) {
    return {
      value: null,
      error: cause instanceof Error ? cause.message : 'Invalid period.',
    }
  }
})
const money = (value: number) =>
  new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency: props.plan.currency,
  }).format(value)
const duration = (minutes: number) => `${(minutes / 60).toFixed(2)} hours`
</script>
<style scoped>
.interval-dialog {
  position: fixed;
  inset: 0;
  margin: auto;
  width: min(760px, 94vw);
  max-height: 90dvh;
  overflow: auto;
  padding: 24px;
  border: 1px solid #64748b;
  border-radius: 16px;
  background: #fff;
  color: #0f172a;
  font-size: 14px;
  line-height: 1.5;
}
.interval-dialog::backdrop {
  background: #0008;
}
header,
.range {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
}
header {
  justify-content: space-between;
}
h2 {
  font-size: 22px;
  font-weight: 700;
}
h3 {
  font-weight: 600;
}
p,
.range,
details {
  margin: 12px 0;
}
label {
  display: grid;
  gap: 6px;
}
input,
button {
  padding: 8px;
  border: 1px solid #94a3b8;
  border-radius: 6px;
}
button,
summary {
  cursor: pointer;
}
pre {
  overflow: auto;
  font-size: 12px;
}
.result {
  background: #f1f5f9;
  border-radius: 10px;
  padding: 16px;
}
.amount {
  font-size: 28px;
}
[role='alert'] {
  color: #b91c1c;
}
</style>
