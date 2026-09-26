<template>
  <section aria-label="Electricity tariff" class="flex flex-col gap-2 text-[12px]">
    <h2 class="classic-section-title">Controller electricity tariff</h2>
    <p>
      The controller stores energy prices, seasons and billing dates for every connected dashboard.
      Saving here takes effect immediately on the controller.
    </p>
    <p v-if="plan">{{ plan.name }} · {{ plan.currency }} · {{ plan.timeZone }}</p>
    <p v-else>No controller tariff is configured.</p>
    <p v-if="!writable">Connect to an updated inverter-control to edit the shared tariff.</p>
    <p v-if="error" role="alert">{{ error }}</p>
    <button type="button" class="classic-input" :disabled="!writable" @click="openEditor">
      {{ plan ? 'Edit controller tariff' : 'Set controller tariff' }}
    </button>
    <TariffEditor
      v-if="editorOpen"
      :plan="plan"
      :persist="false"
      :save-plan="save"
      clear-label="Clear controller tariff"
      @saved="saved"
      @close="editorOpen = false"
    />
  </section>
</template>
<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onBeforeUnmount, ref } from 'vue'
import { validatePlan, type TariffPlan } from '../tariffs/model'
import {
  readControllerTariff,
  saveControllerTariff,
  type ControllerTariff,
} from '../tariffs/controller'

const TariffEditor = defineAsyncComponent(() => import('../tariffs/TariffEditor.vue'))
const controller = ref<ControllerTariff>({ plan: null })
const editorOpen = ref(false)
const error = ref('')
const revision = ref('')
let mounted = true
let timer: ReturnType<typeof setTimeout> | undefined
const plan = computed(() => {
  try {
    return controller.value.plan == null ? null : validatePlan(controller.value.plan)
  } catch {
    return null
  }
})
const writable = computed(
  () => controller.value.status?.writable === true && !!controller.value.status?.revision
)
async function refresh() {
  try {
    const value = await readControllerTariff()
    if (!mounted) return
    if (value.plan != null) validatePlan(value.plan)
    controller.value = value
    error.value = ''
  } catch (cause) {
    if (mounted) {
      error.value = String(cause)
      controller.value = { ...controller.value, status: undefined }
    }
  }
}
async function poll() {
  if (!editorOpen.value) await refresh()
  if (mounted) timer = setTimeout(poll, 2000)
}
function openEditor() {
  revision.value = controller.value.status?.revision ?? ''
  editorOpen.value = true
}
async function save(value: TariffPlan | null) {
  await saveControllerTariff(value, revision.value)
  await refresh()
}
function saved() {
  editorOpen.value = false
}
onMounted(poll)
onBeforeUnmount(() => {
  mounted = false
  clearTimeout(timer)
})
</script>
