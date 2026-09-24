<template>
  <dialog
    ref="dialog"
    class="tariff-dialog"
    aria-labelledby="tariff-title"
    @cancel.prevent="emit('close')"
  >
    <div class="tariff-content">
      <header>
        <div>
          <h2 id="tariff-title">Electricity tariff</h2>
          <p>
            Weekly energy prices per kWh. Saved for this dashboard on this device; Emporia stays
            unchanged.
          </p>
        </div>
        <button type="button" aria-label="Close tariff editor" @click="emit('close')">Close</button>
      </header>
      <div class="tariff-fields">
        <label>Name<input v-model="draft.name" maxlength="120" /></label>
        <label>Currency<input v-model="draft.currency" maxlength="3" placeholder="USD" /></label>
        <label>Time zone<input v-model="draft.timeZone" placeholder="America/Los_Angeles" /></label>
      </div>
      <div class="tariff-actions">
        <label
          >Flat price / kWh<input v-model="flat" type="number" step="any" placeholder="0.31"
        /></label>
        <button type="button" @click="fill">Fill entire week</button>
        <label class="file-button"
          >Import tariff<input type="file" accept=".json,application/json" @change="importFile"
        /></label>
        <button type="button" @click="exportFile">Export tariff</button>
      </div>
      <output v-if="message" class="tariff-note">{{ message }}</output>
      <p v-if="draft.source === 'emporia'">
        Source: Emporia{{ draft.reference ? ` · utility plan ${draft.reference}` : '' }}. Review the
        imported copy against the app.
      </p>
      <p class="tariff-help">
        Rows are half-hour periods in the selected time zone; columns are Monday–Sunday. Paste
        prices into the grid or fill the week, then adjust peak periods. All cells need a price;
        zero is allowed.
      </p>
      <TariffSheet :key="sheetVersion" ref="sheet" :rates="draft.rates" @error="error = $event" />
      <p class="tariff-help">
        This estimate covers energy charges only. Seasonal rates, tiers, demand charges, fixed fees
        and taxes are not included. Daily time-of-use cost needs interval consumption data.
      </p>
      <p v-if="error" role="alert" class="tariff-error">{{ error }}</p>
      <footer>
        <button v-if="plan" type="button" @click="clear">Clear local tariff</button
        ><button type="button" @click="emit('close')">Cancel</button
        ><button type="button" class="tariff-save" :disabled="busy" @click="save">
          {{ busy ? 'Saving…' : 'Save tariff' }}
        </button>
      </footer>
    </div>
  </dialog>
</template>
<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import TariffSheet from './TariffSheet.vue'
import {
  importTariff,
  newDraft,
  rateGrid,
  validatePlan,
  type TariffDraft,
  type TariffPlan,
} from './model'
import { clearTariff, saveTariff } from './storage'

const props = defineProps<{ plan: TariffPlan | null; tariffScope: string }>()
const emit = defineEmits<{ close: []; saved: [plan: TariffPlan | null] }>()
const draft = ref<TariffDraft>(props.plan ? JSON.parse(JSON.stringify(props.plan)) : newDraft())
const dialog = ref<HTMLDialogElement | null>(null)
const sheet = ref<InstanceType<typeof TariffSheet> | null>(null)
const sheetVersion = ref(0)
const error = ref('')
const message = ref('')
const flat = ref('')
const busy = ref(false)
onMounted(() => dialog.value?.showModal())
onBeforeUnmount(() => dialog.value?.close())
const fail = (cause: unknown) => {
  error.value = cause instanceof Error ? cause.message : 'The tariff could not be read.'
}

function fill() {
  if (!String(flat.value).trim() || !Number.isFinite(Number(flat.value))) {
    error.value = 'Enter a numeric flat price first.'
    return
  }
  draft.value.rates = rateGrid(Number(flat.value))
  sheetVersion.value++
  error.value = ''
}
async function importFile(event: Event) {
  const input = event.target as HTMLInputElement
  try {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 100_000) throw new Error('Choose a tariff JSON file smaller than 100 KB.')
    const result = importTariff(JSON.parse(await file.text()))
    draft.value = result.draft
    message.value = result.message
    sheetVersion.value++
    error.value = ''
  } catch (cause) {
    fail(cause)
  } finally {
    input.value = ''
  }
}
async function readPlan() {
  if (!sheet.value) throw new Error('The spreadsheet is still loading.')
  return validatePlan({
    ...draft.value,
    currency: draft.value.currency.toUpperCase(),
    rates: await sheet.value.getRates(),
  })
}
async function save() {
  busy.value = true
  try {
    const plan = await readPlan()
    saveTariff(props.tariffScope, plan)
    emit('saved', plan)
  } catch (cause) {
    fail(cause)
  } finally {
    busy.value = false
  }
}
function clear() {
  try {
    clearTariff(props.tariffScope)
    emit('saved', null)
  } catch (cause) {
    fail(cause)
  }
}
async function exportFile() {
  try {
    const plan = await readPlan()
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(plan, null, 2)], { type: 'application/json' })
    )
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = 'electricity-tariff.json'
    anchor.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
    error.value = ''
  } catch (cause) {
    fail(cause)
  }
}
</script>
<style scoped>
.tariff-dialog {
  position: fixed;
  inset: 0;
  margin: auto;
  width: min(1100px, 96vw);
  max-width: 96vw;
  max-height: 94dvh;
  padding: 0;
  border: 1px solid #596372;
  border-radius: 12px;
  color: #e5eaf0;
  background: #171d26;
  font: 14px/1.5 system-ui;
  user-select: text;
}
.tariff-dialog::backdrop {
  background: #0009;
}
.tariff-content {
  padding: 20px;
}
.tariff-content header,
.tariff-content footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.tariff-content h2 {
  font-size: 22px;
  font-weight: 650;
  margin: 0;
}
.tariff-content p,
.tariff-content output {
  margin: 8px 0;
  color: #b9c4d2;
}
.tariff-fields {
  display: grid;
  grid-template-columns: 2fr 1fr 2fr;
  gap: 12px;
  margin: 16px 0;
}
.tariff-content label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.tariff-content input {
  min-width: 0;
  background: #242e3c;
  color: #f5f7fa;
  padding: 8px;
  border: 1px solid #64748b;
  border-radius: 5px;
}
.tariff-actions {
  display: flex;
  align-items: end;
  flex-wrap: wrap;
  gap: 10px;
}
.tariff-content button,
.file-button {
  border: 1px solid #64748b;
  border-radius: 5px;
  padding: 8px 12px;
  background: #283546;
  color: white;
  cursor: pointer;
}
.file-button input {
  max-width: 220px;
  font-size: 12px;
  padding: 4px;
}
.tariff-help {
  font-size: 12px;
}
.tariff-note {
  display: block;
  padding: 10px;
  background: #273649;
}
.tariff-content .tariff-error {
  color: #ffb4a8;
}
.tariff-content footer {
  justify-content: flex-end;
  margin-top: 14px;
}
.tariff-content .tariff-save {
  background: #25634b;
}
.tariff-content button:disabled {
  opacity: 0.6;
}
.tariff-content :focus-visible {
  outline: 2px solid #93c5fd;
  outline-offset: 2px;
}
@media (max-width: 600px) {
  .tariff-content {
    padding: 12px;
  }
  .tariff-fields {
    grid-template-columns: 1fr 1fr;
  }
  .tariff-fields label:last-child {
    grid-column: 1/-1;
  }
}
</style>
